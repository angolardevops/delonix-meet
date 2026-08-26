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
//! onde vem a media (hoje o SFU, amanhã outra coisa) e torna-o testável com um
//! processo qualquer que leia de um cano.

use std::fmt;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;

use crate::signaling::Secret;

/// Um destino de emissão: para onde vai, e com que chave.
pub struct Destino {
    /// Ex.: `rtmp://a.rtmp.youtube.com/live2`. SEM a chave.
    pub url: String,
    pub chave: Secret,
    /// Nome só para a interface e para os logs — nunca influencia o comando.
    pub rotulo: String,
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
            .finish()
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

/// Decide se uma emissão pode arrancar. Sem efeitos — é a função que os testes
/// atacam, e é a que se lê para saber as regras.
pub fn pode_emitir(
    e2ee_ligado: bool,
    mime_video: &str,
    destinos: &[Destino],
    activas: usize,
    maximo: usize,
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
    if activas >= maximo {
        return Err(Recusa::Tecto { activas, maximo });
    }
    Ok(())
}

/// Junta o URL do destino à chave, sem barras a dobrar.
fn alvo(destino: &Destino) -> String {
    format!(
        "{}/{}",
        destino.url.trim_end_matches('/'),
        destino.chave.expose().trim()
    )
}

/// Monta os argumentos do ffmpeg.
///
/// Existe separado, e devolve `Vec<String>` em vez de um `Command`, para ser
/// testável sem um `ffmpeg` instalado — é o mesmo padrão do `recorder.rs`, e é
/// o que permite verificar que o vídeo é COPIADO (a decisão inteira do ADR
/// assenta nisso) sem precisar de media.
pub fn montar_argumentos(destinos: &[Destino], threads: u32) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
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
    ];
    for d in destinos {
        a.push("-f".into());
        a.push("flv".into());
        a.push(alvo(d));
    }
    a
}

/// Uma emissão a decorrer.
pub struct Emissao {
    filho: Child,
    entrada: Arc<Mutex<Option<ChildStdin>>>,
    pub rotulos: Vec<String>,
}

impl fmt::Debug for Emissao {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emissao")
            .field("rotulos", &self.rotulos)
            .field("pid", &self.filho.id())
            .finish()
    }
}

impl Emissao {
    /// Arranca o ffmpeg. Não valida nada — quem chama passa pelo `pode_emitir`
    /// primeiro, e os testes atacam as duas coisas em separado.
    pub fn arrancar(destinos: &[Destino], threads: u32, programa: &str) -> std::io::Result<Self> {
        let mut cmd = Command::new(programa);
        cmd.args(montar_argumentos(destinos, threads));
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());
        // Se este objecto for largado, o processo morre com ele. Um ffmpeg
        // órfão a empurrar para o YouTube é pior do que uma emissão que cai:
        // consome o core e ninguém sabe que existe.
        cmd.kill_on_drop(true);
        let mut filho = cmd.spawn()?;
        let entrada = filho.stdin.take();
        Ok(Self {
            filho,
            entrada: Arc::new(Mutex::new(entrada)),
            rotulos: destinos.iter().map(|d| d.rotulo.clone()).collect(),
        })
    }

    /// Empurra media. Um erro aqui significa que o ffmpeg morreu — e isso NÃO
    /// pode derrubar a chamada: quem chama regista e pára a emissão, a sala
    /// continua (ponto 5 do portão do ADR).
    pub async fn escrever(&self, dados: &[u8]) -> std::io::Result<()> {
        let mut guarda = self.entrada.lock().await;
        match guarda.as_mut() {
            Some(stdin) => stdin.write_all(dados).await,
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "a emissão já foi fechada",
            )),
        }
    }

    /// Fecha o cano e espera pelo ffmpeg. Fechar o stdin é o que faz o ffmpeg
    /// terminar o ficheiro em condições em vez de ser morto a meio.
    pub async fn parar(mut self) -> std::io::Result<std::process::ExitStatus> {
        {
            let mut guarda = self.entrada.lock().await;
            if let Some(mut stdin) = guarda.take() {
                let _ = stdin.shutdown().await;
            }
        }
        self.filho.wait().await
    }

    pub fn viva(&mut self) -> bool {
        matches!(self.filho.try_wait(), Ok(None))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Programa de teste que ignora os argumentos e drena o stdin — que é o que
    /// o ffmpeg faz com a media. O `cat` não serve: recebe `-hide_banner` e os
    /// restantes como NOMES DE FICHEIRO, não os encontra, e sai com erro.
    /// Um executável que consome o stdin, como o ffmpeg faz.
    ///
    /// Escrito UMA vez por processo, atrás de um `OnceLock`, e nunca enquanto
    /// há lançamentos a decorrer. A versão anterior escrevia-o à chamada e
    /// falhava com `ExecutableFileBusy` em ~3 corridas em 20, com o teste
    /// afectado a mudar de cada vez.
    ///
    /// A causa não é o ficheiro ser partilhado — dar um ficheiro único a cada
    /// chamada NÃO resolveu. É a corrida entre `fork` e `exec`: um filho a
    /// nascer para outro teste herda, na janela entre os dois, o descritor de
    /// escrita que este thread tem aberto, e o Linux recusa executar um
    /// ficheiro que alguém tenha aberto para escrita.
    ///
    /// O `OnceLock` fecha essa janela: quem chegar depois espera pela escrita
    /// terminada em vez de correr contra ela, e a partir daí ninguém volta a
    /// abrir o ficheiro para escrever. (Usar o `cat` do sistema em vez do
    /// script parece mais simples e não serve: sem a redirecção que o script
    /// faz, o processo sai com estado diferente de zero e o teste do ciclo de
    /// vida deixa de valer.)
    fn sorvedouro() -> std::path::PathBuf {
        use std::io::Write;
        static CAMINHO: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
        CAMINHO
            .get_or_init(|| {
                let caminho =
                    std::env::temp_dir().join(format!("dlx-sorvedouro-{}.sh", std::process::id()));
                {
                    let mut f = std::fs::File::create(&caminho).expect("criar o sorvedouro");
                    f.write_all(b"#!/bin/sh\nexec cat > /dev/null\n")
                        .expect("escrever");
                    f.flush().expect("descarregar");
                } // FECHA aqui — antes do bit de execução e de qualquer lançamento
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&caminho, std::fs::Permissions::from_mode(0o755))
                        .expect("tornar executável");
                }
                caminho
            })
            .clone()
    }

    fn destino(rotulo: &str, chave: &str) -> Destino {
        Destino {
            url: "rtmp://a.rtmp.youtube.com/live2".into(),
            chave: Secret::new(chave.to_string()),
            rotulo: rotulo.into(),
        }
    }

    // ---------------------------------------------------------------- recusas

    #[test]
    fn uma_sala_com_e2ee_e_recusada_com_razao() {
        let d = [destino("yt", "abc")];
        let r = pode_emitir(true, "video/h264", &d, 0, 4).expect_err("tinha de recusar");
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
            pode_emitir(true, "video/vp8", &d, 99, 1).unwrap_err(),
            Recusa::E2ee
        );
    }

    #[test]
    fn um_codec_que_nao_se_copia_e_recusado_em_vez_de_produzir_lixo() {
        let d = [destino("yt", "abc")];
        for mime in ["video/vp8", "video/vp9", "video/av1", ""] {
            let r = pode_emitir(false, mime, &d, 0, 4).expect_err("{mime} tinha de recusar");
            assert!(matches!(r, Recusa::Codec { .. }), "{mime}: {r:?}");
        }
    }

    #[test]
    fn o_h264_passa_com_os_dois_nomes_que_os_browsers_usam() {
        let d = [destino("yt", "abc")];
        for mime in ["video/H264", "video/h264", "video/avc"] {
            assert!(pode_emitir(false, mime, &d, 0, 4).is_ok(), "{mime}");
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
                pode_emitir(false, "video/h264", &d, 0, 4).unwrap_err(),
                Recusa::SemDestino
            );
        }
    }

    #[test]
    fn o_tecto_de_emissoes_e_imposto() {
        let d = [destino("yt", "abc")];
        assert!(pode_emitir(false, "video/h264", &d, 1, 2).is_ok());
        assert_eq!(
            pode_emitir(false, "video/h264", &d, 2, 2).unwrap_err(),
            Recusa::Tecto {
                activas: 2,
                maximo: 2
            }
        );
    }

    // ------------------------------------------------------------- argumentos

    #[test]
    fn o_video_e_copiado_e_o_audio_transcodificado() {
        // É a decisão inteira do ADR: sem `-c:v copy` o pod codifica H.264 em
        // software e satura o core que serve a chamada.
        let a = montar_argumentos(&[destino("yt", "k")], 2);
        let i = a.iter().position(|x| x == "-c:v").expect("sem -c:v");
        assert_eq!(a[i + 1], "copy");
        let j = a.iter().position(|x| x == "-c:a").expect("sem -c:a");
        assert_eq!(a[j + 1], "aac");
    }

    #[test]
    fn a_chave_entra_no_alvo_sem_barra_a_dobrar() {
        let d = Destino {
            url: "rtmp://exemplo/live/".into(),
            chave: Secret::new("k-123".into()),
            rotulo: "x".into(),
        };
        let a = montar_argumentos(&[d], 2);
        assert!(
            a.iter().any(|x| x == "rtmp://exemplo/live/k-123"),
            "alvo mal formado: {a:?}"
        );
    }

    #[test]
    fn cada_destino_ganha_a_sua_saida_flv() {
        let a = montar_argumentos(&[destino("yt", "k1"), destino("tw", "k2")], 2);
        assert_eq!(a.iter().filter(|x| *x == "flv").count(), 2);
    }

    #[test]
    fn o_formato_de_entrada_vai_declarado() {
        // Um cano não se pode procurar para trás: sem `-f` o ffmpeg não
        // adivinha o formato e falha a arrancar.
        //
        // E tem de ser `matroska`, não `webm`: o que o browser produz é
        // Matroska com H.264 (medido com ffprobe: `format_name=matroska,webm`).
        // `-f webm` funciona hoje por o desmultiplexador ser o mesmo, mas isso
        // é sorte e não contrato.
        let a = montar_argumentos(&[destino("yt", "k")], 2);
        let i = a.iter().position(|x| x == "-i").expect("sem -i");
        assert_eq!(a[i + 1], "pipe:0");
        assert!(a[..i]
            .windows(2)
            .any(|w| w[0] == "-f" && w[1] == "matroska"));
    }

    #[test]
    fn o_travao_de_cpu_vai_no_comando() {
        let a = montar_argumentos(&[destino("yt", "k")], 3);
        let i = a
            .iter()
            .position(|x| x == "-threads")
            .expect("sem -threads");
        assert_eq!(a[i + 1], "3");
    }

    // ----------------------------------------------------------------- segredo

    #[test]
    fn a_chave_nunca_aparece_no_debug_do_destino() {
        // R43: material de chave não vive num tipo que derive `Debug`.
        let d = destino("yt", "chave-super-secreta");
        let texto = format!("{d:?}");
        assert!(!texto.contains("chave-super-secreta"), "{texto}");
        assert!(texto.contains("[segredo redigido]"), "{texto}");
        // O rótulo e o URL PODEM aparecer — são o que torna o log útil.
        assert!(texto.contains("yt"), "{texto}");
    }

    #[tokio::test]
    async fn a_chave_nao_aparece_no_debug_da_emissao() {
        // `#[tokio::test]`, não `#[test]`: o `Command` do tokio precisa de um
        // reactor a correr, e sem ele entra em pânico em vez de falhar claro.
        let prog = sorvedouro();
        let e = Emissao::arrancar(&[destino("yt", "chave-secreta")], 1, prog.to_str().unwrap())
            .expect("devia arrancar");
        let texto = format!("{e:?}");
        assert!(!texto.contains("chave-secreta"), "{texto}");
    }

    // ------------------------------------------------------- ciclo de vida
    // Usam `cat`, que lê do stdin como o ffmpeg — testável sem ffmpeg
    // instalado, o mesmo padrão do `recorder.rs`.

    #[tokio::test]
    async fn a_emissao_aceita_media_e_termina_ao_fechar_o_cano() {
        let prog = sorvedouro();
        let e =
            Emissao::arrancar(&[destino("yt", "k")], 1, prog.to_str().unwrap()).expect("arrancou");
        e.escrever(b"media").await.expect("devia aceitar");
        let st = e.parar().await.expect("devia terminar");
        assert!(st.success());
    }

    #[tokio::test]
    async fn escrever_depois_de_parar_devolve_erro_em_vez_de_pendurar() {
        let prog = sorvedouro();
        let e =
            Emissao::arrancar(&[destino("yt", "k")], 1, prog.to_str().unwrap()).expect("arrancou");
        {
            let mut g = e.entrada.lock().await;
            g.take();
        }
        let err = e.escrever(b"x").await.expect_err("tinha de falhar");
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[tokio::test]
    async fn um_programa_que_nao_existe_falha_a_arrancar_em_vez_de_ficar_meio_vivo() {
        let r = Emissao::arrancar(&[destino("yt", "k")], 1, "delonix-ffmpeg-que-nao-existe");
        assert!(r.is_err(), "tinha de falhar a arrancar");
    }
}

// ---------------------------------------------------------------------------
//  Registo das emissões vivas do nó
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use uuid::Uuid;

/// As emissões a decorrer neste pod, por sala.
///
/// Por sala e não por utilizador: uma sala emite uma vez. Dois anfitriões a
/// carregar em «ir para o ar» ao mesmo tempo dariam dois ffmpeg a empurrar para
/// a mesma chave, e a plataforma externa corta os dois.
#[derive(Default)]
pub struct Registo {
    activas: Mutex<HashMap<Uuid, Emissao>>,
}

impl Registo {
    pub async fn quantas(&self) -> usize {
        self.activas.lock().await.len()
    }

    pub async fn tem(&self, sala: Uuid) -> bool {
        self.activas.lock().await.contains_key(&sala)
    }

    /// Regista uma emissão. Devolve `false` se a sala já tinha uma — quem
    /// chama trata isso como recusa, não como sucesso silencioso.
    pub async fn inserir(&self, sala: Uuid, e: Emissao) -> bool {
        let mut m = self.activas.lock().await;
        if m.contains_key(&sala) {
            return false;
        }
        m.insert(sala, e);
        true
    }

    /// Empurra media para a emissão da sala. `Ok(false)` = não há emissão.
    pub async fn escrever(&self, sala: Uuid, dados: &[u8]) -> std::io::Result<bool> {
        let m = self.activas.lock().await;
        match m.get(&sala) {
            Some(e) => e.escrever(dados).await.map(|_| true),
            None => Ok(false),
        }
    }

    /// Tira a emissão do registo e fecha-a. Silencioso se não existir.
    pub async fn parar(&self, sala: Uuid) -> Option<std::io::Result<std::process::ExitStatus>> {
        let e = self.activas.lock().await.remove(&sala)?;
        Some(e.parar().await)
    }
}

// ---------------------------------------------------------------------------
//  Rota: o browser empurra a emissão já composta por WebSocket
// ---------------------------------------------------------------------------

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use serde::Deserialize;

use crate::error::ApiError;
use crate::AppState;

#[derive(Deserialize)]
pub struct DirectoQuery {
    /// Token de sala, o mesmo que o `/ws` usa — curto e com âmbito.
    pub token: String,
    /// URL base do destino (sem a chave).
    pub destino: String,
    /// Chave de emissão. Vem na query porque um WebSocket não tem corpo; é
    /// por isso que a rota EXIGE o token de sala e nunca regista a query.
    pub chave: String,
    #[serde(default)]
    pub rotulo: Option<String>,
    /// MIME do vídeo que o browser vai empurrar, para se poder recusar ANTES
    /// de arrancar o ffmpeg.
    pub codec: String,
}

/// `GET /api/rooms/{code}/broadcast` (upgrade para WebSocket).
///
/// O browser compõe, codifica em H.264 e empurra pedaços de WebM por aqui; o
/// servidor remultiplexa para RTMP. Ver o ADR-0003.
pub async fn ws_directo(
    State(state): State<Arc<AppState>>,
    Path(codigo): Path<String>,
    Query(q): Query<DirectoQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let claims = crate::auth::verify_jwt(&state.config.jwt_secret, &q.token, "room")?;
    let sala_id = claims.room.ok_or(ApiError::Unauthorized)?;

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

    let destinos = vec![Destino {
        url: q.destino.clone(),
        chave: Secret::new(q.chave.clone()),
        rotulo: q.rotulo.clone().unwrap_or_else(|| "directo".into()),
    }];

    // As regras ANTES de gastar um processo. A razão vai para o cliente tal
    // como está escrita — é ela que a interface mostra.
    let activas = state.directos.quantas().await;
    if let Err(recusa) = pode_emitir(
        e2ee,
        &q.codec,
        &destinos,
        activas,
        state.config.max_directos,
    ) {
        tracing::warn!(sala = %codigo, motivo = ?recusa, "directo recusado");
        return Err(ApiError::BadRequest(recusa.to_string()));
    }
    if state.directos.tem(sala_id).await {
        return Err(ApiError::Conflict("esta sala já está em directo".into()));
    }

    let emissao = Emissao::arrancar(&destinos, state.config.directo_threads, "ffmpeg")
        .map_err(|e| ApiError::Internal(format!("não foi possível arrancar a emissão: {e}")))?;
    if !state.directos.inserir(sala_id, emissao).await {
        return Err(ApiError::Conflict("esta sala já está em directo".into()));
    }
    tracing::info!(sala = %codigo, destinos = ?destinos, "directo a começar");

    Ok(ws.on_upgrade(move |socket| bombear(socket, state, sala_id, codigo)))
}

/// Empurra o que chega do browser para o ffmpeg, até um dos lados fechar.
async fn bombear(mut socket: WebSocket, state: Arc<AppState>, sala: Uuid, codigo: String) {
    let mut bytes: u64 = 0;
    while let Some(msg) = socket.recv().await {
        match msg {
            Ok(Message::Binary(dados)) => {
                bytes += dados.len() as u64;
                match state.directos.escrever(sala, &dados).await {
                    Ok(true) => {}
                    Ok(false) => break, // alguém parou a emissão pelo outro lado
                    Err(e) => {
                        // O ffmpeg morreu — o destino caiu, a chave é inválida,
                        // o que for. Isto NÃO derruba a chamada: fecha-se a
                        // emissão e a sala continua (ponto 5 do portão do ADR).
                        tracing::warn!(sala = %codigo, erro = %e, "a emissão parou de aceitar media");
                        break;
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {} // ping/pong/texto: não é media, ignora-se
            Err(e) => {
                tracing::warn!(sala = %codigo, erro = %e, "socket do directo caiu");
                break;
            }
        }
    }
    match state.directos.parar(sala).await {
        Some(Ok(st)) => tracing::info!(sala = %codigo, bytes, saida = ?st, "directo terminado"),
        Some(Err(e)) => tracing::warn!(sala = %codigo, bytes, erro = %e, "directo terminou mal"),
        None => tracing::info!(sala = %codigo, bytes, "directo já tinha sido parado"),
    }
}
