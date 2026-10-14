//! A cadeia da dobragem ao vivo: frase → tradução → voz → pista (ADR-0030).
//!
//! # O que já existia, e por isso não se refez
//!
//! **O ASR não é nosso.** Cada cliente transcreve o PRÓPRIO microfone e manda
//! a frase final em `ClientMsg::Transcript` — o servidor já a recebe, já a
//! passa pelo DLP e já a difunde. A dobragem entra DEPOIS dessa limpeza, e é
//! uma propriedade que vale a pena dizer em voz alta: **o que o DLP mascarou
//! na legenda fica mascarado também na voz**. Montar um segundo ASR do lado do
//! servidor seria pagar duas vezes pela mesma frase e abrir um caminho em que
//! a voz podia dizer o que a legenda escondeu.
//!
//! **A tradução também não.** `ai::translate` fala com o Ollama in-cluster.
//!
//! O que esta peça acrescenta é o fim da cadeia: pedir a voz ao `tts-server`,
//! partir o PCM em quadros de 20 ms, codificá-los em Opus e empurrá-los pela
//! fila que o SFU devolveu.
//!
//! # Três decisões que são regras, e não configuração
//!
//! 1. **Uma fila por língua, com fundo.** Se a síntese se atrasar, as frases
//!    novas NÃO se acumulam sem limite: a mais velha cai e fica no log. Uma
//!    dobragem três frases atrasada já não é dobragem — é confusão, e o
//!    ouvinte não tem como saber que está a ouvir o passado.
//! 2. **Falha-se em silêncio, nunca a meio.** Se o `tts-server` não responde,
//!    aquela frase não se diz e a pista continua viva. O que não se faz é
//!    emitir meio enunciado: quem está a ouvir a dobragem não tem o original
//!    como rede (baixou-o), e uma frase cortada a meio é pior do que a sua
//!    ausência.
//! 3. **O ritmo é o do áudio, não o da CPU.** Os quadros saem a 20 ms de
//!    intervalo, como sairiam de um microfone. Despejá-los o mais depressa
//!    possível encheria o jitter buffer do ouvinte e dava fala acelerada.

use crate::sfu::BridgePacket;
use crate::AppState;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;
use webrtc::rtp::header::Header;

/// Opus é sempre 48 kHz na sala, e o `tts-server` entrega já nessa taxa.
const TAXA: u32 = 48_000;
/// 20 ms — o mesmo quadro do resto do áudio desta casa.
const AMOSTRAS_POR_QUADRO: usize = (TAXA / 50) as usize;
/// O payload type do Opus, como a ponte o usa.
const OPUS_PT: u8 = 111;

/// Quantas frases esperam pela voz antes de a mais velha cair.
///
/// Duas, e não vinte: ver a decisão 1 do cabeçalho. Com o tecto de três
/// línguas (ADR-0030) são seis frases em voo no pior caso.
const FILA: usize = 2;

/// Uma frase pronta a dizer, já traduzida.
pub struct Fala {
    pub texto: String,
}

/// Pede a voz ao `tts-server` e devolve o PCM 16-bit mono a 48 kHz.
///
/// `None` quando a dobragem está desligada (sem `TTS_URL`), quando o serviço
/// recusa, ou quando o corpo não é um WAV que saibamos ler — e em qualquer
/// desses casos a frase não se diz (decisão 2).
pub async fn sintetiza(state: &AppState, texto: &str, lang: &str) -> Option<Vec<i16>> {
    let base = state.config.tts_url.as_deref()?;
    let url = format!("{}/synthesize", base.trim_end_matches('/'));
    // A credencial é o MESMO segredo que o whisper-server valida: um token
    // curto, assinado por nós, para o serviço saber que o pedido é do
    // servidor e não de quem alcançou o caminho.
    let token = ficha_de_servico(state)?;
    let resp = state
        .outbound
        .operator()
        .post(&url)
        .bearer_auth(token)
        .json(&serde_json::json!({ "text": texto, "lang": lang }))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        tracing::warn!(%lang, status = %resp.status(), "dobragem: o tts-server recusou a frase");
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    pcm_de_wav(&bytes)
}

/// Uma ficha curta para o `tts-server`, assinada com o `JWT_SECRET`.
///
/// O serviço valida-a como o `whisper-server` valida a dele: sem segredo, nada
/// é aceite. Não é um token de utilizador — a dobragem não age por ninguém,
/// di-la a frase que a sala já difundiu.
fn ficha_de_servico(state: &AppState) -> Option<String> {
    use jsonwebtoken::{encode, EncodingKey, Header as JwtHeader};
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        typ: &'a str,
        exp: u64,
    }
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs()
        + 60;
    encode(
        &JwtHeader::default(),
        &Claims {
            typ: "service",
            exp,
        },
        &EncodingKey::from_secret(state.config.jwt_secret.as_bytes()),
    )
    .ok()
}

/// Extrai as amostras de um WAV PCM 16-bit mono a 48 kHz.
///
/// Lê o cabeçalho em vez de assumir 44 bytes: um WAV com um bloco `LIST` antes
/// do `data` — que algumas bibliotecas escrevem — daria ruído branco se se
/// saltasse um número fixo.
pub fn pcm_de_wav(bytes: &[u8]) -> Option<Vec<i16>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let tam = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?) as usize;
        let corpo = i + 8;
        if id == b"data" {
            let fim = corpo.saturating_add(tam).min(bytes.len());
            return Some(
                bytes[corpo..fim]
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect(),
            );
        }
        i = corpo + tam + (tam & 1); // os blocos RIFF alinham a par
    }
    None
}

/// Parte o PCM em quadros de 20 ms, **enchendo o último com silêncio**.
///
/// A sobra não se deita fora: numa frase de 4,13 s sobram 631 amostras (13 ms
/// de fala), e cortá-las comia a última sílaba. Também não se emite um quadro
/// curto — o Opus recusa um tamanho que não seja dos dele.
pub fn quadros(pcm: &[i16]) -> Vec<Vec<i16>> {
    let mut v: Vec<Vec<i16>> = pcm
        .chunks(AMOSTRAS_POR_QUADRO)
        .map(|c| {
            let mut q = c.to_vec();
            q.resize(AMOSTRAS_POR_QUADRO, 0);
            q
        })
        .collect();
    if v.last().is_some_and(|q| q.iter().all(|&s| s == 0)) && v.len() > 1 {
        // Um quadro inteiramente silencioso no fim não acrescenta nada.
        v.pop();
    }
    v
}

/// A fila de uma língua numa sala: recebe frases já traduzidas e di-las.
pub struct Pista {
    pub(crate) tx: mpsc::Sender<Fala>,
}

impl Pista {
    /// Põe uma frase a caminho. Se a fila estiver cheia, a frase **cai** e
    /// fica no log — ver a decisão 1.
    pub fn diz(&self, texto: String) {
        if self.tx.try_send(Fala { texto }).is_err() {
            tracing::warn!("dobragem: fila cheia, frase descartada (a síntese não acompanha)");
        }
    }
}

/// Arranca a cadeia de uma língua: devolve a ponta por onde se lhe dão frases.
///
/// `rtp` é a fila que o `Sfu::publish_dub_audio` devolveu.
pub fn arranca(
    state: Arc<AppState>,
    lang: String,
    rtp: mpsc::Sender<BridgePacket>,
    ssrc: u32,
) -> Pista {
    let (tx, mut rx) = mpsc::channel::<Fala>(FILA);
    tokio::spawn(async move {
        let Ok(mut enc) = opus_rs::OpusEncoder::new(TAXA as i32, 1, opus_rs::Application::Voip)
        else {
            tracing::error!(%lang, "dobragem: codificador Opus não arrancou — pista morta");
            return;
        };
        enc.bitrate_bps = 32_000;
        let mut ts: u32 = 0;
        let mut seq: u16 = 0;
        let mut primeiro = true;
        while let Some(f) = rx.recv().await {
            let Some(pcm) = sintetiza(&state, &f.texto, &lang).await else {
                continue; // decisão 2: cala-se a frase, não a pista
            };
            let mut relogio = tokio::time::interval(Duration::from_millis(20));
            relogio.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            for q in quadros(&pcm) {
                relogio.tick().await; // decisão 3: o ritmo é o do áudio
                let pcm_f: Vec<f32> = q.iter().map(|&s| s as f32 / 32768.0).collect();
                let mut saida = vec![0u8; 1500];
                let Ok(n) = enc.encode(&pcm_f, AMOSTRAS_POR_QUADRO, &mut saida) else {
                    break;
                };
                saida.truncate(n);
                let packet = webrtc::rtp::packet::Packet {
                    header: Header {
                        version: 2,
                        // O começo de um período de fala — é o que diz ao
                        // ouvinte que isto é uma elocução nova e não a
                        // continuação da anterior.
                        marker: primeiro,
                        payload_type: OPUS_PT,
                        sequence_number: seq,
                        timestamp: ts,
                        ssrc,
                        ..Default::default()
                    },
                    payload: bytes::Bytes::from(saida),
                };
                primeiro = false;
                seq = seq.wrapping_add(1);
                ts = ts.wrapping_add(AMOSTRAS_POR_QUADRO as u32);
                if rtp.send(BridgePacket { packet, level: 40 }).await.is_err() {
                    return; // a pista saiu da sala
                }
            }
            // A frase acabou: a próxima volta a marcar começo de fala.
            primeiro = true;
        }
    });
    Pista { tx }
}

/// Um identificador estável para o publicador sintético de uma língua numa
/// sala. Estável de propósito: uma reentrada não cria um segundo publicador
/// para a mesma língua.
///
/// Derivado com o `sha2` que o repositório já usa, e **marcado como versão 8**
/// — «personalizado», na RFC 9562. Um v5 exigiria ligar mais uma funcionalidade
/// do crate (e o SHA-1 que vem com ela); dizer que é v5 sem o ser seria pior do
/// que ambas as coisas.
pub fn dub_id(room_id: Uuid, lang: &str) -> Uuid {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(room_id.as_bytes());
    h.update(b"/dub/");
    h.update(lang.as_bytes());
    let d = h.finalize();
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    b[6] = (b[6] & 0x0f) | 0x80; // versão 8
    b[8] = (b[8] & 0x3f) | 0x80; // variante RFC
    Uuid::from_bytes(b)
}

/// As línguas activas de cada sala, e a ponta por onde se lhes dão frases.
///
/// Vive aqui e não no SFU porque é estado de PRODUTO (que línguas a sala
/// oferece), não de media. O SFU sabe encaminhar uma pista; quem decide que
/// ela existe é isto.
#[derive(Default)]
pub struct Registo {
    salas: dashmap::DashMap<Uuid, std::collections::HashMap<String, Pista>>,
}

impl Registo {
    /// Liga uma língua numa sala. Recusa pelas regras do domínio — incluindo o
    /// tecto de [`delonix_meet_domain::conferencing::dubbing::MAX_LINGUAS`].
    ///
    /// `publicar` é o que devolve a fila de RTP (na prática,
    /// `Sfu::publish_dub_audio`), e recebe-se como argumento para esta peça
    /// não ter de conhecer o SFU.
    pub async fn liga<F, Fut>(
        &self,
        state: Arc<AppState>,
        room_id: Uuid,
        origem: &str,
        lang: &str,
        publicar: F,
    ) -> Result<(), delonix_meet_core::DomainError>
    where
        F: FnOnce(Uuid, String) -> Fut,
        Fut: std::future::Future<Output = mpsc::Sender<BridgePacket>>,
    {
        use delonix_meet_domain::conferencing::dubbing as regras;
        let activas: Vec<String> = self
            .salas
            .get(&room_id)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        regras::pode_acrescentar(&activas, origem, lang)?;

        let id = dub_id(room_id, lang);
        // O SSRC vem do mesmo sítio que o id: estável por (sala, língua), para
        // uma reentrada não parecer uma fonte nova ao ouvinte.
        let ssrc = u32::from_be_bytes(id.as_bytes()[0..4].try_into().unwrap_or([0; 4]));
        let rtp = publicar(id, lang.to_string()).await;
        let pista = arranca(state, lang.to_string(), rtp, ssrc);
        self.salas
            .entry(room_id)
            .or_default()
            .insert(lang.into(), pista);
        tracing::info!(%room_id, %lang, "dobragem: língua ligada");
        Ok(())
    }

    /// Desliga uma língua: largar a `Pista` fecha a fila, e o SFU retira a
    /// publicação sozinho. **Liberta a vaga de imediato** — o tecto é de
    /// línguas activas, não de línguas já pedidas (ADR-0030).
    pub fn desliga(&self, room_id: Uuid, lang: &str) {
        if let Some(mut m) = self.salas.get_mut(&room_id) {
            if m.remove(lang).is_some() {
                tracing::info!(%room_id, %lang, "dobragem: língua desligada");
            }
        }
    }

    /// As línguas activas de uma sala.
    pub fn activas(&self, room_id: Uuid) -> Vec<String> {
        let mut v: Vec<String> = self
            .salas
            .get(&room_id)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    /// A sala acabou: todas as pistas saem.
    pub fn fecha(&self, room_id: Uuid) {
        self.salas.remove(&room_id);
    }

    /// Uma frase final para todas as línguas activas da sala.
    ///
    /// A tradução é por LÍNGUA e não por ouvinte — é a §2 do ADR aplicada ao
    /// texto: três línguas são três pedidos ao LLM, não um por pessoa.
    pub fn diz(&self, state: Arc<AppState>, room_id: Uuid, texto: &str) {
        let langs = self.activas(room_id);
        if langs.is_empty() {
            return;
        }
        let texto = texto.to_string();
        let salas = &self.salas;
        for lang in langs {
            let Some(pista) = salas
                .get(&room_id)
                .and_then(|m| m.get(&lang).map(|p| p.tx.clone()))
            else {
                continue;
            };
            let state = state.clone();
            let texto = texto.clone();
            tokio::spawn(async move {
                let Some(traduzido) = crate::ai::translate(&state, &texto, &lang).await else {
                    // Sem tradução não se diz nada — dizer o original na pista
                    // da tradução seria pior do que o silêncio: quem a escolheu
                    // baixou o original exactamente para não o ouvir.
                    tracing::warn!(%lang, "dobragem: tradução falhou, frase calada");
                    return;
                };
                if pista.try_send(Fala { texto: traduzido }).is_err() {
                    tracing::warn!(%lang, "dobragem: fila cheia, frase descartada");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sobra_de_amostras_nao_se_perde() {
        // 4,13 s a 48 kHz = 198 391 amostras: 206 quadros inteiros e 631 de
        // sobra. A sobra é a última sílaba — cortá-la ouve-se.
        let pcm: Vec<i16> = (0..198_391).map(|i| (i % 100) as i16).collect();
        let q = quadros(&pcm);
        assert_eq!(q.len(), 207, "206 inteiros + 1 com a sobra");
        assert!(q.iter().all(|f| f.len() == AMOSTRAS_POR_QUADRO));
        // A sobra está lá, e o resto do último quadro é silêncio.
        assert_eq!(q[206][630], pcm[198_390]);
        assert_eq!(q[206][631], 0);
    }

    #[test]
    fn um_quadro_exacto_nao_ganha_um_quadro_de_silencio() {
        let pcm: Vec<i16> = vec![7; AMOSTRAS_POR_QUADRO * 3];
        assert_eq!(quadros(&pcm).len(), 3);
    }

    #[test]
    fn o_wav_le_se_pelo_cabecalho_e_nao_por_um_salto_fixo() {
        // Um WAV com um bloco LIST antes do `data` — saltar 44 bytes daria
        // ruído branco.
        let amostras: Vec<i16> = vec![-5, 0, 5, 10];
        let dados: Vec<u8> = amostras.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&0u32.to_le_bytes());
        w.extend_from_slice(b"WAVE");
        w.extend_from_slice(b"fmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&[0u8; 16]);
        w.extend_from_slice(b"LIST");
        w.extend_from_slice(&4u32.to_le_bytes());
        w.extend_from_slice(b"INFO");
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(dados.len() as u32).to_le_bytes());
        w.extend_from_slice(&dados);
        assert_eq!(pcm_de_wav(&w), Some(amostras));
    }

    #[test]
    fn o_que_nao_e_wav_nao_passa_por_audio() {
        assert_eq!(pcm_de_wav(b"nao-e-um-webm"), None);
        assert_eq!(pcm_de_wav(b""), None);
        assert_eq!(pcm_de_wav(b"RIFF____WAVE"), None, "WAVE sem bloco data");
    }

    #[test]
    fn o_publicador_de_uma_lingua_e_estavel_e_distinto_por_lingua() {
        let sala = Uuid::new_v4();
        assert_eq!(dub_id(sala, "en-US"), dub_id(sala, "en-US"));
        assert_ne!(dub_id(sala, "en-US"), dub_id(sala, "fr-FR"));
        assert_ne!(dub_id(sala, "en-US"), dub_id(Uuid::new_v4(), "en-US"));
    }
}
