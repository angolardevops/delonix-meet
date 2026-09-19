//! Emissão em directo para plataformas externas (RTMP) — ver
//! `docs/adr/0003-directo-para-plataformas.md`.
//!
//! O QUE ESTE MÓDULO FAZ, E O QUE NÃO FAZ. Pela decisão do ADR (opção C), o
//! **browser compõe e codifica** a emissão: ecrã, câmara e convidados entram
//! num só canvas do lado do cliente, que o codifica em H.264 + Opus. Aqui só
//! se **remultiplexa** para RTMP — `-c:v copy` e `-c:a aac`.
//!
//! Porquê: o pod tem `limits.cpu: 1000m` (um core) e o ADR-0001 fixa a sala a
//! um pod. Um encode H.264 contínuo a 1080p30 gasta um a dois cores sozinho, e
//! saturá-lo degradaria exactamente a chamada que está a ser emitida. Copiar o
//! vídeo e transcodificar só o áudio cabe no core que já existe.
//!
//! O ffmpeg lê de **stdin**. É deliberado: mantém este módulo ignorante de
//! onde vem a media (hoje o browser, amanhã outra coisa) e torna-o testável com
//! um processo qualquer que leia de um cano.
//!
//! UM PROCESSO POR DESTINO (revisto a 2026-09-16, ver o ADR-0003). A primeira
//! versão tinha um só ffmpeg com N saídas `-f flv`, e mediram-se dois defeitos:
//!
//! 1. **Um destino mau parava todos.** Uma saída que falha termina o ffmpeg
//!    inteiro — «1 parado, 3 no ar» não podia existir.
//! 2. **Uma emissão parada bloqueava as do nó.** O stderr ia para um cano que
//!    ninguém lia (cheio aos 64 KiB, o ffmpeg pára), e `Registo::escrever`
//!    escrevia no stdin COM o lock do registo preso — a sala B ficava à espera
//!    da sala A. Reproduzido no teste `uma_emissao_parada_nao_bloqueia_outra_sala`.
//!
//! O desenho de agora:
//!
//! - O que chega do browser entra em [`Emissao::escrever`], que é **síncrono**
//!   e nunca espera: reparte o pedaço pelas filas de cada destino (`try`, com
//!   orçamento em bytes). Nenhum `await` acontece com um lock preso.
//! - Cada destino tem um **supervisor** que arranca o ffmpeg, drena o stderr
//!   (os erros viram `motivo`), lê o `-progress` do stdout (débito, frames
//!   descartados), e reinicia com **backoff limitado** quando cai.
//! - Um destino que não acompanha enche a sua fila, é morto e reiniciado — os
//!   outros não dão por nada.
//! - **Reentrar a meio do fluxo.** Um ffmpeg novo precisa do cabeçalho do
//!   Matroska (EBML + Tracks, que traz o SPS/PPS) e de começar num Cluster.
//!   Guarda-se o cabeçalho do início da emissão, e um destino reiniciado recebe
//!   esse cabeçalho seguido do fluxo a partir do próximo Cluster.
//!
//! O custo, dito: o áudio (Opus → AAC) é transcodificado uma vez POR DESTINO.
//! A 128 kbit/s é uma fracção de percentagem de um core cada, e o tecto
//! `MAX_DESTINOS_POR_DIRECTO` (8 no máximo) limita-o. O vídeo continua copiado.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, watch, Notify};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::signaling::Secret;

/// Um destino de emissão: para onde vai, e com que chave.
pub struct Destino {
    /// Ex.: `rtmp://a.rtmp.youtube.com/live2`. SEM a chave.
    pub url: String,
    pub chave: Secret,
    /// Nome só para a interface e para os logs — nunca influencia o comando.
    pub rotulo: String,
    /// Destino guardado da organização de onde este veio (se veio de um).
    pub id: Option<Uuid>,
    /// `youtube | facebook | linkedin | twitch | rtmp` — só informativo.
    pub platform: String,
}

/// O `Debug` derivado imprimiria a `url`, que é inofensiva, mas o hábito de
/// derivar `Debug` num tipo que carrega segredo é o que produziu o R43. Aqui é
/// explícito, e a chave nunca sai — o `Secret` já o garante, mas o tipo que o
/// contém tem de o dizer também.
impl fmt::Debug for Destino {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Destino")
            .field("rotulo", &self.rotulo)
            .field("url", &self.url)
            .field("chave", &self.chave)
            .field("id", &self.id)
            .finish()
    }
}

/// A plataforma a partir do host do URL. Só serve para mostrar (o ícone, o
/// payload do webhook): nunca muda o comando.
pub fn platform_from_url(url: &str) -> &'static str {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .unwrap_or_default();
    let tem = |dominio: &str| host == dominio || host.ends_with(&format!(".{dominio}"));
    if tem("youtube.com") {
        "youtube"
    } else if tem("facebook.com") {
        "facebook"
    } else if tem("linkedin.com") {
        "linkedin"
    } else if tem("twitch.tv") || tem("live-video.net") {
        "twitch"
    } else {
        "rtmp"
    }
}

/// Porque é que uma emissão foi recusada. Cada variante tem de dar uma razão
/// que o utilizador consiga agir — «falhou» não é uma razão.
#[derive(Debug, PartialEq, Eq)]
pub enum Recusa {
    /// A sala tem cifra ponta-a-ponta ligada.
    E2ee,
    /// O codec publicado não é remultiplexável para FLV.
    Codec { encontrado: String },
    /// Já se atingiu o tecto de emissões simultâneas do nó.
    Tecto { activas: usize, maximo: usize },
    /// Nenhum destino, ou um destino sem chave.
    SemDestino,
    /// O URL não é `rtmp://`/`rtmps://`, ou a chave tem caracteres que não
    /// cabem num URL. Recusa-se ANTES do ffmpeg: o ffmpeg aceita como saída um
    /// caminho de ficheiro (`/tmp/x`, `file:`), e sem esta regra o URL de um
    /// destino era uma escrita arbitrária de ficheiros no servidor.
    DestinoInvalido { rotulo: String },
    /// Mais destinos do que o nó admite numa só emissão (multi-canal tipo
    /// StreamYard, mas com tecto — cada destino a mais é mais uma ligação TCP,
    /// mais banda de saída e mais um processo do mesmo pod).
    DemasiadosDestinos { pedidos: usize, maximo: usize },
    /// O servidor não tem ffmpeg. É configuração em falta, não erro do
    /// utilizador — e dizê-lo pelo nome poupa uma investigação inteira a quem
    /// recebe a queixa. Mesma forma que o `causa_legivel` do recorder.
    SemFfmpeg,
}

impl fmt::Display for Recusa {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Emitir para o YouTube é, por definição, entregar a media a um
            // terceiro. Desligar o E2EE em silêncio para permitir o directo
            // seria mentir sobre a promessa central do produto — por isso
            // recusa-se, e diz-se porquê.
            Recusa::E2ee => f.write_str(
                "esta sala tem cifra ponta-a-ponta: emitir em directo entregaria a media à \
                 plataforma externa. Desliga o E2EE ao criar a sala se quiseres emitir.",
            ),
            Recusa::Codec { encontrado } => write!(
                f,
                "o directo copia o vídeo sem reencodificar e só sabe fazê-lo com H.264; \
                 chegou «{encontrado}»"
            ),
            Recusa::Tecto { activas, maximo } => write!(
                f,
                "este nó já tem {activas} emissões em directo (máximo {maximo})"
            ),
            Recusa::SemDestino => f.write_str("não foi indicado nenhum destino com chave"),
            Recusa::DestinoInvalido { rotulo } => write!(
                f,
                "o destino «{rotulo}» não é um endereço RTMP válido: o URL tem de começar por \
                 rtmp:// ou rtmps:// e a chave não pode ter espaços"
            ),
            Recusa::DemasiadosDestinos { pedidos, maximo } => write!(
                f,
                "pediram-se {pedidos} destinos para a mesma emissão; este nó aceita no \
                 máximo {maximo} de cada vez"
            ),
            Recusa::SemFfmpeg => f.write_str(
                "O servidor não tem o ffmpeg instalado, e sem ele não consegue emitir em \
                 directo. É uma configuração em falta no servidor — comunica-o a quem o \
                 administra.",
            ),
        }
    }
}

/// Os codecs de vídeo que se podem COPIAR para FLV sem reencodificar.
///
/// A lista é curta de propósito. O `recordable_codec` do SFU tem a mesma forma
/// e a mesma razão: um codec que o caminho não sabe tratar é RECUSADO com erro
/// escrito, nunca aceite para produzir lixo (ver `sfu.rs`).
pub fn copiavel_para_flv(mime: &str) -> bool {
    matches!(
        mime.to_ascii_lowercase().as_str(),
        "video/h264" | "video/avc"
    )
}

/// O URL é um endereço RTMP e a chave cabe num caminho de URL. Partilhada com
/// os destinos guardados (`stream_destinations.rs`): é a mesma regra.
pub fn rtmp_url_is_valid(url: &str) -> bool {
    let url = url.trim();
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    match url::Url::parse(url) {
        Ok(u) => {
            matches!(u.scheme(), "rtmp" | "rtmps")
                && u.host_str().is_some_and(|h| !h.is_empty())
                && u.username().is_empty()
                && u.password().is_none()
        }
        Err(_) => false,
    }
}

pub fn stream_key_is_valid(chave: &str) -> bool {
    let c = chave.trim();
    !c.is_empty() && !c.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Decide se uma emissão pode arrancar. Sem efeitos — é a função que os testes
/// atacam, e é a que se lê para saber as regras.
pub fn pode_emitir(
    e2ee_ligado: bool,
    mime_video: &str,
    destinos: &[Destino],
    activas: usize,
    maximo: usize,
    maximo_destinos: usize,
) -> Result<(), Recusa> {
    if e2ee_ligado {
        return Err(Recusa::E2ee);
    }
    if !copiavel_para_flv(mime_video) {
        return Err(Recusa::Codec {
            encontrado: mime_video.to_string(),
        });
    }
    if destinos.is_empty() || destinos.iter().any(|d| d.chave.expose().trim().is_empty()) {
        return Err(Recusa::SemDestino);
    }
    if let Some(d) = destinos
        .iter()
        .find(|d| !rtmp_url_is_valid(&d.url) || !stream_key_is_valid(d.chave.expose()))
    {
        return Err(Recusa::DestinoInvalido {
            rotulo: d.rotulo.clone(),
        });
    }
    if destinos.len() > maximo_destinos {
        return Err(Recusa::DemasiadosDestinos {
            pedidos: destinos.len(),
            maximo: maximo_destinos,
        });
    }
    if activas >= maximo {
        return Err(Recusa::Tecto { activas, maximo });
    }
    Ok(())
}

/// Junta o URL do destino à chave, sem barras a dobrar.
fn alvo(destino: &Destino) -> String {
    format!(
        "{}/{}",
        destino.url.trim().trim_end_matches('/'),
        destino.chave.expose().trim()
    )
}

/// Monta os argumentos do ffmpeg de UM destino.
///
/// Existe separado, e devolve `Vec<String>` em vez de um `Command`, para ser
/// testável sem um `ffmpeg` instalado — é o mesmo padrão do `recorder.rs`, e é
/// o que permite verificar que o vídeo é COPIADO (a decisão inteira do ADR
/// assenta nisso) sem precisar de media.
pub fn montar_argumentos(destino: &Destino, threads: u32) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        // O progresso vai para o STDOUT em `chave=valor`, legível por máquina
        // (é de onde saem o débito e os frames descartados), e o `-nostats`
        // tira do stderr a linha de estado que se reescreve a cada 500 ms —
        // no stderr fica só o que é erro, e é daí que sai o `motivo`.
        "-nostats".into(),
        "-progress".into(),
        "pipe:1".into(),
        // Sem isto o ffmpeg herda o stdin do servidor. Aqui o stdin É a media,
        // por isso `-nostdin` NÃO se usa — e é a diferença face ao recorder.
        "-threads".into(),
        threads.to_string(),
        // A media chega por um cano; o formato vai declarado porque o ffmpeg
        // não consegue procurar para trás num cano para o adivinhar.
        //
        // `matroska` e não `webm`, e é uma distinção medida: o browser produz
        // `video/webm;codecs=h264,opus`, mas o WebM oficialmente só admite
        // VP8/VP9/AV1 — o que sai é Matroska com H.264 lá dentro. O `ffprobe`
        // sobre um ficheiro real do MediaRecorder diz `format_name=matroska,webm`.
        // Com `-f webm` também funciona HOJE, porque o desmultiplexador de WebM
        // do ffmpeg É o de Matroska; mas é sorte, não contrato — uma build mais
        // estrita ou uma versão futura pode recusar H.264 declarado como WebM.
        "-f".into(),
        "matroska".into(),
        "-i".into(),
        "pipe:0".into(),
        // O VÍDEO É COPIADO. É a decisão inteira do ADR: sem isto o pod
        // codificaria H.264 em software e saturaria o core que serve a chamada.
        "-c:v".into(),
        "copy".into(),
        // O áudio TEM de ser transcodificado: o RTMP não transporta Opus.
        // É aritmética de brinquedo ao lado de um encode de vídeo.
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "128k".into(),
        "-ar".into(),
        "44100".into(),
        // Uma ligação RTMP que deixa de andar (rede parada, servidor que não
        // responde) sem isto pendura o ffmpeg para sempre, e o destino ficava
        // «a ligar» eternamente. Em microssegundos.
        "-rw_timeout".into(),
        "15000000".into(),
        // Num destino RTMP não há como voltar atrás no fim para escrever a
        // duração; sem esta flag o ffmpeg tenta e escreve um aviso a cada fecho.
        "-flvflags".into(),
        "no_duration_filesize".into(),
        "-f".into(),
        "flv".into(),
        alvo(destino),
    ]
}

// ---------------------------------------------------------------------------
//  Matroska: onde se pode (re)entrar no fluxo
// ---------------------------------------------------------------------------

const CLUSTER_ID: [u8; 4] = [0x1F, 0x43, 0xB6, 0x75];
const TIMESTAMP_ID: u8 = 0xE7;
/// Elemento CRC-32 (id 0xBF, tamanho 0x84 = 4 bytes). O muxer de Matroska do
/// ffmpeg põe-no como primeiro filho de cada Cluster; o do Chromium não.
const CRC32_HEAD: [u8; 2] = [0xBF, 0x84];
/// Bytes que um início de Cluster ocupa até ao Timestamp: 4 (id) + até 8
/// (tamanho) + 6 (CRC-32 opcional) + 1 (id do Timestamp).
const CLUSTER_PROBE: usize = 19;
/// Tecto do cabeçalho guardado. O do MediaRecorder tem centenas de bytes; se
/// não aparecer um Cluster até aqui, o fluxo não é o que se espera.
const HEADER_MAX: usize = 1024 * 1024;

/// Posição do primeiro início de Cluster em `buf`.
///
/// Procurar só os 4 bytes do id daria falsos positivos dentro dos dados de
/// vídeo (1 em 2³² por posição — a 4,5 Mbit/s, um por hora e pouco). Exige-se
/// também um tamanho EBML bem formado e, logo a seguir, o elemento Timestamp —
/// ou um CRC-32 e depois o Timestamp.
///
/// MEDIDO (2026-09-16, contra um RTMP a sério): a primeira versão só aceitava
/// o Timestamp logo a seguir, que é o que o MediaRecorder do Chromium escreve.
/// Com media gerada pelo ffmpeg (CRC-32 primeiro) nenhum Cluster era
/// reconhecido, e um destino reiniciado ficava «a ligar» para sempre à espera
/// de um ponto de entrada que nunca chegava.
pub fn cluster_start(buf: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + CLUSTER_ID.len() <= buf.len() {
        let p = i + buf[i..].windows(4).position(|w| w == CLUSTER_ID)?;
        if let Some(&b) = buf.get(p + 4) {
            let first_child = p + 4 + b.leading_zeros() as usize + 1;
            let timestamp_now = buf.get(first_child) == Some(&TIMESTAMP_ID);
            let crc_then_timestamp = buf.get(first_child..first_child + 2) == Some(&CRC32_HEAD[..])
                && buf.get(first_child + 6) == Some(&TIMESTAMP_ID);
            if b != 0 && (timestamp_now || crc_then_timestamp) {
                return Some(p);
            }
        }
        i = p + 1;
    }
    None
}

// ---------------------------------------------------------------------------
//  Estado por destino
// ---------------------------------------------------------------------------

/// O estado de um destino, como o cliente o vê.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DestinationState {
    /// O processo arrancou e ainda não saiu nenhum byte para o destino.
    #[serde(rename = "a-ligar")]
    Connecting,
    /// O ffmpeg reporta bytes a sair para o destino.
    #[serde(rename = "no-ar")]
    Live,
    /// Terminado: pela pessoa (sem `motivo`) ou porque se esgotaram as
    /// tentativas (com `motivo`). Não volta sozinho.
    #[serde(rename = "parado")]
    Stopped,
    /// Caiu, e vai tentar outra vez depois do backoff.
    #[serde(rename = "erro")]
    Error,
}

/// Um destino num instante — é o que vai no WebSocket, a intervalos.
///
/// Os nomes dos campos são contrato com `web/src/studio/directo.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct DestinationReport {
    /// Índice do destino no array que o cliente enviou.
    pub dest: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    pub rotulo: String,
    pub platform: String,
    pub estado: DestinationState,
    /// Débito de saída medido pelo ffmpeg, em kbit/s. 0 fora do ar.
    pub kbps: u32,
    /// Percentagem dos bytes recebidos do browser que NÃO chegaram ao ffmpeg
    /// deste destino (fila cheia, ou a reentrar depois de uma queda).
    pub perdas: f64,
    pub motivo: Option<String>,
    /// Quedas consecutivas desde a última vez que o destino esteve estável.
    pub tentativas: u32,
    /// `drop_frames` do ffmpeg (acumulado).
    pub frames_descartados: u64,
    /// Bytes escritos para o destino (acumulado entre reinícios).
    pub bytes_enviados: u64,
}

/// Reinícios e filas. Os valores por omissão são para produção; os testes
/// encolhem-nos.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    /// Quedas seguidas antes de desistir (o destino passa a `parado`).
    pub max_attempts: u32,
    /// Tempo no ar a partir do qual uma queda volta a contar do zero.
    pub stable_after: Duration,
    /// Orçamento da fila de cada destino. Cheia = o destino não acompanha.
    pub queue_max_bytes: usize,
    /// Quanto se espera, ao parar, que o ffmpeg feche em condições antes de
    /// o matar.
    pub stop_grace: Duration,
    /// Tempo máximo entre arrancar o processo e sair o primeiro byte.
    pub connect_timeout: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            max_attempts: 8,
            stable_after: Duration::from_secs(30),
            // ~14 s a 4,5 Mbit/s. Chega para um soluço de rede; mais do que
            // isso é memória do pod à espera de um destino que não volta.
            queue_max_bytes: 8 * 1024 * 1024,
            stop_grace: Duration::from_secs(5),
            // Maior que o `-rw_timeout` (15 s), para o ffmpeg dizer primeiro
            // porquê quando a causa é a rede.
            connect_timeout: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32 << attempt.saturating_sub(1).min(16);
        self.initial_backoff
            .saturating_mul(factor)
            .min(self.max_backoff)
    }
}

struct Status {
    state: DestinationState,
    reason: Option<String>,
    kbps: u32,
    frames_dropped: u64,
    /// Bytes das gerações anteriores do processo.
    bytes_before: u64,
    /// `total_size` da geração actual.
    bytes_now: u64,
    attempts: u32,
    ever_live: bool,
}

/// A ligação entre o repartidor e o processo ACTUAL de um destino.
struct Feed {
    tx: Option<mpsc::UnboundedSender<Bytes>>,
    queued: Arc<AtomicUsize>,
    kill: Option<Arc<Notify>>,
    /// O processo actual entrou a meio: espera um Cluster, e leva o cabeçalho.
    resync: bool,
    tail: Vec<u8>,
    overflowed: bool,
}

struct Output {
    index: usize,
    label: String,
    id: Option<Uuid>,
    platform: String,
    key: Secret,
    args: Vec<String>,
    feed: StdMutex<Feed>,
    received: AtomicU64,
    dropped: AtomicU64,
    status: StdMutex<Status>,
}

/// Lock que não propaga o envenenamento: um pânico num leitor de progresso não
/// pode deixar as outras emissões do nó sem conseguir ler o estado.
fn lock<T>(m: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Output {
    fn report(&self) -> DestinationReport {
        let s = lock(&self.status);
        let received = self.received.load(Ordering::Relaxed);
        let dropped = self.dropped.load(Ordering::Relaxed);
        let perdas = if received == 0 {
            0.0
        } else {
            ((dropped as f64 / received as f64) * 1000.0).round() / 10.0
        };
        DestinationReport {
            dest: self.index,
            id: self.id,
            rotulo: self.label.clone(),
            platform: self.platform.clone(),
            estado: s.state,
            kbps: if s.state == DestinationState::Live {
                s.kbps
            } else {
                0
            },
            perdas,
            motivo: s.reason.clone(),
            tentativas: s.attempts,
            frames_descartados: s.frames_dropped,
            bytes_enviados: s.bytes_before + s.bytes_now,
        }
    }

    /// Entrega um pedaço ao processo deste destino. Nunca espera.
    fn push(&self, data: &Bytes, header: Option<&Bytes>, queue_max: usize) {
        let n = data.len() as u64;
        self.received.fetch_add(n, Ordering::Relaxed);
        let mut f = lock(&self.feed);
        let Some(tx) = f.tx.clone() else {
            // A reiniciar: o que chega agora perde-se para este destino.
            self.dropped.fetch_add(n, Ordering::Relaxed);
            return;
        };
        let payload = if f.resync {
            let Some(header) = header else {
                // Sem cabeçalho guardado não há como reentrar.
                self.dropped.fetch_add(n, Ordering::Relaxed);
                return;
            };
            let mut joined = std::mem::take(&mut f.tail);
            joined.extend_from_slice(data);
            match cluster_start(&joined) {
                Some(p) => {
                    f.resync = false;
                    let sent = (joined.len() - p) as u64;
                    self.dropped
                        .fetch_add(n.saturating_sub(sent), Ordering::Relaxed);
                    let mut v = Vec::with_capacity(header.len() + joined.len() - p);
                    v.extend_from_slice(header);
                    v.extend_from_slice(&joined[p..]);
                    Bytes::from(v)
                }
                None => {
                    self.dropped.fetch_add(n, Ordering::Relaxed);
                    let keep = joined.len().min(CLUSTER_PROBE - 1);
                    f.tail = joined[joined.len() - keep..].to_vec();
                    return;
                }
            }
        } else {
            data.clone()
        };
        let len = payload.len();
        if f.queued.load(Ordering::Relaxed) + len > queue_max {
            // O processo não está a ler (rede parada, destino lento). Mata-se e
            // o supervisor reinicia-o — esperar por ele era parar os outros.
            self.dropped.fetch_add(n, Ordering::Relaxed);
            f.tx = None;
            f.overflowed = true;
            if let Some(k) = f.kill.take() {
                k.notify_one();
            }
            return;
        }
        f.queued.fetch_add(len, Ordering::Relaxed);
        if tx.send(payload).is_err() {
            f.queued.fetch_sub(len, Ordering::Relaxed);
            self.dropped.fetch_add(n, Ordering::Relaxed);
            f.tx = None;
        }
    }

    fn set_state(
        &self,
        state: DestinationState,
        reason: Option<String>,
        changes: &watch::Sender<u64>,
    ) {
        {
            let mut s = lock(&self.status);
            if s.state == state && s.reason == reason {
                return;
            }
            s.state = state;
            s.reason = reason;
            if state == DestinationState::Live {
                s.ever_live = true;
            }
        }
        changes.send_modify(|v| *v = v.wrapping_add(1));
    }
}

/// Um processo ffmpeg a correr, com as três pontas separadas.
struct Process {
    child: Child,
    /// `None` quando já foi ligado ao repartidor (o primeiro arranque liga-o
    /// em `Emissao::arrancar`, antes de a tarefa do supervisor correr).
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
    stderr: ChildStderr,
}

fn spawn_process(program: &str, args: &[String]) -> std::io::Result<Process> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    // Se este objecto for largado, o processo morre com ele. Um ffmpeg órfão
    // a empurrar para o YouTube é pior do que uma emissão que cai: consome o
    // core e ninguém sabe que existe.
    cmd.kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let io = |what: &str| std::io::Error::other(format!("o processo nasceu sem {what}"));
    let stdin = child.stdin.take().ok_or_else(|| io("stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| io("stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| io("stderr"))?;
    Ok(Process {
        child,
        stdin: Some(stdin),
        stdout,
        stderr,
    })
}

/// Liga um processo novo ao repartidor. Devolve o sinal de «mata-o».
fn attach(output: &Output, stdin: ChildStdin, resync: bool) -> Arc<Notify> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Bytes>();
    let queued = Arc::new(AtomicUsize::new(0));
    let kill = Arc::new(Notify::new());
    {
        let mut f = lock(&output.feed);
        f.tx = Some(tx);
        f.queued = queued.clone();
        f.kill = Some(kill.clone());
        f.resync = resync;
        f.tail.clear();
        f.overflowed = false;
    }
    // O ÚNICO sítio que espera pelo stdin do ffmpeg. Se o ffmpeg deixar de
    // ler, fica pendurada esta tarefa — e só esta.
    tokio::spawn(async move {
        let mut stdin = stdin;
        while let Some(b) = rx.recv().await {
            let r = stdin.write_all(&b).await;
            queued.fetch_sub(b.len(), Ordering::Relaxed);
            if r.is_err() {
                return;
            }
        }
        // Canal fechado: fim pedido. Fechar o stdin faz o ffmpeg terminar em
        // condições em vez de ser morto a meio.
        let _ = stdin.shutdown().await;
    });
    kill
}

/// Desliga o processo actual. Devolve se tinha transbordado.
fn detach(output: &Output) -> bool {
    let mut f = lock(&output.feed);
    f.tx = None;
    f.kill = None;
    f.resync = true;
    f.tail.clear();
    std::mem::take(&mut f.overflowed)
}

/// Lê o `-progress` do ffmpeg: débito, frames descartados, e a passagem a
/// «no ar» quando o primeiro byte sai para o destino.
async fn read_progress(stdout: ChildStdout, output: Arc<Output>, changes: watch::Sender<u64>) {
    let mut lines = BufReader::new(stdout).lines();
    let mut last: Option<(u64, Instant)> = None;
    while let Ok(Some(line)) = lines.next_line().await {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "total_size" => {
                let Ok(total) = v.trim().parse::<u64>() else {
                    continue;
                };
                let now = Instant::now();
                let kbps = match last {
                    Some((prev, t)) if total >= prev => {
                        let dt = now.duration_since(t).as_secs_f64();
                        (dt > 0.0).then(|| ((total - prev) as f64 * 8.0 / 1000.0 / dt) as u32)
                    }
                    _ => None,
                };
                let live = total > 0;
                {
                    let mut s = lock(&output.status);
                    s.bytes_now = total;
                    if let Some(k) = kbps {
                        s.kbps = k;
                    }
                }
                last = Some((total, now));
                if live {
                    output.set_state(DestinationState::Live, None, &changes);
                }
            }
            "drop_frames" => {
                if let Ok(d) = v.trim().parse::<u64>() {
                    lock(&output.status).frames_dropped = d;
                }
            }
            _ => {}
        }
    }
}

/// Quantas linhas do stderr se guardam para o `motivo`.
const STDERR_KEEP: usize = 4;
/// Tecto de uma linha: o que passar disto é cortado, nunca acumulado.
const STDERR_LINE_MAX: usize = 512;
/// Linhas por processo que vão para o log. As seguintes só se contam — um
/// ffmpeg em ciclo de erro não pode encher o log do nó.
const STDERR_LOG_MAX: usize = 20;

/// Drena o stderr SEMPRE (um cano cheio pára o ffmpeg) e guarda as últimas
/// linhas, com a chave de emissão redigida.
async fn drain_stderr(stderr: ChildStderr, key: String, label: String) -> VecDeque<String> {
    let mut keep: VecDeque<String> = VecDeque::new();
    let mut reader = stderr;
    let mut buf = [0u8; 8192];
    let mut line: Vec<u8> = Vec::new();
    let mut logged = 0usize;
    let mut finish = |line: &mut Vec<u8>, keep: &mut VecDeque<String>| {
        let texto = String::from_utf8_lossy(line).trim().to_string();
        line.clear();
        if texto.is_empty() {
            return;
        }
        let texto = redact(&texto, &key);
        if logged < STDERR_LOG_MAX {
            tracing::warn!(destino = %label, linha = %texto, "ffmpeg do directo");
        }
        logged += 1;
        if keep.len() == STDERR_KEEP {
            keep.pop_front();
        }
        keep.push_back(texto);
    };
    loop {
        match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                for &b in &buf[..n] {
                    if b == b'\n' || b == b'\r' {
                        finish(&mut line, &mut keep);
                    } else if line.len() < STDERR_LINE_MAX {
                        line.push(b);
                    }
                }
            }
        }
    }
    finish(&mut line, &mut keep);
    keep
}

/// Tira a chave de emissão de um texto. O ffmpeg põe o URL completo — com a
/// chave — nas mensagens de erro de saída (R43).
pub fn redact(texto: &str, key: &str) -> String {
    let key = key.trim();
    if key.is_empty() {
        texto.to_string()
    } else {
        texto.replace(key, "[chave]")
    }
}

/// Uma razão que a pessoa consiga agir, a partir do que o ffmpeg disse.
pub fn classify_failure(
    lines: &VecDeque<String>,
    exit: Option<std::process::ExitStatus>,
) -> String {
    let todo = lines
        .iter()
        .map(|l| l.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let tem = |padroes: &[&str]| padroes.iter().any(|p| todo.contains(p));
    if tem(&["connection refused"]) {
        "o servidor de destino recusou a ligação (endereço ou porta errados, ou o serviço está em baixo)".into()
    } else if tem(&["timed out", "timeout"]) {
        "o servidor de destino deixou de responder".into()
    } else if tem(&[
        "name or service not known",
        "failed to resolve",
        "cannot resolve",
        "temporary failure in name resolution",
    ]) {
        "o nome do servidor de destino não resolve".into()
    } else if tem(&[
        "403",
        "401",
        "forbidden",
        "unauthorized",
        "authentication",
        "badname",
        "publish",
    ]) {
        "o destino recusou a emissão — a chave está errada ou expirou".into()
    } else if tem(&["broken pipe", "connection reset", "end of file"]) {
        "o servidor de destino fechou a ligação".into()
    } else if tem(&["invalid data found", "ebml", "matroska"]) {
        "o fluxo de media chegou ilegível ao ffmpeg".into()
    } else if let Some(l) = lines.back() {
        let l: String = l.chars().take(200).collect();
        format!("o ffmpeg terminou: {l}")
    } else {
        match exit {
            Some(st) => format!("o ffmpeg terminou sem explicação ({st})"),
            None => "o ffmpeg terminou sem explicação".into(),
        }
    }
}

/// O ciclo de vida de UM destino: arrancar, vigiar, reiniciar, desistir.
async fn supervise(
    output: Arc<Output>,
    first: Process,
    // O sinal de «mata-o» do primeiro processo, criado em `arrancar`. Vem por
    // parâmetro e não lido do `Feed`: o repartidor pode já o ter consumido
    // (fila cheia) antes de esta tarefa correr, e um sinal perdido deixava o
    // processo parado vivo para sempre.
    mut first_kill: Option<Arc<Notify>>,
    program: String,
    policy: RetryPolicy,
    mut end: watch::Receiver<bool>,
    changes: watch::Sender<u64>,
) {
    let mut next = Some(first);
    loop {
        let proc = match next.take() {
            Some(p) => p,
            None => match spawn_process(&program, &output.args) {
                Ok(p) => {
                    output.set_state(DestinationState::Connecting, None, &changes);
                    p
                }
                Err(e) => {
                    let reason = if e.kind() == std::io::ErrorKind::NotFound {
                        Recusa::SemFfmpeg.to_string()
                    } else {
                        format!("o ffmpeg não arrancou: {e}")
                    };
                    if !retry_or_give_up(&output, &policy, reason, &mut end, &changes).await {
                        return;
                    }
                    continue;
                }
            },
        };
        let Process {
            mut child,
            stdin,
            stdout,
            stderr,
        } = proc;
        let kill = match stdin {
            // Um processo novo depois de uma queda entra a meio do fluxo.
            Some(stdin) => attach(&output, stdin, true),
            None => first_kill.take().unwrap_or_default(),
        };
        lock(&output.status).bytes_now = 0;
        let progress = tokio::spawn(read_progress(stdout, output.clone(), changes.clone()));
        let errors = tokio::spawn(drain_stderr(
            stderr,
            output.key.expose().to_string(),
            output.label.clone(),
        ));
        let started = Instant::now();

        enum Exit {
            Died(Option<std::process::ExitStatus>),
            Stopped,
        }
        // Vigia de arranque: um processo que não chega ao ar em
        // `connect_timeout` é dado como caído. Sem isto, um ffmpeg à espera de
        // media que nunca chega (ou de um servidor que aceita a ligação e não
        // responde) ficava «a ligar» para sempre e nunca contava como queda.
        let connect_deadline = tokio::time::sleep(policy.connect_timeout);
        tokio::pin!(connect_deadline);
        let mut watching = true;
        let mut stalled = false;
        let exit = loop {
            tokio::select! {
                st = child.wait() => break Exit::Died(st.ok()),
                _ = kill.notified() => {
                    let _ = child.start_kill();
                    break Exit::Died(child.wait().await.ok());
                }
                _ = until_true(&mut end) => break Exit::Stopped,
                _ = &mut connect_deadline, if watching => {
                    watching = false;
                    if lock(&output.status).state != DestinationState::Live {
                        stalled = true;
                        let _ = child.start_kill();
                        break Exit::Died(child.wait().await.ok());
                    }
                }
            }
        };
        let overflowed = detach(&output);
        match exit {
            Exit::Stopped => {
                // Fecho em condições: o canal fechou, o stdin fecha, o ffmpeg
                // escreve o que falta e sai. Se não sair, mata-se.
                if tokio::time::timeout(policy.stop_grace, child.wait())
                    .await
                    .is_err()
                {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                }
                let _ = progress.await;
                let _ = errors.await;
                fold_bytes(&output);
                output.set_state(DestinationState::Stopped, None, &changes);
                return;
            }
            Exit::Died(st) => {
                let _ = tokio::time::timeout(Duration::from_secs(1), progress).await;
                let lines = tokio::time::timeout(Duration::from_secs(1), errors)
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .unwrap_or_default();
                fold_bytes(&output);
                let reason = if stalled {
                    let causa = classify_failure(&lines, None);
                    format!(
                        "o destino não ficou no ar em {} s ({causa})",
                        policy.connect_timeout.as_secs()
                    )
                } else if overflowed {
                    format!(
                        "o destino não acompanhou o débito (fila de {} MB cheia) — a ligação ao \
                         servidor de destino está lenta ou parada",
                        policy.queue_max_bytes / (1024 * 1024)
                    )
                } else {
                    classify_failure(&lines, st)
                };
                tracing::warn!(
                    destino = %output.label,
                    motivo = %reason,
                    saida = ?st,
                    "um destino do directo caiu"
                );
                let was_stable = lock(&output.status).state == DestinationState::Live
                    && started.elapsed() >= policy.stable_after;
                if was_stable {
                    lock(&output.status).attempts = 0;
                }
                if !retry_or_give_up(&output, &policy, reason, &mut end, &changes).await {
                    return;
                }
            }
        }
    }
}

/// Espera até o sinal ficar a `true` (ou o emissor desaparecer). Existe para
/// não guardar o `watch::Ref` — que não é `Send` — do outro lado de um `await`.
async fn until_true(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|v| *v).await;
}

/// Passa os bytes da geração que acabou para o acumulado.
fn fold_bytes(output: &Output) {
    let mut s = lock(&output.status);
    s.bytes_before += std::mem::take(&mut s.bytes_now);
}

/// Conta a queda e espera o backoff. `false` = desistir (ou a emissão acabou).
async fn retry_or_give_up(
    output: &Output,
    policy: &RetryPolicy,
    reason: String,
    end: &mut watch::Receiver<bool>,
    changes: &watch::Sender<u64>,
) -> bool {
    if *end.borrow() {
        output.set_state(DestinationState::Stopped, None, changes);
        return false;
    }
    let attempts = {
        let mut s = lock(&output.status);
        s.attempts += 1;
        s.attempts
    };
    if attempts > policy.max_attempts {
        output.set_state(
            DestinationState::Stopped,
            Some(format!(
                "desistiu depois de {} tentativas: {reason}",
                policy.max_attempts
            )),
            changes,
        );
        return false;
    }
    output.set_state(DestinationState::Error, Some(reason), changes);
    tokio::select! {
        _ = tokio::time::sleep(policy.backoff(attempts)) => true,
        _ = until_true(end) => {
            output.set_state(DestinationState::Stopped, None, changes);
            false
        }
    }
}

/// O cabeçalho do Matroska, guardado do início da emissão.
#[derive(Default)]
struct Header {
    buf: Vec<u8>,
    ready: Option<Bytes>,
    gave_up: bool,
}

/// Uma emissão a decorrer: N destinos, cada um com o seu processo.
pub struct Emissao {
    outputs: Vec<Arc<Output>>,
    header: StdMutex<Header>,
    end: watch::Sender<bool>,
    changes: watch::Sender<u64>,
    tasks: StdMutex<Vec<JoinHandle<()>>>,
    policy: RetryPolicy,
    pub rotulos: Vec<String>,
    /// Quando a emissão arrancou (G1: `GET /api/rooms/{room_code}/live/status`).
    desde: chrono::DateTime<chrono::Utc>,
}

/// Retrato do estado vivo de uma emissão (G1) — o que `GET
/// /api/rooms/{room_code}/live/status` devolve.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct EmissaoEstado {
    /// Rótulos dos destinos desta emissão — não o estado de cada um: ver
    /// `report()` para o detalhe por destino.
    pub rotulos: Vec<String>,
    pub viva: bool,
    pub bytes_enviados: u64,
    /// Débito binário agregado agora (soma do `kbps` de cada destino no ar), em bit/s.
    pub bitrate_bps: u64,
    pub desde: chrono::DateTime<chrono::Utc>,
}

impl fmt::Debug for Emissao {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emissao")
            .field("rotulos", &self.rotulos)
            .field("destinos", &self.outputs.len())
            .finish()
    }
}

impl Emissao {
    /// Arranca um ffmpeg por destino. Não valida nada — quem chama passa pelo
    /// `pode_emitir` primeiro, e os testes atacam as duas coisas em separado.
    ///
    /// Os primeiros processos nascem AQUI, de forma síncrona: assim o primeiro
    /// pedaço do browser (o que traz o cabeçalho) já os encontra ligados. Um
    /// ffmpeg que não existe falha aqui, para todos, com `NotFound`.
    pub fn arrancar(
        destinos: &[Destino],
        threads: u32,
        programa: &str,
        policy: RetryPolicy,
    ) -> std::io::Result<Arc<Self>> {
        let (end, _) = watch::channel(false);
        let (changes, _) = watch::channel(0u64);
        let mut outputs = Vec::with_capacity(destinos.len());
        let mut firsts = Vec::with_capacity(destinos.len());
        for (index, d) in destinos.iter().enumerate() {
            let args = montar_argumentos(d, threads);
            // Erro num destino = erro da emissão inteira, e os processos já
            // arrancados morrem com o `Vec` (kill_on_drop).
            let p = spawn_process(programa, &args)?;
            outputs.push(Arc::new(Output {
                index,
                label: d.rotulo.clone(),
                id: d.id,
                platform: d.platform.clone(),
                key: Secret::new(d.chave.expose().to_string()),
                args,
                feed: StdMutex::new(Feed {
                    tx: None,
                    queued: Arc::new(AtomicUsize::new(0)),
                    kill: None,
                    resync: false,
                    tail: Vec::new(),
                    overflowed: false,
                }),
                received: AtomicU64::new(0),
                dropped: AtomicU64::new(0),
                status: StdMutex::new(Status {
                    state: DestinationState::Connecting,
                    reason: None,
                    kbps: 0,
                    frames_dropped: 0,
                    bytes_before: 0,
                    bytes_now: 0,
                    attempts: 0,
                    ever_live: false,
                }),
            }));
            firsts.push(p);
        }
        let emissao = Arc::new(Self {
            rotulos: destinos.iter().map(|d| d.rotulo.clone()).collect(),
            outputs,
            header: StdMutex::new(Header::default()),
            end,
            changes,
            tasks: StdMutex::new(Vec::new()),
            policy: policy.clone(),
            desde: chrono::Utc::now(),
        });
        let mut tasks = Vec::with_capacity(firsts.len());
        for (output, mut first) in emissao.outputs.iter().zip(firsts) {
            // O stdin liga-se já, antes de a tarefa correr: nenhum byte do
            // início se perde à espera do escalonador.
            let kill = first.stdin.take().map(|stdin| attach(output, stdin, false));
            tasks.push(tokio::spawn(supervise(
                output.clone(),
                first,
                kill,
                programa.to_string(),
                policy.clone(),
                emissao.end.subscribe(),
                emissao.changes.clone(),
            )));
        }
        *lock(&emissao.tasks) = tasks;
        Ok(emissao)
    }

    /// Empurra media para todos os destinos. **Síncrono e sem esperas**: é a
    /// correcção do bloqueio — um destino parado nunca segura quem escreve.
    pub fn escrever(&self, dados: &Bytes) {
        let header = self.capture_header(dados);
        for o in &self.outputs {
            o.push(dados, header.as_ref(), self.policy.queue_max_bytes);
        }
    }

    fn capture_header(&self, dados: &[u8]) -> Option<Bytes> {
        let mut h = lock(&self.header);
        if h.ready.is_none() && !h.gave_up {
            h.buf.extend_from_slice(dados);
            if let Some(p) = cluster_start(&h.buf) {
                let ready = Bytes::copy_from_slice(&h.buf[..p]);
                h.ready = Some(ready);
                h.buf = Vec::new();
            } else if h.buf.len() > HEADER_MAX {
                tracing::warn!(
                    "o directo não trouxe um Cluster de Matroska no primeiro MiB: um destino que \
                     caia não vai conseguir reentrar"
                );
                h.gave_up = true;
                h.buf = Vec::new();
            }
        }
        h.ready.clone()
    }

    /// O estado de cada destino, agora.
    pub fn report(&self) -> Vec<DestinationReport> {
        self.outputs.iter().map(|o| o.report()).collect()
    }

    /// Retrato agregado da emissão (G1): bytes totais e débito agregado agora.
    /// `viva` é falso só quando TODOS os destinos pararam (pela pessoa ou por
    /// desistência) — um destino ainda a tentar conta como viva.
    pub fn estado(&self) -> EmissaoEstado {
        let relatorios = self.report();
        let bytes_enviados: u64 = relatorios.iter().map(|r| r.bytes_enviados).sum();
        let bitrate_bps: u64 = relatorios.iter().map(|r| r.kbps as u64 * 1000).sum();
        let viva = relatorios
            .iter()
            .any(|r| r.estado != DestinationState::Stopped);
        EmissaoEstado {
            rotulos: self.rotulos.clone(),
            viva,
            bytes_enviados,
            bitrate_bps,
            desde: self.desde,
        }
    }

    /// Muda sempre que um destino muda de estado.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    /// Todos os destinos desistiram (e a emissão não foi parada pela pessoa).
    pub fn all_gave_up(&self) -> bool {
        !*self.end.borrow()
            && self
                .outputs
                .iter()
                .all(|o| lock(&o.status).state == DestinationState::Stopped)
    }

    /// Chegou a estar algum destino no ar?
    pub fn ever_live(&self) -> bool {
        self.outputs.iter().any(|o| lock(&o.status).ever_live)
    }

    /// Fecha todos os destinos e espera pelos processos. Idempotente.
    pub async fn parar(&self) -> Vec<DestinationReport> {
        // `send_replace` e não `send`: o `send` não muda o valor quando já não
        // há receptores (todos os supervisores terminaram), e o «parado pela
        // pessoa» perdia-se.
        self.end.send_replace(true);
        let tasks = std::mem::take(&mut *lock(&self.tasks));
        for t in tasks {
            let _ = t.await;
        }
        self.report()
    }
}

impl Drop for Emissao {
    fn drop(&mut self) {
        self.end.send_replace(true);
    }
}

// ---------------------------------------------------------------------------
//  Registo das emissões vivas do nó
// ---------------------------------------------------------------------------

/// As emissões a decorrer neste pod, por sala.
///
/// Por sala e não por utilizador: uma sala emite uma vez. Dois anfitriões a
/// carregar em «ir para o ar» ao mesmo tempo dariam dois ffmpeg a empurrar para
/// a mesma chave, e a plataforma externa corta os dois.
///
/// O lock só protege o MAPA: nenhuma operação espera com ele preso. Antes, o
/// `escrever` escrevia no ffmpeg com o lock preso, e uma sala parada parava o nó.
#[derive(Default)]
pub struct Registo {
    activas: StdMutex<HashMap<Uuid, Arc<Emissao>>>,
}

impl Registo {
    pub fn quantas(&self) -> usize {
        lock(&self.activas).len()
    }

    pub fn tem(&self, sala: Uuid) -> bool {
        lock(&self.activas).contains_key(&sala)
    }

    /// Retrato do estado vivo da emissão da sala (G1). `None` = não há
    /// emissão activa — quem chama devolve 404, não um `EmissaoEstado` vazio.
    pub fn estado(&self, sala: Uuid) -> Option<EmissaoEstado> {
        let e = lock(&self.activas).get(&sala).cloned()?;
        Some(e.estado())
    }

    /// Regista uma emissão. Devolve `false` se a sala já tinha uma — quem
    /// chama trata isso como recusa, não como sucesso silencioso.
    pub fn inserir(&self, sala: Uuid, e: Arc<Emissao>) -> bool {
        let mut m = lock(&self.activas);
        if m.contains_key(&sala) {
            return false;
        }
        m.insert(sala, e);
        true
    }

    /// Regista respeitando o tecto do nó, na MESMA secção crítica: dois
    /// pedidos simultâneos não passam os dois por um `quantas()` antigo.
    pub fn inserir_com_tecto(
        &self,
        sala: Uuid,
        e: Arc<Emissao>,
        maximo: usize,
    ) -> Result<(), String> {
        let mut m = lock(&self.activas);
        if m.contains_key(&sala) {
            return Err("esta sala já está em directo".into());
        }
        if m.len() >= maximo {
            return Err(Recusa::Tecto {
                activas: m.len(),
                maximo,
            }
            .to_string());
        }
        m.insert(sala, e);
        Ok(())
    }

    /// Empurra media para a emissão da sala. `false` = não há emissão.
    pub fn escrever(&self, sala: Uuid, dados: &Bytes) -> bool {
        let e = lock(&self.activas).get(&sala).cloned();
        match e {
            Some(e) => {
                e.escrever(dados);
                true
            }
            None => false,
        }
    }

    /// Tira a emissão do registo e fecha-a. `None` se não existir.
    pub async fn parar(&self, sala: Uuid) -> Option<Vec<DestinationReport>> {
        let e = lock(&self.activas).remove(&sala)?;
        Some(e.parar().await)
    }
}

// ---------------------------------------------------------------------------
//  Rota: o browser empurra a emissão já composta por WebSocket
// ---------------------------------------------------------------------------

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

use crate::error::ApiError;
use crate::AppState;

/// De quanto em quanto tempo o estado dos destinos vai ao cliente, mesmo sem
/// mudanças (o débito muda sem mudar o estado).
const REPORT_EVERY: Duration = Duration::from_secs(2);
/// Um cliente que não lê o estado durante isto é dado como perdido.
const REPORT_SEND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
pub struct DirectoQuery {
    /// Token de sala, o mesmo que o `/ws` usa — curto e com âmbito.
    pub token: String,
    /// Os destinos, como JSON: `[{"url":"...","chave":"...","rotulo":"..."}]`
    /// ou, para um destino guardado da organização, `[{"id":"<uuid>"}]`.
    ///
    /// Um array e não N parâmetros nomeados (`destino1`, `chave1`,
    /// `destino2`…): um WebSocket não tem corpo, a query é o único lugar, e
    /// um array cresce para o multi-canal (tipo StreamYard) sem inventar
    /// esquema novo por cada plataforma a mais. O parsing é manual (ver
    /// `ws_directo`) e não `#[derive(Deserialize)]` num `Vec<DestinoBruto>`
    /// directo no extractor: um JSON malformado tem de dar a MESMA recusa
    /// legível pós-upgrade que as outras regras — um erro do extractor do
    /// axum falha ANTES do upgrade, e é exactamente o que o comentário em
    /// `ws_directo` explica que fica invisível para o browser.
    pub destinos: String,
    /// MIME do vídeo que o browser vai empurrar, para se poder recusar ANTES
    /// de arrancar o ffmpeg.
    pub codec: String,
}

/// A forma solta que chega na query, antes de a chave virar `Secret`.
#[derive(Deserialize)]
struct DestinoBruto {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    chave: Option<String>,
    #[serde(default)]
    rotulo: Option<String>,
    /// Destino guardado da organização. Com `id`, o URL e a chave vêm da base
    /// (decifrada no servidor) e os do pedido são ignorados — é o que deixa a
    /// chave nunca mais voltar ao browser depois de guardada.
    #[serde(default)]
    id: Option<Uuid>,
}

/// `GET /api/rooms/{code}/broadcast` (upgrade para WebSocket).
///
/// O browser compõe, codifica em H.264 e empurra pedaços de WebM por aqui; o
/// servidor remultiplexa para RTMP. Ver o ADR-0003.
///
/// **Mensagens do servidor** (tramas de texto, JSON):
///
/// - `{"erro": "<frase>"}` — recusa antes de emitir, ou fim porque nenhum
///   destino ficou no ar; o servidor fecha a seguir.
/// - `{"tipo": "destinos", "destinos": [DestinationReport…]}` — a cada 2 s e
///   sempre que um destino muda de estado. Um cliente antigo ignora-a.
pub async fn ws_directo(
    State(state): State<Arc<AppState>>,
    Path(codigo): Path<String>,
    Query(q): Query<DirectoQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let claims = crate::auth::verify_jwt(&state.config.jwt_secret, &q.token, "room")?;
    let sala_id = claims.room.ok_or(ApiError::Unauthorized)?;
    let user_id = claims.sub;

    // Não há helper partilhado de leitura por código — cada handler consulta o
    // que precisa, e aqui precisa-se só do id (para conferir com o token) e do
    // `e2ee` (a primeira regra de recusa).
    let (id_bd, e2ee): (Uuid, bool) = sqlx::query_as("SELECT id, e2ee FROM rooms WHERE code = $1")
        .bind(codigo.to_lowercase())
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    if id_bd != sala_id {
        return Err(ApiError::Unauthorized);
    }

    // As regras correm ANTES de gastar um processo — mas a recusa é ENTREGUE
    // depois do upgrade, e é uma distinção que se descobre a testar.
    //
    // Um erro HTTP devolvido antes do upgrade NÃO chega ao browser: a API de
    // WebSocket não expõe o estado nem o corpo de um handshake falhado, e o
    // `onclose` traz `reason` vazio. Medido: a razão «esta sala tem cifra
    // ponta-a-ponta…» chegava ao cliente como «não foi possível ligar» — ou
    // seja, a recusa mais importante do ADR-0003 era invisível a quem a devia
    // ler. Por isso aceita-se o upgrade e manda-se a razão numa trama de texto.
    // O JSON dos destinos segue a MESMA regra: um parse malformado tem de
    // chegar como razão legível, não como um 400 antes do upgrade.
    let recusa = |ws: WebSocketUpgrade, m: String| Ok(ws.on_upgrade(move |s| recusar(s, m)));
    let brutos = match serde_json::from_str::<Vec<DestinoBruto>>(&q.destinos) {
        Ok(b) => b,
        Err(e) => return recusa(ws, format!("os destinos vieram malformados: {e}")),
    };
    // O tecto conta ANTES de ir à base: mil ids não podem ser mil consultas.
    if brutos.len() > state.config.max_destinos_por_directo {
        let r = Recusa::DemasiadosDestinos {
            pedidos: brutos.len(),
            maximo: state.config.max_destinos_por_directo,
        };
        return recusa(ws, r.to_string());
    }
    if e2ee {
        // A recusa do E2EE ganha a todas, e não se decifra uma chave para nada.
        return recusa(ws, Recusa::E2ee.to_string());
    }
    // Destinos guardados: um só pedido em lote (não N, um por `id`) — a
    // organização vem de quem pede, nunca do corpo do pedido.
    let ids_guardados: Vec<Uuid> = brutos.iter().filter_map(|b| b.id).collect();
    let mut resolvidos: std::collections::HashMap<Uuid, (String, String, String, String)> =
        std::collections::HashMap::new();
    if !ids_guardados.is_empty() {
        let org_id: Option<Uuid> = crate::org::orgs_of_user(&state, user_id)
            .await
            .first()
            .copied();
        let Some(org_id) = org_id else {
            return recusa(
                ws,
                "sem organização: não pode usar destinos guardados".into(),
            );
        };
        match crate::stream_destinations::resolve_for_broadcast(&state, org_id, &ids_guardados)
            .await
        {
            Ok(rows) => {
                for (id, url, chave, label, kind) in rows {
                    resolvidos.insert(id, (url, chave, label, kind));
                }
            }
            Err(_) => return recusa(
                ws,
                "um dos destinos guardados não existe, não é desta organização, ou não está pronto"
                    .into(),
            ),
        }
    }
    let mut destinos = Vec::with_capacity(brutos.len());
    for b in brutos {
        match b.id {
            Some(id) => {
                let Some((url, chave, label, kind)) = resolvidos.remove(&id) else {
                    return recusa(
                        ws,
                        "um dos destinos guardados não existe, não é desta organização, ou não está pronto"
                            .into(),
                    );
                };
                let rotulo = b.rotulo.filter(|r| !r.trim().is_empty()).unwrap_or(label);
                destinos.push(Destino {
                    platform: kind,
                    url,
                    chave: Secret::new(chave),
                    rotulo,
                    id: Some(id),
                });
            }
            None => {
                let url = b.url.unwrap_or_default();
                destinos.push(Destino {
                    platform: platform_from_url(&url).into(),
                    url,
                    chave: Secret::new(b.chave.unwrap_or_default()),
                    rotulo: b.rotulo.unwrap_or_else(|| "directo".into()),
                    id: None,
                });
            }
        }
    }

    let activas = state.directos.quantas();
    let mut motivo: Option<String> = match pode_emitir(
        e2ee,
        &q.codec,
        &destinos,
        activas,
        state.config.max_directos,
        state.config.max_destinos_por_directo,
    ) {
        Err(r) => {
            tracing::warn!(sala = %codigo, motivo = ?r, "directo recusado");
            Some(r.to_string())
        }
        Ok(()) => None,
    };
    if motivo.is_none() && state.directos.tem(sala_id) {
        motivo = Some("esta sala já está em directo".into());
    }
    if let Some(m) = motivo {
        return recusa(ws, m);
    }

    // Os processos só arrancam DEPOIS do upgrade. Antes arrancavam aqui, e um
    // cliente que desistisse a meio do handshake deixava um ffmpeg registado
    // que ninguém parava: o `on_upgrade` nunca corria.
    Ok(ws.on_upgrade(move |socket| emitir(socket, state, sala_id, codigo, user_id, destinos)))
}

/// Entrega a razão da recusa e fecha.
///
/// A trama de TEXTO é o único caminho pelo qual uma frase inteira chega ao
/// browser: o `reason` do `close` está limitado a 123 bytes e é truncado sem
/// aviso, e estas mensagens são frases de propósito — a do E2EE explica o
/// porquê E o que fazer.
async fn recusar(mut socket: WebSocket, motivo: String) {
    let corpo = serde_json::json!({ "erro": motivo }).to_string();
    let _ = socket.send(Message::Text(corpo.into())).await;
    // Fechar é enviar a trama de Close: o `WebSocket` do axum não tem `close()`,
    // e largar o socket sem a enviar deixa o browser com um 1006 sem razão.
    let _ = socket.send(Message::Close(None)).await;
}

/// Arranca os processos e regista a emissão; depois, `bombear`.
async fn emitir(
    socket: WebSocket,
    state: Arc<AppState>,
    sala: Uuid,
    codigo: String,
    user_id: Uuid,
    destinos: Vec<Destino>,
) {
    let emissao = match Emissao::arrancar(
        &destinos,
        state.config.directo_threads,
        &state.config.ffmpeg_bin,
        RetryPolicy::default(),
    ) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Um ffmpeg em falta é CONFIGURAÇÃO, não avaria: merece a mesma
            // mensagem nomeada que o recorder dá, em vez de um erro opaco que
            // manda quem recebe a queixa investigar do zero.
            tracing::error!(sala = %codigo, binario = %state.config.ffmpeg_bin, "ffmpeg-ausente");
            return recusar(socket, Recusa::SemFfmpeg.to_string()).await;
        }
        Err(e) => {
            tracing::error!(sala = %codigo, erro = %e, "a emissão não arrancou");
            return recusar(socket, format!("não foi possível arrancar a emissão: {e}")).await;
        }
    };
    if let Err(m) =
        state
            .directos
            .inserir_com_tecto(sala, emissao.clone(), state.config.max_directos)
    {
        emissao.parar().await;
        return recusar(socket, m).await;
    }
    tracing::info!(sala = %codigo, destinos = ?destinos, "directo a começar");
    let platforms: Vec<String> = destinos.iter().map(|d| d.platform.clone()).collect();
    drop(destinos); // as chaves não ficam vivas mais do que o necessário
    bombear(socket, state, sala, codigo, user_id, emissao, platforms).await
}

/// Empurra o que chega do browser para os destinos, e o estado deles para o
/// browser, até um dos lados fechar.
async fn bombear(
    socket: WebSocket,
    state: Arc<AppState>,
    sala: Uuid,
    codigo: String,
    user_id: Uuid,
    emissao: Arc<Emissao>,
    platforms: Vec<String>,
) {
    let (mut sink, mut stream) = socket.split();
    let published: Arc<StdMutex<Option<chrono::DateTime<chrono::Utc>>>> =
        Arc::new(StdMutex::new(None));
    let (dead_tx, mut dead_rx) = watch::channel(false);

    // O estado vai por uma tarefa própria: um cliente lento a ler o estado
    // não atrasa a media, e a media não atrasa o estado.
    let reporter = {
        let emissao = emissao.clone();
        let state = state.clone();
        let codigo = codigo.clone();
        let published = published.clone();
        let platforms = platforms.clone();
        tokio::spawn(async move {
            let mut changes = emissao.subscribe();
            let mut tick = tokio::time::interval(REPORT_EVERY);
            let mut live_summary: Option<usize> = None;
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    r = changes.changed() => if r.is_err() { break },
                }
                let reports = emissao.report();
                let no_ar = reports
                    .iter()
                    .filter(|r| r.estado == DestinationState::Live)
                    .count();
                let started_at = {
                    let mut p = lock(&published);
                    if no_ar > 0 && p.is_none() {
                        let agora = chrono::Utc::now();
                        *p = Some(agora);
                        fire_stream_event(
                            &state,
                            user_id,
                            "stream.published",
                            format!("Em directo: sala {codigo}"),
                            serde_json::json!({
                                "room_id": sala,
                                "room_code": codigo,
                                "started_at": agora,
                                "by_user_id": user_id,
                                "destinations": webhook_destinations(&reports),
                            }),
                        );
                    }
                    *p
                };
                if let Some(started_at) = started_at {
                    if live_summary != Some(no_ar) {
                        live_summary = Some(no_ar);
                        announce_live(
                            &state,
                            sala,
                            Some(LiveInfo {
                                started_at,
                                by_user_id: user_id,
                                destinations_live: no_ar,
                                destinations_total: reports.len(),
                                platforms: reports
                                    .iter()
                                    .filter(|r| r.estado == DestinationState::Live)
                                    .map(|r| platforms.get(r.dest).cloned().unwrap_or_default())
                                    .collect(),
                            }),
                        );
                    }
                }
                let msg =
                    serde_json::json!({ "tipo": "destinos", "destinos": reports }).to_string();
                match tokio::time::timeout(
                    REPORT_SEND_TIMEOUT,
                    sink.send(Message::Text(msg.into())),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    _ => break,
                }
                if emissao.all_gave_up() {
                    let motivos = reports
                        .iter()
                        .filter_map(|r| r.motivo.as_deref().map(|m| format!("{}: {m}", r.rotulo)))
                        .collect::<Vec<_>>()
                        .join("; ");
                    let corpo = serde_json::json!({
                        "erro": format!("nenhum destino ficou no ar — {motivos}")
                    });
                    let _ = sink.send(Message::Text(corpo.to_string().into())).await;
                    let _ = sink.send(Message::Close(None)).await;
                    let _ = dead_tx.send(true);
                    break;
                }
            }
        })
    };

    let mut bytes: u64 = 0;
    loop {
        tokio::select! {
            msg = stream.next() => match msg {
                Some(Ok(Message::Binary(dados))) => {
                    bytes += dados.len() as u64;
                    // Síncrono e sem esperas: nenhum destino segura a leitura.
                    if !state.directos.escrever(sala, &dados) {
                        break; // alguém parou a emissão pelo outro lado
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ping/pong/texto: não é media, ignora-se
                Some(Err(e)) => {
                    tracing::warn!(sala = %codigo, erro = %e, "socket do directo caiu");
                    break;
                }
            },
            _ = until_true(&mut dead_rx) => break,
        }
    }
    reporter.abort();

    // A sala continua (ponto 5 do portão do ADR): parar a emissão não toca na chamada.
    let finais = match state.directos.parar(sala).await {
        Some(r) => r,
        None => emissao.parar().await,
    };
    tracing::info!(sala = %codigo, bytes, destinos = ?finais, "directo terminado");
    let started_at = *lock(&published);
    if let Some(started_at) = started_at {
        announce_live(&state, sala, None);
        let agora = chrono::Utc::now();
        fire_stream_event(
            &state,
            user_id,
            "stream.ended",
            format!("Directo terminado: sala {codigo}"),
            serde_json::json!({
                "room_id": sala,
                "room_code": codigo,
                "started_at": started_at,
                "ended_at": agora,
                "duration_secs": (agora - started_at).num_seconds().max(0),
                "by_user_id": user_id,
                "destinations": webhook_destinations(&finais),
            }),
        );
    }
}

/// Os destinos no payload de um webhook: rótulo, plataforma, estado e motivo.
/// NUNCA o URL — um URL interno de RTMP é informação da infraestrutura do
/// cliente, e o webhook sai para terceiros (Slack, Teams…).
fn webhook_destinations(reports: &[DestinationReport]) -> serde_json::Value {
    serde_json::Value::Array(
        reports
            .iter()
            .map(|r| {
                serde_json::json!({
                    "label": r.rotulo,
                    "platform": r.platform,
                    "state": r.estado,
                    "reason": r.motivo,
                    "bytes_sent": r.bytes_enviados,
                    "saved_destination_id": r.id,
                })
            })
            .collect(),
    )
}

/// Dispara um evento de directo para as organizações de quem emite — a mesma
/// regra do `recording.ready` no recorder (uma sala não tem organização; quem
/// a usa, tem).
fn fire_stream_event(
    state: &Arc<AppState>,
    user_id: Uuid,
    name: &'static str,
    text: String,
    payload: serde_json::Value,
) {
    let state = state.clone();
    tokio::spawn(async move {
        for org_id in crate::org::orgs_of_user(&state, user_id).await {
            crate::webhooks::fire(
                state.clone(),
                org_id,
                crate::webhooks::Event {
                    name,
                    title: "Delonix Meet".into(),
                    text: text.clone(),
                    payload: payload.clone(),
                },
            );
        }
    });
}

/// O estado AO VIVO de uma sala, para a sinalização o difundir a quem lá está.
///
/// **Contrato com `frontend/b1-sala`** (que expõe a função no hub):
///
/// ```text
/// impl SignalingHub {
///     /// `Some` = a sala está no ar (chamado à primeira passagem a «no ar» e
///     /// sempre que muda o número de destinos no ar); `None` = saiu do ar.
///     /// Não espera, não falha: difunde aos participantes locais e guarda para
///     /// quem entrar depois (o `Joined` leva-o).
///     pub fn set_live(&self, room_id: Uuid, info: signaling::LiveInfo) -> Option<signaling::LiveInfo>;
/// }
/// ```
///
/// Chamada deste módulo, e de mais nenhum sítio.
#[derive(Debug, Clone, Serialize)]
pub struct LiveInfo {
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub by_user_id: Uuid,
    pub destinations_live: usize,
    pub destinations_total: usize,
    /// Plataformas dos destinos no ar (`youtube`, `rtmp`, …). Nunca URLs.
    pub platforms: Vec<String>,
}

fn announce_live(state: &AppState, room_id: Uuid, info: Option<LiveInfo>) {
    // O contrato da sinalização (`SignalingHub::set_live`) leva só rótulo e
    // estado por destino — nunca URL nem chave. Aqui o rótulo é a plataforma.
    let live = match info {
        Some(i) => crate::signaling::LiveInfo {
            on: true,
            destinations: i
                .platforms
                .into_iter()
                .map(|p| crate::signaling::LiveDestination {
                    label: p,
                    state: "live".into(),
                    kbps: None,
                })
                .collect(),
            since: Some(i.started_at.timestamp_millis()),
        },
        None => crate::signaling::LiveInfo::default(),
    };
    // Não há sala neste nó (o hub devolve `None`): nada a difundir aqui.
    let _ = state.hub.set_live(room_id, live);
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Os programas falsos, escritos UMA vez por processo e TODOS de uma vez,
    /// atrás de um `OnceLock`, antes de qualquer lançamento.
    ///
    /// Porquê assim: escrever um executável enquanto outro teste faz `fork`
    /// falha com `ExecutableFileBusy` em ~3 corridas em 20 (R77) — o filho a
    /// nascer herda, na janela entre `fork` e `exec`, o descritor de escrita
    /// aberto, e o Linux recusa executar um ficheiro aberto para escrita.
    struct Falsos {
        dir: std::path::PathBuf,
        sorvedouro: std::path::PathBuf,
        /// Comporta-se conforme o PREFIXO da chave (o último argumento é o
        /// alvo `url/chave`, como no ffmpeg a sério):
        ///
        /// - `recusa…`   — escreve o erro do ffmpeg com o URL (e a chave!) e sai 1;
        /// - `entupido…` — despeja 1 MB no stderr ANTES de ler o stdin;
        /// - `surdo…`    — nunca lê o stdin;
        /// - `parado…`   — na 1.ª vida não lê o stdin; na 2.ª lê para `<chave>.bin`;
        /// - outro       — reporta progresso e guarda o stdin em `<chave>.bin`.
        ffmpeg: std::path::PathBuf,
    }

    fn falsos() -> &'static Falsos {
        use std::io::Write;
        static F: std::sync::OnceLock<Falsos> = std::sync::OnceLock::new();
        F.get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("dlx-directo-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("criar a pasta dos falsos");
            let escrever = |nome: &str, corpo: &str| {
                let caminho = dir.join(nome);
                {
                    let mut f = std::fs::File::create(&caminho).expect("criar");
                    f.write_all(corpo.as_bytes()).expect("escrever");
                    f.flush().expect("descarregar");
                } // FECHA aqui — antes do bit de execução e de qualquer lançamento
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&caminho, std::fs::Permissions::from_mode(0o755))
                    .expect("tornar executável");
                caminho
            };
            let sorvedouro = escrever("sorvedouro.sh", "#!/bin/sh\nexec cat > /dev/null\n");
            let d = dir.display();
            let ffmpeg = escrever(
                "ffmpeg-falso.sh",
                &format!(
                    r#"#!/bin/sh
for a; do alvo=$a; done
chave=${{alvo##*/}}
D='{d}'
case "$chave" in
  recusa*)
    echo "[tcp @ 0x5555] Connection to tcp://127.0.0.1:1 failed: Connection refused" >&2
    echo "[out#0/flv @ 0x5556] Error opening output $alvo: Connection refused" >&2
    exit 1 ;;
  entupido*)
    head -c 1000000 /dev/zero | tr '\0' e >&2
    exec cat > "$D/$chave.bin" ;;
  surdo*)
    exec sleep 30 ;;
  mudo*)
    exec cat > /dev/null ;;
  parado*)
    if [ ! -e "$D/$chave.marca" ]; then : > "$D/$chave.marca"; exec sleep 30; fi
    printf 'total_size=1\nprogress=continue\n'
    exec cat > "$D/$chave.bin" ;;
  *)
    printf 'total_size=100\nprogress=continue\n'
    (sleep 0.2; printf 'total_size=5000\ndrop_frames=3\nprogress=continue\n') &
    exec cat > "$D/$chave.bin" ;;
esac
"#
                ),
            );
            Falsos {
                dir,
                sorvedouro,
                ffmpeg,
            }
        })
    }

    fn prog(p: &std::path::Path) -> &str {
        p.to_str().expect("caminho utf-8")
    }

    fn destino(rotulo: &str, chave: &str) -> Destino {
        Destino {
            url: "rtmp://a.rtmp.youtube.com/live2".into(),
            chave: Secret::new(chave.to_string()),
            rotulo: rotulo.into(),
            id: None,
            platform: "youtube".into(),
        }
    }

    /// Uma chave única por teste: os ficheiros `<chave>.bin` não se cruzam.
    fn chave(prefixo: &str) -> String {
        format!("{prefixo}-{}", Uuid::new_v4().simple())
    }

    fn rapida() -> RetryPolicy {
        RetryPolicy {
            initial_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(50),
            max_attempts: 2,
            stable_after: Duration::from_secs(60),
            queue_max_bytes: 8 * 1024 * 1024,
            stop_grace: Duration::from_millis(500),
            connect_timeout: Duration::from_secs(10),
        }
    }

    /// Espera até `cond` ser verdade, ou falha com o último relatório.
    async fn esperar(e: &Emissao, o_que: &str, cond: impl Fn(&[DestinationReport]) -> bool) {
        let limite = Instant::now() + Duration::from_secs(10);
        loop {
            let r = e.report();
            if cond(&r) {
                return;
            }
            assert!(
                Instant::now() < limite,
                "à espera de {o_que}; estado: {r:#?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Um início de Cluster de Matroska como o Chromium o escreve: tamanho
    /// desconhecido (0x01FFFFFFFFFFFFFF) e o Timestamp logo a seguir.
    fn cluster(ts: u8) -> Vec<u8> {
        vec![
            0x1F, 0x43, 0xB6, 0x75, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xE7, 0x81, ts,
        ]
    }

    // ---------------------------------------------------------------- recusas

    #[test]
    fn uma_sala_com_e2ee_e_recusada_com_razao() {
        let d = [destino("yt", "abc")];
        let r = pode_emitir(true, "video/h264", &d, 0, 4, 4).expect_err("tinha de recusar");
        assert_eq!(r, Recusa::E2ee);
        // A razão tem de explicar o porquê E dizer o que fazer.
        let texto = r.to_string();
        assert!(texto.contains("ponta-a-ponta"), "{texto}");
        assert!(texto.contains("Desliga o E2EE"), "{texto}");
    }

    #[test]
    fn o_e2ee_ganha_a_todas_as_outras_recusas() {
        // A ordem importa: se o codec também estiver errado, a razão dada tem
        // de ser a do E2EE — é a que muda a decisão de quem criou a sala.
        let d: [Destino; 0] = [];
        assert_eq!(
            pode_emitir(true, "video/vp8", &d, 99, 1, 4).unwrap_err(),
            Recusa::E2ee
        );
    }

    #[test]
    fn um_codec_que_nao_se_copia_e_recusado_em_vez_de_produzir_lixo() {
        let d = [destino("yt", "abc")];
        for mime in ["video/vp8", "video/vp9", "video/av1", ""] {
            let r = pode_emitir(false, mime, &d, 0, 4, 4).expect_err("{mime} tinha de recusar");
            assert!(matches!(r, Recusa::Codec { .. }), "{mime}: {r:?}");
        }
    }

    #[test]
    fn o_h264_passa_com_os_dois_nomes_que_os_browsers_usam() {
        let d = [destino("yt", "abc")];
        for mime in ["video/H264", "video/h264", "video/avc"] {
            assert!(pode_emitir(false, mime, &d, 0, 4, 4).is_ok(), "{mime}");
        }
    }

    #[test]
    fn uma_chave_em_branco_conta_como_sem_destino() {
        // Um campo deixado vazio na interface não pode dar um comando com um
        // URL a acabar em barra — o ffmpeg tentaria e falharia com uma
        // mensagem que ninguém liga à causa.
        for chave in ["", "   "] {
            let d = [destino("yt", chave)];
            assert_eq!(
                pode_emitir(false, "video/h264", &d, 0, 4, 4).unwrap_err(),
                Recusa::SemDestino
            );
        }
    }

    #[test]
    fn um_url_que_nao_e_rtmp_e_recusado_antes_do_ffmpeg() {
        // O ffmpeg aceita um caminho como saída: sem esta regra, o URL de um
        // destino escrevia ficheiros no disco do servidor.
        for url in [
            "/tmp/roubado.flv",
            "file:///etc/cron.d/x",
            "http://exemplo/live",
            "rtmp://u:p@exemplo/live",
            "rtmp://exemplo/li ve",
        ] {
            let mut d = destino("mau", "k");
            d.url = url.into();
            let r = pode_emitir(false, "video/h264", &[d], 0, 4, 4).unwrap_err();
            assert_eq!(
                r,
                Recusa::DestinoInvalido {
                    rotulo: "mau".into()
                },
                "{url}"
            );
            assert!(r.to_string().contains("rtmp://"), "{r}");
        }
        let d = destino("chave com espaço", "a b");
        assert!(matches!(
            pode_emitir(false, "video/h264", &[d], 0, 4, 4),
            Err(Recusa::DestinoInvalido { .. })
        ));
        let mut d = destino("rtmps", "k");
        d.url = "rtmps://live-api-s.facebook.com:443/rtmp/".into();
        assert!(pode_emitir(false, "video/h264", &[d], 0, 4, 4).is_ok());
    }

    #[test]
    fn a_falta_de_ffmpeg_diz_o_nome_da_causa() {
        // Quem recebe a queixa tem de saber que a correcção é INSTALAR o
        // ffmpeg, não voltar a tentar. Mesma forma que o recorder.
        let texto = Recusa::SemFfmpeg.to_string();
        assert!(texto.contains("ffmpeg"), "{texto}");
        assert!(texto.contains("administra"), "{texto}");
    }

    #[test]
    fn o_tecto_de_emissoes_e_imposto() {
        let d = [destino("yt", "abc")];
        assert!(pode_emitir(false, "video/h264", &d, 1, 2, 4).is_ok());
        assert_eq!(
            pode_emitir(false, "video/h264", &d, 2, 2, 4).unwrap_err(),
            Recusa::Tecto {
                activas: 2,
                maximo: 2
            }
        );
    }

    #[test]
    fn varios_destinos_dentro_do_tecto_passam_juntos() {
        let d = [
            destino("yt", "k1"),
            destino("tw", "k2"),
            destino("fb", "k3"),
        ];
        assert!(pode_emitir(false, "video/h264", &d, 0, 4, 4).is_ok());
    }

    #[test]
    fn destinos_a_mais_para_o_tecto_do_no_sao_recusados_com_razao() {
        let d = [
            destino("yt", "k1"),
            destino("tw", "k2"),
            destino("fb", "k3"),
        ];
        let r = pode_emitir(false, "video/h264", &d, 0, 4, 2).unwrap_err();
        assert_eq!(
            r,
            Recusa::DemasiadosDestinos {
                pedidos: 3,
                maximo: 2
            }
        );
        let texto = r.to_string();
        assert!(texto.contains('3') && texto.contains('2'), "{texto}");
    }

    #[test]
    fn a_plataforma_sai_do_host_e_nunca_do_caminho() {
        assert_eq!(
            platform_from_url("rtmp://a.rtmp.youtube.com/live2"),
            "youtube"
        );
        assert_eq!(
            platform_from_url("rtmps://live-api-s.facebook.com:443/rtmp"),
            "facebook"
        );
        assert_eq!(platform_from_url("rtmp://127.0.0.1/youtube.com"), "rtmp");
        assert_eq!(platform_from_url("rtmp://notyoutube.com/x"), "rtmp");
    }

    // ------------------------------------------------------------- argumentos

    #[test]
    fn o_video_e_copiado_e_o_audio_transcodificado() {
        // É a decisão inteira do ADR: sem `-c:v copy` o pod codifica H.264 em
        // software e satura o core que serve a chamada.
        let a = montar_argumentos(&destino("yt", "k"), 2);
        let i = a.iter().position(|x| x == "-c:v").expect("sem -c:v");
        assert_eq!(a[i + 1], "copy");
        let j = a.iter().position(|x| x == "-c:a").expect("sem -c:a");
        assert_eq!(a[j + 1], "aac");
    }

    #[test]
    fn a_chave_entra_no_alvo_sem_barra_a_dobrar() {
        let mut d = destino("x", "k-123");
        d.url = "rtmp://exemplo/live/".into();
        let a = montar_argumentos(&d, 2);
        assert_eq!(a.last().unwrap(), "rtmp://exemplo/live/k-123");
    }

    #[test]
    fn cada_destino_tem_o_seu_processo_com_uma_so_saida() {
        // Um processo por destino: um destino mau não termina os outros.
        let a = montar_argumentos(&destino("yt", "k1"), 2);
        assert_eq!(a.iter().filter(|x| *x == "flv").count(), 1);
        assert!(!a.iter().any(|x| x == "tee"));
    }

    #[test]
    fn o_progresso_vai_para_o_stdout_e_o_stderr_fica_para_erros() {
        let a = montar_argumentos(&destino("yt", "k"), 2);
        let i = a
            .iter()
            .position(|x| x == "-progress")
            .expect("sem -progress");
        assert_eq!(a[i + 1], "pipe:1");
        assert!(a.iter().any(|x| x == "-nostats"));
        let j = a
            .iter()
            .position(|x| x == "-rw_timeout")
            .expect("sem -rw_timeout");
        assert!(a[j + 1].parse::<u64>().unwrap() > 0);
        // As opções de saída vêm ANTES do alvo, senão o ffmpeg ignora-as.
        assert!(j < a.len() - 1);
    }

    #[test]
    fn o_formato_de_entrada_vai_declarado() {
        // Um cano não se pode procurar para trás: sem `-f` o ffmpeg não
        // adivinha o formato e falha a arrancar. E tem de ser `matroska`
        // (medido com ffprobe: `format_name=matroska,webm`).
        let a = montar_argumentos(&destino("yt", "k"), 2);
        let i = a.iter().position(|x| x == "-i").expect("sem -i");
        assert_eq!(a[i + 1], "pipe:0");
        assert!(a[..i]
            .windows(2)
            .any(|w| w[0] == "-f" && w[1] == "matroska"));
    }

    #[test]
    fn o_travao_de_cpu_vai_no_comando() {
        let a = montar_argumentos(&destino("yt", "k"), 3);
        let i = a
            .iter()
            .position(|x| x == "-threads")
            .expect("sem -threads");
        assert_eq!(a[i + 1], "3");
    }

    // --------------------------------------------------------------- Matroska

    #[test]
    fn um_cluster_e_encontrado_com_o_tamanho_e_o_timestamp() {
        let mut buf = vec![0xAA; 50];
        buf.extend(cluster(0));
        assert_eq!(cluster_start(&buf), Some(50));
        // Tamanho de 1 byte (0x81) também é EBML válido.
        let curto = [0x00, 0x1F, 0x43, 0xB6, 0x75, 0x81, 0xE7, 0x81, 0x00];
        assert_eq!(cluster_start(&curto), Some(1));
    }

    #[test]
    fn os_quatro_bytes_do_id_dentro_do_video_nao_sao_um_cluster() {
        // Sem o Timestamp logo a seguir, é coincidência nos dados.
        let falso = [0x1F, 0x43, 0xB6, 0x75, 0x81, 0x00, 0x00, 0x00];
        assert_eq!(cluster_start(&falso), None);
        let zero = [0x1F, 0x43, 0xB6, 0x75, 0x00, 0xE7];
        assert_eq!(cluster_start(&zero), None);
        // Um falso seguido de um verdadeiro: é o verdadeiro que conta.
        let mut dois = falso.to_vec();
        dois.extend(cluster(1));
        assert_eq!(cluster_start(&dois), Some(falso.len()));
        // Cortado a meio: ainda não se sabe — e não se inventa.
        assert_eq!(cluster_start(&cluster(0)[..8]), None);
    }

    #[test]
    fn um_cluster_do_ffmpeg_com_crc32_primeiro_tambem_conta() {
        // O muxer do ffmpeg escreve CRC-32 antes do Timestamp (medido contra
        // um RTMP a sério: sem isto nenhum reinício reentrava).
        let c = [
            0x1F, 0x43, 0xB6, 0x75, 0x10, 0x00, 0x40, 0x00, 0xBF, 0x84, 0x12, 0x34, 0x56, 0x78,
            0xE7, 0x81, 0x00,
        ];
        let mut buf = vec![0x00; 7];
        buf.extend_from_slice(&c);
        assert_eq!(cluster_start(&buf), Some(7));
        // Um CRC sem o Timestamp a seguir não chega.
        let mut sem = c;
        sem[14] = 0x00;
        assert_eq!(cluster_start(&sem), None);
    }

    // ------------------------------------------------------- erros e segredo

    #[test]
    fn o_motivo_e_uma_frase_que_se_age_e_nunca_leva_a_chave() {
        let linhas: VecDeque<String> = [
            "[tcp @ 0x1] Connection to tcp://x:1935 failed: Connection refused",
            "Error opening output rtmp://x/live2/[chave]: Connection refused",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let m = classify_failure(&linhas, None);
        assert!(m.contains("recusou a ligação"), "{m}");
        let chave: VecDeque<String> =
            ["RTMP_ReadPacket, server sent: NetStream.Publish.BadName".into()].into();
        assert!(classify_failure(&chave, None).contains("chave"));
        let nada = classify_failure(&VecDeque::new(), None);
        assert!(nada.contains("sem explicação"), "{nada}");
        assert_eq!(
            redact(
                "Error opening output rtmp://x/live2/SEGREDO-1: I/O",
                "SEGREDO-1"
            ),
            "Error opening output rtmp://x/live2/[chave]: I/O"
        );
    }

    #[test]
    fn o_backoff_cresce_e_tem_tecto() {
        let p = RetryPolicy::default();
        assert_eq!(p.backoff(1), Duration::from_secs(1));
        assert_eq!(p.backoff(2), Duration::from_secs(2));
        assert_eq!(p.backoff(4), Duration::from_secs(8));
        assert_eq!(p.backoff(10), Duration::from_secs(30));
        assert_eq!(p.backoff(1000), Duration::from_secs(30));
    }

    #[test]
    fn o_estado_serializa_com_os_nomes_que_o_cliente_le() {
        let r = DestinationReport {
            dest: 1,
            id: None,
            rotulo: "YouTube".into(),
            platform: "youtube".into(),
            estado: DestinationState::Live,
            kbps: 4500,
            perdas: 0.2,
            motivo: None,
            tentativas: 0,
            frames_descartados: 0,
            bytes_enviados: 10,
        };
        let v = serde_json::to_value(&r).unwrap();
        for (k, esperado) in [
            ("dest", serde_json::json!(1)),
            ("estado", serde_json::json!("no-ar")),
            ("kbps", serde_json::json!(4500)),
            ("perdas", serde_json::json!(0.2)),
            ("motivo", serde_json::Value::Null),
        ] {
            assert_eq!(v[k], esperado, "{k}");
        }
        assert!(v.get("id").is_none(), "um destino não guardado não leva id");
        for (e, s) in [
            (DestinationState::Connecting, "a-ligar"),
            (DestinationState::Stopped, "parado"),
            (DestinationState::Error, "erro"),
        ] {
            assert_eq!(serde_json::to_value(e).unwrap(), serde_json::json!(s));
        }
    }

    #[test]
    fn a_chave_nunca_aparece_no_debug_do_destino() {
        // R43: material de chave não vive num tipo que derive `Debug`.
        let d = destino("yt", "chave-super-secreta");
        let texto = format!("{d:?}");
        assert!(!texto.contains("chave-super-secreta"), "{texto}");
        assert!(texto.contains("[segredo redigido]"), "{texto}");
        assert!(texto.contains("yt"), "{texto}");
    }

    #[tokio::test]
    async fn a_chave_nao_aparece_no_debug_da_emissao() {
        let e = Emissao::arrancar(
            &[destino("yt", "chave-secreta")],
            1,
            prog(&falsos().sorvedouro),
            RetryPolicy::default(),
        )
        .expect("devia arrancar");
        let texto = format!("{e:?}");
        assert!(!texto.contains("chave-secreta"), "{texto}");
        e.parar().await;
    }

    // ------------------------------------------------------- ciclo de vida

    #[tokio::test]
    async fn a_emissao_aceita_media_e_termina_ao_parar() {
        let e = Emissao::arrancar(
            &[destino("yt", "k")],
            1,
            prog(&falsos().sorvedouro),
            RetryPolicy::default(),
        )
        .expect("arrancou");
        e.escrever(&Bytes::from_static(b"media"));
        let finais = e.parar().await;
        assert_eq!(finais[0].estado, DestinationState::Stopped);
        assert_eq!(finais[0].motivo, None, "parar pela pessoa não é um erro");
        // Idempotente: parar outra vez não pendura nem atira.
        e.parar().await;
    }

    #[tokio::test]
    async fn um_programa_que_nao_existe_falha_a_arrancar_em_vez_de_ficar_meio_vivo() {
        let r = Emissao::arrancar(
            &[destino("yt", "k")],
            1,
            "delonix-ffmpeg-que-nao-existe",
            RetryPolicy::default(),
        );
        assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    }

    /// PROBLEMA 1 da auditoria de 2026-09-16, reproduzido contra o código
    /// antigo (falhava: «a sala B ficou bloqueada pela emissão parada da sala
    /// A») e verde contra este. Um ffmpeg que não lê o stdin não pode segurar
    /// quem escreve, nem a emissão de outra sala do mesmo nó.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn uma_emissao_parada_nao_bloqueia_outra_sala() {
        let f = falsos();
        let registo = Registo::default();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let ea = Emissao::arrancar(
            &[destino("a", &chave("surdo"))],
            1,
            prog(&f.ffmpeg),
            rapida(),
        )
        .unwrap();
        let chave_b = chave("ok");
        let eb =
            Emissao::arrancar(&[destino("b", &chave_b)], 1, prog(&f.ffmpeg), rapida()).unwrap();
        assert!(registo.inserir(a, ea.clone()));
        assert!(registo.inserir(b, eb.clone()));

        let t = Instant::now();
        let pedaco = Bytes::from(vec![0u8; 64 * 1024]);
        for _ in 0..32 {
            // 2 MiB para quem não lê: o cano do SO enche aos 64 KiB.
            assert!(registo.escrever(a, &pedaco));
        }
        assert!(registo.escrever(b, &Bytes::from_static(b"media-da-sala-b")));
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "escrever demorou {:?}: alguém esperou pelo ffmpeg parado",
            t.elapsed()
        );
        esperar(&eb, "a sala B no ar", |r| {
            r[0].estado == DestinationState::Live
        })
        .await;
        registo.parar(b).await.expect("existia");
        let recebido = std::fs::read(f.dir.join(format!("{chave_b}.bin"))).unwrap();
        assert_eq!(
            recebido, b"media-da-sala-b",
            "a media da sala B chegou inteira"
        );
        registo.parar(a).await.expect("existia");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn o_stderr_e_drenado_e_a_media_chega_inteira() {
        // Um ffmpeg que despeja 1 MB no stderr antes de ler o stdin: sem
        // drenagem o cano do stderr enche aos 64 KiB, o processo pára, e o
        // stdin nunca mais anda.
        let f = falsos();
        let k = chave("entupido");
        let e = Emissao::arrancar(
            &[destino("yt", &k)],
            1,
            prog(&f.ffmpeg),
            RetryPolicy::default(),
        )
        .unwrap();
        let pedaco = Bytes::from(vec![7u8; 256 * 1024]);
        for _ in 0..8 {
            e.escrever(&pedaco);
        }
        let finais = tokio::time::timeout(Duration::from_secs(10), e.parar())
            .await
            .expect("parar pendurou: o stderr não foi drenado");
        assert_eq!(finais[0].perdas, 0.0, "{finais:?}");
        let n = std::fs::metadata(f.dir.join(format!("{k}.bin")))
            .unwrap()
            .len();
        assert_eq!(n, 8 * 256 * 1024);
    }

    /// PROBLEMA 2: um destino mau não pára os outros, reinicia com backoff
    /// limitado, e desiste com um motivo que não leva a chave.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn um_destino_que_cai_nao_derruba_os_outros() {
        let f = falsos();
        let (mau, bom) = (chave("recusa"), chave("ok"));
        let e = Emissao::arrancar(
            &[destino("Mau", &mau), destino("Bom", &bom)],
            1,
            prog(&f.ffmpeg),
            rapida(),
        )
        .unwrap();
        e.escrever(&Bytes::from_static(b"antes"));
        esperar(&e, "o mau a desistir e o bom no ar", |r| {
            r[0].estado == DestinationState::Stopped && r[1].estado == DestinationState::Live
        })
        .await;
        e.escrever(&Bytes::from_static(b"-depois"));

        let r = e.report();
        let motivo = r[0].motivo.clone().expect("desistir tem motivo");
        assert!(motivo.contains("recusou a ligação"), "{motivo}");
        assert!(motivo.contains("2 tentativas"), "{motivo}");
        assert!(!motivo.contains(&mau), "a chave vazou no motivo: {motivo}");
        assert_eq!(
            r[0].tentativas, 3,
            "duas tentativas depois da primeira queda"
        );
        assert!(!e.all_gave_up(), "o bom continua no ar");

        esperar(&e, "o débito do bom", |r| {
            r[1].kbps > 0 && r[1].frames_descartados == 3
        })
        .await;
        let finais = e.parar().await;
        assert_eq!(finais[1].motivo, None);
        let recebido = std::fs::read(f.dir.join(format!("{bom}.bin"))).unwrap();
        assert_eq!(recebido, b"antes-depois", "o destino bom recebeu tudo");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn um_destino_que_nunca_chega_ao_ar_conta_como_queda() {
        // Lê a media mas nunca sai nada para o destino: sem a vigia de
        // arranque ficava «a ligar» para sempre, sem tentativas nem motivo.
        let politica = RetryPolicy {
            connect_timeout: Duration::from_millis(300),
            initial_backoff: Duration::from_millis(300),
            ..rapida()
        };
        let e = Emissao::arrancar(
            &[destino("Mudo", &chave("mudo"))],
            1,
            prog(&falsos().ffmpeg),
            politica,
        )
        .unwrap();
        esperar(&e, "a vigia a dar pela falta", |r| {
            r[0].estado == DestinationState::Error
                && r[0]
                    .motivo
                    .as_deref()
                    .is_some_and(|m| m.contains("não ficou no ar"))
        })
        .await;
        e.parar().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn quando_todos_desistem_a_emissao_sabe() {
        let e = Emissao::arrancar(
            &[
                destino("A", &chave("recusa")),
                destino("B", &chave("recusa")),
            ],
            1,
            prog(&falsos().ffmpeg),
            rapida(),
        )
        .unwrap();
        esperar(&e, "os dois a desistir", |r| {
            r.iter().all(|d| d.estado == DestinationState::Stopped)
        })
        .await;
        assert!(e.all_gave_up());
        assert!(!e.ever_live());
        e.parar().await;
        assert!(
            !e.all_gave_up(),
            "depois de parar pela pessoa já não é «desistiram»"
        );
    }

    /// Um destino que deixa de ler é morto e reiniciado, e o processo novo
    /// recebe o cabeçalho guardado seguido do fluxo a partir do próximo
    /// Cluster — mesmo com o id do Cluster partido entre dois pedaços.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn um_destino_que_nao_acompanha_reinicia_e_reentra_num_cluster() {
        let f = falsos();
        let k = chave("parado");
        let politica = RetryPolicy {
            queue_max_bytes: 256 * 1024,
            // Longo o bastante para o estado «erro» se ver entre a queda e o
            // reinício.
            initial_backoff: Duration::from_millis(400),
            ..rapida()
        };
        let e = Emissao::arrancar(&[destino("Lento", &k)], 1, prog(&f.ffmpeg), politica).unwrap();

        let cabecalho = vec![0xAAu8; 100];
        let mut inicio = cabecalho.clone();
        inicio.extend(cluster(0));
        inicio.extend(vec![0xBB; 1000]);
        // O falso só está «parado» depois de marcar a 1.ª vida; matá-lo antes
        // disso faria a 2.ª vida parar em vez de ler.
        let marca = f.dir.join(format!("{k}.marca"));
        let limite = Instant::now() + Duration::from_secs(10);
        while !marca.exists() {
            assert!(Instant::now() < limite, "o falso não arrancou");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        e.escrever(&Bytes::from(inicio));
        // Enche a fila de quem não lê até transbordar.
        let meio = Bytes::from(vec![0xCC; 32 * 1024]);
        for _ in 0..64 {
            e.escrever(&meio);
        }
        esperar(&e, "a queda por fila cheia", |r| {
            r[0].estado == DestinationState::Error
                && r[0]
                    .motivo
                    .as_deref()
                    .is_some_and(|m| m.contains("não acompanhou"))
        })
        .await;
        esperar(&e, "o reinício no ar", |r| {
            r[0].estado == DestinationState::Live
        })
        .await;

        // Nada de Cluster aqui: o processo novo não pode receber isto.
        e.escrever(&Bytes::from(vec![0xDD; 500]));
        // O id do Cluster partido entre dois pedaços.
        let c = cluster(9);
        let mut a = vec![0xDDu8; 10];
        a.extend_from_slice(&c[..2]);
        let mut b = c[2..].to_vec();
        b.extend(vec![0xEE; 100]);
        e.escrever(&Bytes::from(a));
        e.escrever(&Bytes::from(b));
        let finais = e.parar().await;

        let recebido = std::fs::read(f.dir.join(format!("{k}.bin"))).unwrap();
        let mut esperado = cabecalho;
        esperado.extend(cluster(9));
        esperado.extend(vec![0xEE; 100]);
        assert_eq!(recebido.len(), esperado.len());
        assert!(recebido == esperado, "o reinício não entrou no Cluster");
        assert!(finais[0].perdas > 0.0, "o que se perdeu conta: {finais:?}");
    }
}

// ---------------------------------------------------------------------------
//  Rota: estado vivo da emissão (G1)
// ---------------------------------------------------------------------------

/// `GET /api/rooms/{room_code}/live/status` — estado vivo da emissão desta
/// sala: rótulos dos destinos, bytes enviados, débito binário recente.
/// `404` se a sala não existe ou não está em directo agora.
///
/// Sem controlo de acesso além da sessão: o código da sala já é a
/// credencial (a mesma nota de `get_room`, em `rooms.rs`) — quem o conhece
/// vê os metadados, não só quem entra na chamada.
#[utoipa::path(
    get, path = "/api/rooms/{room_code}/live/status", tag = "rooms",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala (`abc-defg-hij`).")),
    responses(
        (status = 200, body = EmissaoEstado),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A sala não existe, ou não está em directo agora.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn estado_directo(
    State(state): State<Arc<AppState>>,
    auth: crate::auth::AuthUser,
    Path(codigo): Path<String>,
) -> Result<axum::Json<EmissaoEstado>, ApiError> {
    let _ = auth;
    let id: Uuid = sqlx::query_scalar("SELECT id FROM rooms WHERE code = $1")
        .bind(codigo.to_lowercase())
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    state
        .directos
        .estado(id)
        .map(axum::Json)
        .ok_or(ApiError::NotFound)
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(estado_directo), components(schemas(EmissaoEstado)))]
pub struct ApiDoc;
