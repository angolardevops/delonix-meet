use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
};
use dashmap::DashMap;
use futures_util::{stream::SplitStream, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering::Relaxed},
    sync::Arc,
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::{auth::verify_jwt, error::ApiError, AppState};

// ---------- Protocol ----------

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ClientMsg {
    Offer {
        to: Uuid,
        sdp: String,
    },
    Answer {
        to: Uuid,
        sdp: String,
    },
    Ice {
        to: Uuid,
        candidate: serde_json::Value,
    },
    Chat {
        text: String,
    },
    Reaction {
        emoji: String,
    },
    Hand {
        raised: bool,
    },
    /// De que participantes este cliente quer receber VÍDEO (a página da
    /// grelha visível). O SFU deixa de enviar o resto — ver
    /// `SfuState::set_video_interest`. Áudio nunca depende disto.
    VideoInterest {
        peers: Vec<Uuid>,
        /// Camada DESEJADA por publicador (`"q"` | `"h"` | `"f"`), decidida no
        /// cliente — é lá que se sabe o tamanho a que o tile está desenhado, se
        /// a aba está em segundo plano, se a máquina está travada por CPU, a
        /// bateria e a poupança de dados. Ver `web/src/layerPolicy.ts`.
        ///
        /// Ausente (cliente antigo) => o servidor decide como sempre decidiu,
        /// pelo tamanho da sala. É uma SUGESTÃO: a perda medida por RTCP corta
        /// por cima dela, e o servidor limita quantas camadas altas um
        /// subscritor pode segurar.
        #[serde(default)]
        quality: Option<std::collections::HashMap<Uuid, String>>,
    },
    /// Estado local de câmara/microfone — os outros mostram avatar/ícone
    /// em vez de vídeo preto ou indicador de som errado.
    Media {
        cam: bool,
        mic: bool,
    },
    Recording {
        active: bool,
    },
    Transcript {
        text: String,
    },
    /// Legenda ao vivo (parcial) — enquanto a pessoa ainda fala. Difundida a
    /// todos e substituída pela `Transcript` final; NUNCA persiste na ata.
    TranscriptInterim {
        text: String,
    },
    /// Anfitrião liga/desliga a transcrição PARTILHADA (Nota AI). Ao ligar,
    /// todos os clientes transcrevem o PRÓPRIO microfone e difundem as frases —
    /// capta todos os oradores, não só o mic de quem iniciou.
    TranscriptionToggle {
        on: bool,
    },
    // Só o anfitrião:
    Admit {
        to: Uuid,
    },
    Deny {
        to: Uuid,
    },
    ForceMute {
        to: Uuid,
    },
    Kick {
        to: Uuid,
    },
    // Controlos do anfitrião sobre a sala (em runtime):
    RoomLock {
        locked: bool,
    },
    HostShareOnly {
        on: bool,
    },
    // Ferramentas de reunião: sondagens, Q&A e temporizador.
    PollCreate {
        question: String,
        options: Vec<String>,
        /// Quiz: índice da resposta certa (opcional).
        #[serde(default)]
        correct_option: Option<usize>,
        /// Quiz com tempo: duração da votação em segundos (opcional).
        #[serde(default)]
        duration_secs: Option<u64>,
    },
    PollVote {
        poll: Uuid,
        option: usize,
    },
    PollClose {
        poll: Uuid,
    },
    QaAsk {
        text: String,
    },
    QaUpvote {
        id: Uuid,
    },
    QaAnswered {
        id: Uuid,
    },
    TimerSet {
        minutes: u32,
    },
    TimerClear,
    // Quadro branco colaborativo: traços com coordenadas normalizadas 0..1.
    WbStroke {
        stroke: WbStrokeData,
    },
    WbClear,
    /// Fecha o quadro branco em TODOS os participantes (não só localmente).
    WbClose,
    /// Gravação no servidor (só anfitrião, só salas SFU). Em salas E2EE o
    /// anfitrião cede a chave (base64) só para a duração da gravação.
    ServerRecord {
        active: bool,
        #[serde(default)]
        e2ee_key: Option<Secret>,
    },
    /// Anuncia partilha de ecrã: a próxima track de vídeo sem rid é o ecrã.
    ScreenShare {
        on: bool,
    },
    BreakoutsCreate {
        count: u32,
        #[serde(default)]
        minutes: Option<u32>,
    },
    BreakoutRename {
        code: String,
        label: String,
    },
    BreakoutAdd,
    /// Move um participante (identificado pelo username) para o grupo `code`
    /// — ou de volta à principal se `code` for o da sala-mãe.
    BreakoutMoveUser {
        name: String,
        code: String,
    },
    BreakoutsClose,
    Leave,
    // SFU mode: SDP/ICE exchanged with the server itself, not another peer.
    SfuOffer {
        sdp: String,
    },
    SfuAnswer {
        sdp: String,
    },
    SfuIce {
        candidate: serde_json::Value,
    },
    RemoteControl {
        to: Uuid,
        action: String,
        payload: serde_json::Value,
    },
    /// Anfitrião autoriza/revoga a partilha de ecrã de um participante.
    ShareGrant {
        to: Uuid,
        allowed: bool,
    },
    /// Não-anfitrião pede ao anfitrião autorização para partilhar o ecrã.
    ShareRequest,
    /// Apresentador (ou anfitrião) abre o quadro branco em todos.
    WbOpen,
}

/// Serializa um `Secret` **de propósito**. Existe uma única chamada — o campo
/// `reconnect` do `Joined` — e é para ela que este nome é assim tão explícito:
/// quem acrescentar a segunda tem de escrever o nome outra vez e reparar no que
/// está a fazer.
fn serializa_segredo_deliberadamente<S>(v: &Option<Secret>, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match v {
        Some(sec) => s.serialize_str(sec.expose()),
        None => s.serialize_none(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ServerMsg {
    Joined {
        peer_id: Uuid,
        peers: Vec<PeerInfo>,
        /// Segredo para reclamar este lugar se o socket cair (R91). Vai
        /// SÓ para quem entrou, na sua própria mensagem de entrada — nunca
        /// num `PeerJoined`, que toda a sala recebe.
        ///
        /// O `Secret` NÃO implementa `Serialize` de propósito: é o que impede
        /// um segredo de escorregar para uma mensagem por acidente. Aqui a
        /// saída é deliberada e está isolada num serializador só deste campo,
        /// para o `Debug` redigido (R43) continuar a valer em todo o resto.
        #[serde(
            skip_serializing_if = "Option::is_none",
            serialize_with = "serializa_segredo_deliberadamente"
        )]
        reconnect: Option<Secret>,
    },
    PeerJoined {
        peer: PeerInfo,
    },
    PeerLeft {
        peer_id: Uuid,
    },
    /// O socket deste participante caiu, mas o lugar dele está reservado
    /// (R91). É deliberadamente DIFERENTE de `PeerLeft`: o cliente mantém o
    /// retrato no sítio em vez de o remover e voltar a criar, e o anfitrião não
    /// vê uma saída seguida de um pedido novo de admissão.
    PeerReconnecting {
        peer_id: Uuid,
    },
    Offer {
        from: Uuid,
        sdp: String,
    },
    Answer {
        from: Uuid,
        sdp: String,
    },
    Ice {
        from: Uuid,
        candidate: serde_json::Value,
    },
    Chat {
        from: Uuid,
        username: String,
        text: String,
    },
    Reaction {
        from: Uuid,
        username: String,
        emoji: String,
    },
    Hand {
        from: Uuid,
        raised: bool,
    },
    Media {
        from: Uuid,
        cam: bool,
        mic: bool,
    },
    Recording {
        from: Uuid,
        username: String,
        active: bool,
    },
    Transcript {
        from: Uuid,
        username: String,
        text: String,
    },
    /// Legenda ao vivo (parcial) de um orador — atualiza a legenda em tempo
    /// real; é substituída pela `Transcript` final.
    TranscriptInterim {
        from: Uuid,
        username: String,
        text: String,
    },
    /// Difusão: o anfitrião ligou/desligou a transcrição partilhada. Todos os
    /// clientes começam/param de transcrever o próprio microfone.
    Transcription {
        on: bool,
        by: String,
    },
    // Sala de espera:
    Waiting, // para o convidado: estás em espera
    WaitingJoin {
        peer: PeerInfo,
    }, // para o anfitrião: alguém espera
    WaitingLeft {
        peer_id: Uuid,
    }, // para o anfitrião: desistiu
    Denied,  // para o convidado: entrada recusada
    // Controlo do anfitrião:
    ForceMuted, // para o alvo: foste silenciado
    Kicked,     // para o alvo: foste removido
    /// Definições runtime da sala (lock, só-anfitrião-partilha).
    RoomSettings {
        locked: bool,
        host_share_only: bool,
    },
    // Ferramentas: estado completo difundido a cada mudança.
    Polls {
        polls: Vec<PollView>,
    },
    Qa {
        questions: Vec<QaView>,
    },
    Timer {
        ends_at: Option<i64>,
    },
    ServerRecording {
        active: bool,
        by: String,
    },
    WbStroke {
        stroke: WbStrokeData,
    },
    WbClear,
    /// Fecha o quadro em todos os participantes.
    WbClose,
    /// Snapshot do quadro para quem entra a meio.
    WbState {
        strokes: Vec<WbStrokeData>,
    },
    /// Difusão fiável de quem está a apresentar (ecrã). Permite aos recetores
    /// definir/limpar a apresentação sem depender de eventos frágeis de track.
    Presenting {
        from: Uuid,
        on: bool,
    },
    // Breakout rooms:
    BreakoutMove {
        code: String,
        label: String,
        back: bool,
        ends_at: Option<i64>,
    },
    BreakoutsCreated {
        rooms: Vec<BreakoutInfo>,
        ends_at: Option<i64>,
    },
    Error {
        message: String,
    },
    SfuOffer {
        sdp: String,
    },
    SfuAnswer {
        sdp: String,
    },
    SfuIce {
        candidate: serde_json::Value,
    },
    RemoteControl {
        from: Uuid,
        action: String,
        payload: serde_json::Value,
    },
    /// Resposta do anfitrião a um pedido de partilha (ou grant direto).
    ShareGranted {
        allowed: bool,
    },
    /// Pedido de partilha de ecrã de um não-anfitrião (entregue aos anfitriões).
    ShareRequest {
        from: Uuid,
        username: String,
    },
    /// O apresentador abriu o quadro branco — abre em todos.
    WbOpen {
        by: String,
    },
    /// Este nó está a DRENAR (vai fechar). O cliente reconecta daqui a
    /// `reconnect_in_ms` — e, como o pod já saiu dos endpoints do balanceador,
    /// o hash por sala manda a sala INTEIRA para o mesmo pod novo. É isso que
    /// permite migrar sem partir o SFU, que é in-memory por pod (ADR-0001).
    ///
    /// NÃO é um erro nem um `kicked`: a chamada continua a funcionar até o
    /// cliente decidir migrar.
    Draining {
        reconnect_in_ms: u64,
    },
}

/// Traço do quadro branco: pontos normalizados (0..1), cor CSS e espessura.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WbStrokeData {
    pub pts: Vec<[f32; 2]>,
    pub c: String,
    pub w: f32,
}

// ---------- Ferramentas de reunião (estado em memória por sala) ----------

/// Vista pública de uma sondagem: contagens agregadas, sem revelar quem
/// votou em quê (o cliente lembra o próprio voto localmente).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollView {
    pub id: Uuid,
    pub question: String,
    pub options: Vec<String>,
    pub counts: Vec<u32>,
    pub open: bool,
    pub by: String,
    /// Quiz: índice da resposta certa — SÓ revelado depois de fechar (None
    /// enquanto aberta, mesmo que exista, para ninguém fazer batota).
    pub correct: Option<usize>,
    /// Quiz com tempo: epoch ms do fim da votação (contagem no cliente).
    pub ends_at: Option<i64>,
    /// Totais revelados no fecho (0 enquanto aberta ou sem resposta certa).
    pub total_right: u32,
    pub total_wrong: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaView {
    pub id: Uuid,
    pub text: String,
    pub by: String,
    pub upvotes: u32,
    pub answered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollState {
    pub id: Uuid,
    pub question: String,
    pub options: Vec<String>,
    pub votes: HashMap<Uuid, usize>,
    pub open: bool,
    pub by: String,
    // Quiz (serde default: polls antigas no Redis continuam a desserializar).
    #[serde(default)]
    pub correct: Option<usize>,
    #[serde(default)]
    pub ends_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaState {
    pub id: Uuid,
    pub text: String,
    pub by: String,
    pub upvotes: std::collections::HashSet<Uuid>,
    pub answered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakoutInfo {
    pub code: String,
    pub label: String,
    /// Quem está agora neste grupo (usernames) — para o anfitrião mover pessoas.
    pub people: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BreakoutChild {
    pub id: Uuid,
    pub code: String,
    pub label: String,
}

/// Conjunto de salas de grupo ativas de uma sala principal.
pub struct BreakoutSet {
    pub parent_code: String,
    pub children: Vec<BreakoutChild>,
    /// Deadline (epoch segundos) do temporizador, se definido.
    pub ends_at: Option<i64>,
    /// Distingue gerações de grupos: um timer antigo não fecha grupos novos.
    pub nonce: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: Uuid,
    pub username: String,
    pub host: bool,
    #[serde(default)]
    pub hand: bool,
    /// Estado de media conhecido (true até o peer dizer o contrário).
    pub cam: bool,
    pub mic: bool,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub is_pstn: bool,
}

// ---------- Hub (room registry, WS-agnostic and unit-testable) ----------

/// Acima deste número de participantes as legendas PARCIAIS deixam de ser
/// difundidas (a final continua). O fan-out é n×n e o ganho de ver a frase a
/// formar-se não compensa milhares de mensagens por segundo numa sala grande.
const INTERIM_MAX_ROOM: usize = 12;
/// Parciais por segundo aceites de CADA emissor (o cliente já se auto-limita a
/// 4/s; isto impede que um cliente alterado ou modificado inunde a sala).
const INTERIM_BURST: f64 = 8.0;
const INTERIM_PER_SEC: f64 = 4.0;

/// Uma `String` que carrega SEGREDO: nunca aparece num `Debug`, e os bytes são
/// limpos da memória quando é largada.
///
/// Existe por causa de uma armadilha concreta: o `ClientMsg` deriva `Debug`, e
/// a chave E2EE cedida pelo anfitrião viaja lá dentro. Hoje nenhum log imprime
/// a mensagem inteira — verificado —, mas basta um `tracing::debug!(?msg)`
/// acrescentado por boas razões num dia mau para a chave AES-256 da sala ir
/// parar ao ficheiro de log, em base64, pronta a ler. A garantia não pode
/// depender de ninguém se lembrar disto.
#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Embrulha um segredo. O campo é privado de propósito: obriga a passar
    /// por aqui, e por aqui vê-se que o valor entra num tipo com `Debug`
    /// redigido e `Drop` que sobrescreve (R43).
    pub fn new(valor: String) -> Self {
        Self(valor)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Nem o comprimento: num campo de tamanho fixo (32 bytes em base64) o
        // comprimento não acrescenta nada e a ausência é mais fácil de auditar.
        f.write_str("[segredo redigido]")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Sobrescreve antes de libertar. Não é uma garantia forte em Rust (a
        // String pode ter sido realocada antes disto, e o optimizador podia
        // eliminar a escrita se não fosse por volatile dentro do zeroize), mas
        // reduz a janela em que a chave fica legível em memória libertada.
        use zeroize::Zeroize;
        self.0.zeroize();
    }
}

impl ServerMsg {
    /// Esta mensagem pode perder-se sem quebrar o protocolo nem o estado?
    ///
    /// Só é `true` para o que é EFÉMERO e auto-substituível: a legenda parcial
    /// é substituída pela final, o traço de quadro só existe enquanto se
    /// desenha, e a reacção some ao fim de segundos. Tudo o resto — oferta,
    /// resposta, ICE, entrada/saída, admissão, definições da sala, estado das
    /// ferramentas — é PROTOCOLO ou ESTADO: perder uma mensagem dessas deixa o
    /// cliente a acreditar num sistema que já não existe, que é pior do que o
    /// desligar. Por isso o transbordo com uma destas fecha o socket.
    fn is_droppable(&self) -> bool {
        matches!(
            self,
            ServerMsg::TranscriptInterim { .. }
                | ServerMsg::WbStroke { .. }
                | ServerMsg::Reaction { .. }
        )
    }
}

/// Fila de saída de UM socket de sinalização — **limitada**.
///
/// Porquê limitada: o `writer` só drena a fila ao ritmo a que o TCP do cliente
/// aceita bytes. Um cliente numa rede degradada (o caso normal do nosso
/// mercado), com a aba suspensa, ou parado num depurador, deixa de drenar — e
/// a sala continua a difundir-lhe traços de quadro, legendas e ICE. Com uma
/// fila ilimitada isso cresce até à memória do nó acabar: UM consumidor lento
/// derruba o pod e com ele TODAS as salas nesse pod (a afinidade por sala
/// concentra-as, ver ADR-0001). O limite troca essa falha global por uma
/// falha local e explícita.
///
/// Política no transbordo, por ordem:
///  1. mensagem descartável → descarta e conta (`ws_queue_dropped_total`);
///  2. mensagem de protocolo/estado → fecha o socket UMA vez
///     (`ws_slow_consumer_kills_total`) e o cliente reentra.
///
/// Nunca `send().await`: os emissores estão dentro do lock do `DashMap` das
/// salas (ver R16 — segurar o lock através de um `await` trava a sala inteira).
/// É sempre `try_send`.
#[derive(Clone)]
pub struct PeerTx {
    tx: mpsc::Sender<ServerMsg>,
    cap: usize,
    /// Acorda o laço de leitura do socket para ele terminar de forma ordenada.
    shutdown: Arc<tokio::sync::Notify>,
    /// Garante que um socket em transbordo só conta como UMA morte, por muitas
    /// difusões que ainda lhe sejam dirigidas antes de o laço reagir.
    killed: Arc<AtomicBool>,
    metrics: Arc<crate::metrics::Metrics>,
}

impl PeerTx {
    pub fn new(
        cap: usize,
        metrics: Arc<crate::metrics::Metrics>,
    ) -> (Self, mpsc::Receiver<ServerMsg>, Arc<tokio::sync::Notify>) {
        let (tx, rx) = mpsc::channel(cap);
        let shutdown = Arc::new(tokio::sync::Notify::new());
        (
            Self {
                tx,
                cap,
                shutdown: shutdown.clone(),
                killed: Arc::new(AtomicBool::new(false)),
                metrics,
            },
            rx,
            shutdown,
        )
    }

    /// Entrega best-effort. `true` = entrou na fila.
    pub fn send(&self, msg: ServerMsg) -> bool {
        let droppable = msg.is_droppable();
        match self.tx.try_send(msg) {
            Ok(()) => {
                // Marca de água: quanto é que esta fila chegou a acumular.
                let used = self.cap.saturating_sub(self.tx.capacity()) as i64;
                self.metrics.ws_queue_high_water.fetch_max(used, Relaxed);
                true
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
            Err(mpsc::error::TrySendError::Full(_)) => {
                if droppable {
                    crate::metrics::Metrics::bump(&self.metrics.ws_queue_dropped_total);
                } else if !self.killed.swap(true, Relaxed) {
                    crate::metrics::Metrics::bump(&self.metrics.ws_slow_consumer_kills_total);
                    tracing::warn!(
                        cap = self.cap,
                        "consumidor lento: fila de saída cheia com mensagem de protocolo — a fechar o socket"
                    );
                    self.shutdown.notify_one();
                }
                false
            }
        }
    }
}

struct Peer {
    username: String,
    user_id: Uuid,
    is_host: bool,
    can_admit: bool,
    hand: bool,
    cam_on: bool,
    mic_on: bool,
    is_bot: bool,
    is_pstn: bool,
    tx: PeerTx,
    /// Travão das legendas parciais deste peer (ver `allow_interim`).
    interim: crate::rate_limit::TokenBucket,
    /// Segredo que permite a este participante RECLAMAR o seu lugar depois de
    /// o socket cair (R91). Opaco, aleatório, e nunca sai deste processo a não
    /// ser para o próprio dono, no `joined`.
    ///
    /// Porque não se reutiliza o token de sala: esse é uma capability sobre a
    /// SALA — quem o tiver entra como quem quiser. Este é sobre o LUGAR: prova
    /// que quem volta é quem estava, e é o que autoriza herdar o papel de
    /// anfitrião. Confundir os dois dá promoção a anfitrião por conhecer um
    /// link.
    reconnect_secret: Secret,
    /// `Some` desde que o socket caiu. Enquanto estiver dentro da janela de
    /// graça o lugar continua ocupado: o participante conta para a sala, o
    /// papel não se perde, e os outros veem-no «a voltar» em vez de sair.
    disconnected_at: Option<std::time::Instant>,
}

/// O que se herda ao reclamar um lugar. Só o papel e a identidade — nada de
/// estado de media, que se renegoceia do zero com o socket novo.
pub struct ReclaimedSeat {
    pub peer_id: Uuid,
    pub username: String,
    pub user_id: Uuid,
    pub is_host: bool,
    pub can_admit: bool,
}

/// 32 bytes de `OsRng` em hexadecimal. Não é um JWT de propósito: não precisa
/// de ser lido por ninguém, não transporta afirmações, e um valor opaco não
/// tenta ninguém a decidir coisas a partir do que lá está dentro.
fn novo_segredo_de_reclamacao() -> String {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

struct WaitingPeer {
    username: String,
    admit_tx: oneshot::Sender<bool>,
}

#[derive(Default)]
pub(crate) struct Room {
    peers: HashMap<Uuid, Peer>,
    waiting: HashMap<Uuid, WaitingPeer>,
    /// Reunião bloqueada: ninguém entra sem ser admitido (mesmo com link).
    locked: bool,
    /// Só o anfitrião pode partilhar ecrã.
    host_share_only: bool,
    /// Autorizações de partilha de ecrã dadas pelo anfitrião (por peer).
    share_grants: HashSet<Uuid>,
    /// Quem está a apresentar agora — gateia controlo remoto e wb-open.
    presenter: Option<Uuid>,
    // Ferramentas de reunião:
    pub(crate) polls: Vec<PollState>,
    pub(crate) questions: Vec<QaState>,
    pub(crate) timer_ends_at: Option<i64>,
    /// Quadro branco: traços acumulados (repostos a quem entra).
    pub(crate) wb_strokes: Vec<WbStrokeData>,
}

#[derive(Default)]
pub struct SignalingHub {
    pub(crate) rooms: DashMap<Uuid, Room>,
    pub bus: Option<Arc<crate::pubsub::PubSubBus>>,
}

impl SignalingHub {
    /// Quantos participantes há na sala (0 se não existir).
    fn peer_count(&self, room_id: Uuid) -> usize {
        self.rooms.get(&room_id).map(|r| r.peers.len()).unwrap_or(0)
    }

    /// Consome um token do travão de legendas parciais deste peer.
    fn allow_interim(&self, room_id: Uuid, peer_id: Uuid) -> bool {
        match self.rooms.get_mut(&room_id) {
            Some(mut room) => room
                .peers
                .get_mut(&peer_id)
                .map(|p| p.interim.allow())
                .unwrap_or(false),
            None => false,
        }
    }

    /// Adds a peer to a room. Announces it to the others and returns the
    /// current roster (excluding the new peer itself). Hosts also receive
    /// the queue of guests already waiting.
    pub fn apply_redis_state(
        &self,
        room_id: Uuid,
        polls: Vec<PollState>,
        questions: Vec<QaState>,
        wb_strokes: Vec<WbStrokeData>,
        timer_ends_at: Option<i64>,
        locked: bool,
        host_share_only: bool,
    ) {
        let mut room = self.rooms.entry(room_id).or_default();
        room.polls = polls;
        room.questions = questions;
        room.wb_strokes = wb_strokes;
        room.timer_ends_at = timer_ends_at;
        room.locked = locked;
        room.host_share_only = host_share_only;
    }

    pub fn join(
        &self,
        room_id: Uuid,
        peer_id: Uuid,
        user_id: Uuid,
        username: String,
        is_host: bool,
        can_admit: bool,
        is_bot: bool,
        tx: PeerTx,
    ) -> (Vec<PeerInfo>, String) {
        // O segredo de reclamação nasce aqui e é a ÚNICA coisa que sai deste
        // método além do roster: quem entra leva-o, ninguém mais o vê.
        let segredo = novo_segredo_de_reclamacao();
        // Collect data and mutate under the DashMap write lock, then release
        // BEFORE broadcasting. Broadcasting calls broadcast_all_local which
        // calls self.rooms.get() — acquiring a read lock on the same shard
        // while a write lock is held causes a deadlock that hangs tokio threads.
        let (existing, announce, waiting_msgs) = {
            let mut room = self.rooms.entry(room_id).or_default();
            let existing: Vec<PeerInfo> = room
                .peers
                .iter()
                .map(|(id, p)| PeerInfo {
                    peer_id: *id,
                    username: p.username.clone(),
                    host: p.is_host,
                    hand: p.hand,
                    cam: p.cam_on,
                    mic: p.mic_on,
                    is_bot: p.is_bot,
                    is_pstn: p.is_pstn,
                })
                .collect();
            let announce = ServerMsg::PeerJoined {
                peer: PeerInfo {
                    peer_id,
                    username: username.clone(),
                    host: is_host,
                    hand: false,
                    cam: true,
                    mic: true,
                    is_bot,
                    is_pstn: false,
                },
            };
            let waiting_msgs: Vec<ServerMsg> = if is_host {
                room.waiting
                    .iter()
                    .map(|(id, w)| ServerMsg::WaitingJoin {
                        peer: PeerInfo {
                            peer_id: *id,
                            username: w.username.clone(),
                            host: false,
                            hand: false,
                            cam: true,
                            mic: true,
                            is_bot: false,
                            is_pstn: false,
                        },
                    })
                    .collect()
            } else {
                vec![]
            };
            room.peers.insert(
                peer_id,
                Peer {
                    username,
                    user_id,
                    is_host,
                    can_admit,
                    hand: false,
                    cam_on: true,
                    mic_on: true,
                    is_bot,
                    is_pstn: false,
                    tx: tx.clone(),
                    interim: crate::rate_limit::TokenBucket::new(INTERIM_BURST, INTERIM_PER_SEC),
                    reconnect_secret: Secret::new(segredo.clone()),
                    disconnected_at: None,
                },
            );
            (existing, announce, waiting_msgs)
        }; // ← DashMap write lock released here
        self.broadcast_all(room_id, announce);
        for msg in waiting_msgs {
            let _ = tx.send(msg);
        }
        (existing, segredo)
    }

    /// O socket caiu, mas o lugar NÃO se perde já (R91).
    ///
    /// Antes desta janela, um `F5` a meio de uma reunião era indistinguível de
    /// sair: o `peer_id` nasce por socket, o papel de anfitrião ia com ele, as
    /// autorizações de partilha desapareciam, e um convidado voltava a cair na
    /// sala de espera à espera de ser admitido outra vez.
    ///
    /// Devolve `true` se o lugar ficou reservado; `false` se não havia lugar
    /// nenhum (chamada repetida, ou já expirado).
    pub fn disconnect(&self, room_id: Uuid, peer_id: Uuid) -> bool {
        let marcado = self
            .rooms
            .get_mut(&room_id)
            .and_then(|mut r| {
                r.peers.get_mut(&peer_id).map(|p| {
                    p.disconnected_at = Some(std::time::Instant::now());
                })
            })
            .is_some();
        if marcado {
            // «A voltar», não «saiu». A diferença é visível: o retrato fica no
            // sítio em vez de desaparecer e reaparecer, e ninguém tem de ser
            // readmitido.
            self.broadcast_all(room_id, ServerMsg::PeerReconnecting { peer_id });
        }
        marcado
    }

    /// Tenta reclamar um lugar reservado. Devolve o `peer_id` original e o
    /// papel a herdar, ou `None` se o segredo não bate, se o lugar já expirou,
    /// ou se o dono do lugar está VIVO — que é o caso de um segredo roubado a
    /// tentar entrar por cima de quem está lá.
    pub fn reclaim(
        &self,
        room_id: Uuid,
        segredo: &str,
        janela: std::time::Duration,
    ) -> Option<ReclaimedSeat> {
        // Um segredo vazio nunca reclama nada. Sem isto, um cliente que envie
        // `?reconnect=` (vazio) entraria no lugar do primeiro peer da sala.
        if segredo.is_empty() {
            return None;
        }
        let mut room = self.rooms.get_mut(&room_id)?;
        let agora = std::time::Instant::now();
        let (peer_id, papel) = room.peers.iter().find_map(|(id, p)| {
            let caiu = p.disconnected_at?;
            if agora.duration_since(caiu) > janela {
                return None;
            }
            // Comparação em tempo constante: o segredo é uma credencial.
            if !crate::apikeys::ct_eq(p.reconnect_secret.expose().as_bytes(), segredo.as_bytes()) {
                return None;
            }
            Some((
                *id,
                ReclaimedSeat {
                    peer_id: *id,
                    username: p.username.clone(),
                    user_id: p.user_id,
                    is_host: p.is_host,
                    can_admit: p.can_admit,
                },
            ))
        })?;
        // O lugar é consumido: remove-se a entrada antiga para o `join` que se
        // segue a recriar com o socket novo. Sem isto ficavam dois peers com o
        // mesmo id e o roster duplicava.
        room.peers.remove(&peer_id);
        Some(papel)
    }

    /// Varre os lugares reservados que passaram da janela e transforma-os em
    /// saídas a sério. Chamado periodicamente; devolve quantos expiraram.
    pub fn expire_disconnected(&self, janela: std::time::Duration) -> usize {
        let agora = std::time::Instant::now();
        let mut expirados: Vec<(Uuid, Uuid)> = Vec::new();
        for room in self.rooms.iter() {
            for (peer_id, p) in room.peers.iter() {
                if let Some(caiu) = p.disconnected_at {
                    if agora.duration_since(caiu) > janela {
                        expirados.push((*room.key(), *peer_id));
                    }
                }
            }
        }
        for (room_id, peer_id) in &expirados {
            self.leave(*room_id, *peer_id);
        }
        expirados.len()
    }

    /// Regista um convidado na fila de espera e avisa os anfitriões presentes.
    pub fn add_waiting(
        &self,
        room_id: Uuid,
        peer_id: Uuid,
        username: String,
        admit_tx: oneshot::Sender<bool>,
    ) {
        let info = PeerInfo {
            peer_id,
            username: username.clone(),
            host: false,
            hand: false,
            cam: true,
            mic: true,
            is_bot: false,
            is_pstn: false,
        };
        // Insert under lock, broadcast after lock is released.
        {
            let mut room = self.rooms.entry(room_id).or_default();
            room.waiting
                .insert(peer_id, WaitingPeer { username, admit_tx });
        }
        self.broadcast_hosts(room_id, ServerMsg::WaitingJoin { peer: info });
    }

    pub fn remove_waiting(&self, room_id: Uuid, peer_id: Uuid) {
        let removed = self
            .rooms
            .get_mut(&room_id)
            .map(|mut r| r.waiting.remove(&peer_id).is_some())
            .unwrap_or(false);
        if removed {
            self.broadcast_hosts(room_id, ServerMsg::WaitingLeft { peer_id });
        }
    }

    /// Decisão do anfitrião sobre um convidado em espera.
    fn decide_waiting(&self, room_id: Uuid, host: Uuid, target: Uuid, admit: bool) {
        let admitted_tx = {
            let Some(mut room) = self.rooms.get_mut(&room_id) else {
                return;
            };
            if !room.peers.get(&host).map(|p| p.is_host).unwrap_or(false) {
                return; // só o anfitrião decide
            }
            room.waiting.remove(&target).map(|w| w.admit_tx)
        }; // ← DashMap write lock released here
        if let Some(tx) = admitted_tx {
            let _ = tx.send(admit);
            self.broadcast_hosts(room_id, ServerMsg::WaitingLeft { peer_id: target });
        }
    }

    pub fn leave(&self, room_id: Uuid, peer_id: Uuid) {
        let removed = self
            .rooms
            .get_mut(&room_id)
            .map(|mut r| {
                r.share_grants.remove(&peer_id);
                if r.presenter == Some(peer_id) {
                    r.presenter = None;
                }
                r.peers.remove(&peer_id).is_some()
            })
            .unwrap_or(false);
        let empty = self
            .rooms
            .get(&room_id)
            .map(|r| r.peers.is_empty() && r.waiting.is_empty())
            .unwrap_or(false);
        if removed {
            self.broadcast_all(room_id, ServerMsg::PeerLeft { peer_id });
        }
        if empty {
            self.rooms.remove_if(&room_id, |_, room| {
                room.peers.is_empty() && room.waiting.is_empty()
            });
        }
    }

    pub fn send_to(&self, room_id: Uuid, target: Uuid, msg: ServerMsg) -> bool {
        if let Some(bus) = &self.bus {
            let bus = bus.clone();
            let msg_clone = msg.clone();
            tokio::spawn(async move {
                bus.publish_signaling(
                    room_id,
                    &crate::pubsub::RedisRoomEvent::SendTo {
                        node_id: *crate::pubsub::NODE_ID,
                        to: target,
                        msg: msg_clone,
                    },
                )
                .await;
            });
        }
        self.send_to_local(room_id, target, msg)
    }

    pub fn send_to_local(&self, room_id: Uuid, target: Uuid, msg: ServerMsg) -> bool {
        self.rooms
            .get(&room_id)
            .and_then(|room| room.peers.get(&target).map(|p| p.tx.send(msg)))
            .unwrap_or(false)
    }

    pub fn broadcast(&self, room_id: Uuid, from: Uuid, msg: ServerMsg) {
        if let Some(bus) = &self.bus {
            let bus = bus.clone();
            let msg_clone = msg.clone();
            tokio::spawn(async move {
                bus.publish_signaling(
                    room_id,
                    &crate::pubsub::RedisRoomEvent::Broadcast {
                        node_id: *crate::pubsub::NODE_ID,
                        from,
                        msg: msg_clone,
                    },
                )
                .await;
            });
        }
        self.broadcast_local(room_id, from, msg)
    }

    pub fn broadcast_local(&self, room_id: Uuid, from: Uuid, msg: ServerMsg) {
        if let Some(room) = self.rooms.get(&room_id) {
            for (id, peer) in room.peers.iter() {
                if *id != from {
                    let _ = peer.tx.send(msg.clone());
                }
            }
        }
    }

    pub fn broadcast_all(&self, room_id: Uuid, msg: ServerMsg) {
        if let Some(bus) = &self.bus {
            let bus = bus.clone();
            let msg_clone = msg.clone();
            tokio::spawn(async move {
                bus.publish_signaling(
                    room_id,
                    &crate::pubsub::RedisRoomEvent::BroadcastAll {
                        node_id: *crate::pubsub::NODE_ID,
                        msg: msg_clone,
                    },
                )
                .await;
            });
        }
        self.broadcast_all_local(room_id, msg)
    }

    pub fn broadcast_all_local(&self, room_id: Uuid, msg: ServerMsg) {
        if let Some(room) = self.rooms.get(&room_id) {
            for peer in room.peers.values() {
                let _ = peer.tx.send(msg.clone());
            }
        }
    }

    /// Avisa TODAS as salas deste nó de que ele vai fechar. Devolve quantas.
    ///
    /// Não passa pelo Redis de propósito: isto é sobre ESTE pod a fechar, e
    /// difundir para os outros nós mandaria migrar quem não precisa.
    pub fn broadcast_draining(&self, reconnect_in_ms: u64) -> usize {
        let mut salas = 0;
        for room in self.rooms.iter() {
            salas += 1;
            for peer in room.peers.values() {
                peer.tx.send(ServerMsg::Draining { reconnect_in_ms });
            }
        }
        salas
    }

    /// Esta sala já existe neste nó?
    pub fn tem_sala(&self, room_id: Uuid) -> bool {
        self.rooms
            .get(&room_id)
            .is_some_and(|r| !r.peers.is_empty())
    }

    /// Participantes ligados a este nó. É o que diz se o drain já pode fechar.
    pub fn peers_ligados(&self) -> usize {
        self.rooms.iter().map(|r| r.peers.len()).sum()
    }

    pub fn broadcast_hosts(&self, room_id: Uuid, msg: ServerMsg) {
        if let Some(bus) = &self.bus {
            let bus = bus.clone();
            let msg_clone = msg.clone();
            tokio::spawn(async move {
                bus.publish_signaling(
                    room_id,
                    &crate::pubsub::RedisRoomEvent::BroadcastHosts {
                        node_id: *crate::pubsub::NODE_ID,
                        msg: msg_clone,
                    },
                )
                .await;
            });
        }
        self.broadcast_hosts_local(room_id, msg)
    }

    pub fn broadcast_hosts_local(&self, room_id: Uuid, msg: ServerMsg) {
        if let Some(room) = self.rooms.get(&room_id) {
            for peer in room.peers.values().filter(|p| p.is_host) {
                let _ = peer.tx.send(msg.clone());
            }
        }
    }

    /// Lista (peer_id, é_anfitrião) dos presentes na sala.
    /// Roster com usernames — identidade estável entre salas (o peer_id muda
    /// a cada ligação, o username não).
    pub fn roster_named(&self, room_id: Uuid) -> Vec<(Uuid, String, bool)> {
        self.rooms
            .get(&room_id)
            .map(|r| {
                r.peers
                    .iter()
                    .map(|(id, p)| (*id, p.username.clone(), p.is_host))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn roster(&self, room_id: Uuid) -> Vec<(Uuid, bool)> {
        self.rooms
            .get(&room_id)
            .map(|room| room.peers.iter().map(|(id, p)| (*id, p.is_host)).collect())
            .unwrap_or_default()
    }

    pub fn username_of(&self, room_id: Uuid, peer_id: Uuid) -> Option<String> {
        self.rooms
            .get(&room_id)
            .and_then(|room| room.peers.get(&peer_id).map(|p| p.username.clone()))
    }

    /// Sala bloqueada? (usado no join para forçar sala de espera)
    pub fn is_locked(&self, room_id: Uuid) -> bool {
        self.rooms.get(&room_id).map(|r| r.locked).unwrap_or(false)
    }

    pub fn has_peer(&self, room_id: Uuid, peer_id: Uuid) -> bool {
        self.rooms
            .get(&room_id)
            .map(|r| r.peers.contains_key(&peer_id))
            .unwrap_or(false)
    }

    /// IDs de utilizadores actualmente na sala (para o cron de auto-ring).
    pub fn users_in_room(&self, room_id: Uuid) -> std::collections::HashSet<Uuid> {
        self.rooms
            .get(&room_id)
            .map(|room| room.peers.values().map(|p| p.user_id).collect())
            .unwrap_or_default()
    }

    /// Difunde as definições runtime a todos os presentes.
    pub fn broadcast_settings(&self, room_id: Uuid) {
        if let Some(room) = self.rooms.get(&room_id) {
            let msg = ServerMsg::RoomSettings {
                locked: room.locked,
                host_share_only: room.host_share_only,
            };
            for peer in room.peers.values() {
                let _ = peer.tx.send(msg.clone());
            }
        }
    }

    fn polls_view(&self, room_id: Uuid) -> Vec<PollView> {
        self.rooms
            .get(&room_id)
            .map(|r| {
                r.polls
                    .iter()
                    .map(|p| {
                        let mut counts = vec![0u32; p.options.len()];
                        for opt in p.votes.values() {
                            if let Some(c) = counts.get_mut(*opt) {
                                *c += 1;
                            }
                        }
                        let (total_right, total_wrong) = match (p.open, p.correct) {
                            (false, Some(c)) => {
                                let right = p.votes.values().filter(|&&v| v == c).count() as u32;
                                (right, p.votes.len() as u32 - right)
                            }
                            _ => (0, 0),
                        };
                        PollView {
                            id: p.id,
                            question: p.question.clone(),
                            options: p.options.clone(),
                            counts,
                            open: p.open,
                            by: p.by.clone(),
                            // A resposta certa só sai do servidor após o fecho.
                            correct: if p.open { None } else { p.correct },
                            ends_at: p.ends_at,
                            total_right,
                            total_wrong,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn qa_view(&self, room_id: Uuid) -> Vec<QaView> {
        self.rooms
            .get(&room_id)
            .map(|r| {
                let mut qs: Vec<QaView> = r
                    .questions
                    .iter()
                    .map(|q| QaView {
                        id: q.id,
                        text: q.text.clone(),
                        by: q.by.clone(),
                        upvotes: q.upvotes.len() as u32,
                        answered: q.answered,
                    })
                    .collect();
                // Não respondidas primeiro, depois por votos.
                qs.sort_by(|a, b| a.answered.cmp(&b.answered).then(b.upvotes.cmp(&a.upvotes)));
                qs
            })
            .unwrap_or_default()
    }

    /// Fecha uma sondagem (só anfitrião) e devolve o snapshot final — usado
    /// pela camada async para persistir o resultado no meeting da sala.
    pub(crate) fn close_poll(&self, room_id: Uuid, peer_id: Uuid, poll: Uuid) -> Option<PollState> {
        if !self.is_host(room_id, peer_id) {
            return None;
        }
        let snap = self.rooms.get_mut(&room_id).and_then(|mut r| {
            r.polls.iter_mut().find(|p| p.id == poll).map(|p| {
                p.open = false;
                p.clone()
            })
        });
        if snap.is_some() {
            self.broadcast_polls(room_id);
        }
        snap
    }

    pub(crate) fn broadcast_polls(&self, room_id: Uuid) {
        let msg = ServerMsg::Polls {
            polls: self.polls_view(room_id),
        };
        self.broadcast_all(room_id, msg);
    }

    pub(crate) fn broadcast_qa(&self, room_id: Uuid) {
        let msg = ServerMsg::Qa {
            questions: self.qa_view(room_id),
        };
        self.broadcast_all(room_id, msg);
    }

    /// Estado das ferramentas (para quem entra a meio).
    pub fn tools_snapshot(&self, room_id: Uuid) -> (Vec<PollView>, Vec<QaView>, Option<i64>) {
        let timer = self.rooms.get(&room_id).and_then(|r| r.timer_ends_at);
        (self.polls_view(room_id), self.qa_view(room_id), timer)
    }

    /// Traços atuais do quadro branco (para quem entra a meio).
    pub fn wb_snapshot(&self, room_id: Uuid) -> Vec<WbStrokeData> {
        self.rooms
            .get(&room_id)
            .map(|r| r.wb_strokes.clone())
            .unwrap_or_default()
    }

    /// Definições atuais (para enviar a quem acabou de entrar).
    pub fn settings_of(&self, room_id: Uuid) -> (bool, bool) {
        self.rooms
            .get(&room_id)
            .map(|r| (r.locked, r.host_share_only))
            .unwrap_or((false, false))
    }

    /// O peer tem autorização do anfitrião para partilhar o ecrã?
    pub(crate) fn share_allowed(&self, room_id: Uuid, peer_id: Uuid) -> bool {
        self.rooms
            .get(&room_id)
            .map(|r| r.share_grants.contains(&peer_id))
            .unwrap_or(false)
    }

    /// Regista quem está a apresentar (para controlo remoto e wb-open).
    pub(crate) fn set_presenter(&self, room_id: Uuid, peer_id: Uuid, on: bool) {
        if let Some(mut room) = self.rooms.get_mut(&room_id) {
            if on {
                room.presenter = Some(peer_id);
            } else if room.presenter == Some(peer_id) {
                room.presenter = None;
            }
        }
    }

    pub(crate) fn is_host(&self, room_id: Uuid, peer_id: Uuid) -> bool {
        self.rooms
            .get(&room_id)
            .and_then(|room| room.peers.get(&peer_id).map(|p| p.is_host))
            .unwrap_or(false)
    }

    pub fn room_size(&self, room_id: Uuid) -> usize {
        self.rooms.get(&room_id).map(|r| r.peers.len()).unwrap_or(0)
    }

    /// Routes one client message. Returns `false` when the peer asked to leave.
    pub fn handle(
        &self,
        room_id: Uuid,
        peer_id: Uuid,
        msg: ClientMsg,
        bus: Option<&std::sync::Arc<crate::pubsub::PubSubBus>>,
    ) -> bool {
        // Mensagens de quem já não está na sala (ex.: expulso) são ignoradas.
        let in_room = self.username_of(room_id, peer_id).is_some();
        match msg {
            ClientMsg::Leave => return false,
            _ if !in_room => return true,
            ClientMsg::Offer { to, sdp } => {
                self.send_to(room_id, to, ServerMsg::Offer { from: peer_id, sdp });
            }
            ClientMsg::Answer { to, sdp } => {
                self.send_to(room_id, to, ServerMsg::Answer { from: peer_id, sdp });
            }
            ClientMsg::Ice { to, candidate } => {
                self.send_to(
                    room_id,
                    to,
                    ServerMsg::Ice {
                        from: peer_id,
                        candidate,
                    },
                );
            }
            ClientMsg::RemoteControl {
                to,
                action,
                payload,
            } => {
                // Controlo remoto só existe sobre a tela partilhada: pedidos a
                // quem não está a apresentar são descartados (respostas passam).
                if action == "request" {
                    let target_presenting = self
                        .rooms
                        .get(&room_id)
                        .map(|r| r.presenter == Some(to))
                        .unwrap_or(false);
                    if !target_presenting {
                        return true;
                    }
                }
                self.send_to(
                    room_id,
                    to,
                    ServerMsg::RemoteControl {
                        from: peer_id,
                        action,
                        payload,
                    },
                );
            }
            ClientMsg::ShareGrant { to, allowed } => {
                if self.is_host(room_id, peer_id) {
                    if let Some(mut room) = self.rooms.get_mut(&room_id) {
                        if allowed {
                            room.share_grants.insert(to);
                        } else {
                            room.share_grants.remove(&to);
                        }
                    }
                    self.send_to(room_id, to, ServerMsg::ShareGranted { allowed });
                }
            }
            ClientMsg::ShareRequest => {
                // Com "só o anfitrião partilha", os pedidos são recusados logo.
                let blocked = self
                    .rooms
                    .get(&room_id)
                    .map(|r| r.host_share_only)
                    .unwrap_or(false);
                if blocked {
                    self.send_to(room_id, peer_id, ServerMsg::ShareGranted { allowed: false });
                } else {
                    let username = self
                        .username_of(room_id, peer_id)
                        .unwrap_or_else(|| "?".into());
                    self.broadcast_hosts(
                        room_id,
                        ServerMsg::ShareRequest {
                            from: peer_id,
                            username,
                        },
                    );
                }
            }
            ClientMsg::WbOpen => {
                // Só o apresentador atual (ou o anfitrião) abre o quadro a todos.
                let is_presenter = self
                    .rooms
                    .get(&room_id)
                    .map(|r| r.presenter == Some(peer_id))
                    .unwrap_or(false);
                if is_presenter || self.is_host(room_id, peer_id) {
                    let by = self
                        .username_of(room_id, peer_id)
                        .unwrap_or_else(|| "?".into());
                    self.broadcast(room_id, peer_id, ServerMsg::WbOpen { by });
                }
            }
            ClientMsg::Chat { text } => {
                if text.is_empty() || text.len() > 4000 {
                    return true;
                }
                let text = crate::dlp::censor(&text);
                let username = self
                    .username_of(room_id, peer_id)
                    .unwrap_or_else(|| "?".into());
                self.broadcast(
                    room_id,
                    peer_id,
                    ServerMsg::Chat {
                        from: peer_id,
                        username,
                        text,
                    },
                );
            }
            ClientMsg::Reaction { emoji } => {
                // Só emojis curtos — nada de spam de texto por aqui.
                if emoji.is_empty() || emoji.chars().count() > 4 {
                    return true;
                }
                let username = self
                    .username_of(room_id, peer_id)
                    .unwrap_or_else(|| "?".into());
                self.broadcast(
                    room_id,
                    peer_id,
                    ServerMsg::Reaction {
                        from: peer_id,
                        username,
                        emoji,
                    },
                );
            }
            ClientMsg::Hand { raised } => {
                if let Some(mut room) = self.rooms.get_mut(&room_id) {
                    if let Some(p) = room.peers.get_mut(&peer_id) {
                        p.hand = raised;
                    }
                }
                self.broadcast(
                    room_id,
                    peer_id,
                    ServerMsg::Hand {
                        from: peer_id,
                        raised,
                    },
                );
            }
            ClientMsg::Media { cam, mic } => {
                if let Some(mut room) = self.rooms.get_mut(&room_id) {
                    if let Some(p) = room.peers.get_mut(&peer_id) {
                        p.cam_on = cam;
                        p.mic_on = mic;
                    }
                }
                self.broadcast(
                    room_id,
                    peer_id,
                    ServerMsg::Media {
                        from: peer_id,
                        cam,
                        mic,
                    },
                );
            }
            ClientMsg::Recording { active } => {
                let username = self
                    .username_of(room_id, peer_id)
                    .unwrap_or_else(|| "?".into());
                self.broadcast(
                    room_id,
                    peer_id,
                    ServerMsg::Recording {
                        from: peer_id,
                        username,
                        active,
                    },
                );
            }
            ClientMsg::Transcript { text } => {
                // Frase final da fala do próprio microfone — difunde aos outros
                // para montarem a transcrição partilhada, legendada por orador.
                if text.is_empty() || text.len() > 4000 {
                    return true;
                }
                // Limpeza: PII (DLP) + máscara de palavrões antes de difundir.
                let text = crate::dlp::clean_caption(&text);
                let username = self
                    .username_of(room_id, peer_id)
                    .unwrap_or_else(|| "?".into());
                self.broadcast(
                    room_id,
                    peer_id,
                    ServerMsg::Transcript {
                        from: peer_id,
                        username,
                        text,
                    },
                );
            }
            ClientMsg::TranscriptInterim { text } => {
                // Legenda ao vivo (parcial): difunde já para a legenda dos outros
                // acompanhar em tempo real, sem esperar pelo fim da frase. Não
                // persiste na ata (efémera; é substituída pela final).
                if text.is_empty() || text.len() > 4000 {
                    return true;
                }
                // Fan-out O(n²): cada cliente transcreve o PRÓPRIO microfone e
                // difunde, logo n emissores × n destinatários. A 4 msg/s por
                // cliente, uma sala de 30 gerava ~3500 msg/s a sair de um só
                // nó — cada uma a provocar um `setState` em todos os browsers.
                // Duas travagens: numa sala grande as parciais deixam de valer
                // o custo (a legenda FINAL chega na mesma), e um limite por
                // emissor impede um cliente rápido de dominar o canal.
                if self.peer_count(room_id) > INTERIM_MAX_ROOM {
                    return true;
                }
                if !self.allow_interim(room_id, peer_id) {
                    return true;
                }
                let text = crate::dlp::clean_caption(&text);
                let username = self
                    .username_of(room_id, peer_id)
                    .unwrap_or_else(|| "?".into());
                self.broadcast(
                    room_id,
                    peer_id,
                    ServerMsg::TranscriptInterim {
                        from: peer_id,
                        username,
                        text,
                    },
                );
            }
            ClientMsg::TranscriptionToggle { on } => {
                // Só o anfitrião controla a transcrição partilhada; difunde a
                // todos (incl. quem envia) para começarem/pararem de captar.
                if self.is_host(room_id, peer_id) {
                    let by = self
                        .username_of(room_id, peer_id)
                        .unwrap_or_else(|| "?".into());
                    self.broadcast_all(room_id, ServerMsg::Transcription { on, by });
                }
            }
            ClientMsg::RoomLock { locked } => {
                if self.is_host(room_id, peer_id) {
                    if let Some(mut room) = self.rooms.get_mut(&room_id) {
                        room.locked = locked;
                    }
                    self.broadcast_settings(room_id);
                }
            }
            ClientMsg::HostShareOnly { on } => {
                if self.is_host(room_id, peer_id) {
                    if let Some(mut room) = self.rooms.get_mut(&room_id) {
                        room.host_share_only = on;
                    }
                    self.broadcast_settings(room_id);
                }
            }
            // Ferramentas de colaboração (sondagens, Q&A, temporizador, quadro
            // branco) — contexto próprio, extraído para room_tools.rs (Arq #3).
            m @ (ClientMsg::PollCreate { .. }
            | ClientMsg::PollVote { .. }
            | ClientMsg::PollClose { .. }
            | ClientMsg::QaAsk { .. }
            | ClientMsg::QaUpvote { .. }
            | ClientMsg::QaAnswered { .. }
            | ClientMsg::TimerSet { .. }
            | ClientMsg::WbStroke { .. }
            | ClientMsg::WbClear
            | ClientMsg::WbClose
            | ClientMsg::TimerClear) => self.handle_tool_msg(room_id, peer_id, m, bus),
            ClientMsg::Admit { to } => self.decide_waiting(room_id, peer_id, to, true),
            ClientMsg::Deny { to } => self.decide_waiting(room_id, peer_id, to, false),
            ClientMsg::ForceMute { to } => {
                if self.is_host(room_id, peer_id) {
                    self.send_to(room_id, to, ServerMsg::ForceMuted);
                }
            }
            ClientMsg::Kick { to } => {
                if self.is_host(room_id, peer_id) && to != peer_id {
                    self.send_to(room_id, to, ServerMsg::Kicked);
                    self.leave(room_id, to);
                }
            }
            // SFU e breakouts são tratados na camada de transporte (async/DB).
            ClientMsg::SfuOffer { .. }
            | ClientMsg::SfuAnswer { .. }
            | ClientMsg::SfuIce { .. }
            | ClientMsg::BreakoutsCreate { .. }
            | ClientMsg::BreakoutRename { .. }
            | ClientMsg::BreakoutAdd
            | ClientMsg::BreakoutMoveUser { .. }
            | ClientMsg::ServerRecord { .. }
            | ClientMsg::ScreenShare { .. }
            | ClientMsg::VideoInterest { .. }
            | ClientMsg::BreakoutsClose => {}
        }
        true
    }
}

// ---------- WebSocket transport ----------

#[derive(Deserialize)]
pub struct WsQuery {
    token: String,
    /// Segredo para reclamar um lugar reservado (R91). Opcional: sem ele,
    /// entra-se de novo, como sempre se entrou.
    ///
    /// Viaja na query e NÃO num cabeçalho pela mesma razão do `token`: num
    /// WebSocket do browser não há como pôr cabeçalhos. É por isso que ele
    /// nunca é registado — a query desta rota não entra em log nenhum, tal
    /// como a chave RTMP do `/broadcast`.
    reconnect: Option<String>,
}

pub async fn ws_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    // Only short-lived, room-scoped tokens open a signaling socket.
    let claims = verify_jwt(&state.config.jwt_secret, &query.token, "room")?;
    let room_id = claims.room.ok_or(ApiError::Unauthorized)?;

    // Nó a drenar: recusa ENTRADAS NOVAS, mas só as de salas que ainda não
    // existem aqui. Quem já está numa sala deste pod tem de conseguir voltar a
    // ligar-se (uma quebra de rede a meio do drain), senão o drain acabava por
    // expulsar exactamente quem estava a tentar aguentar-se.
    //
    // O 503 é deliberado: o cliente sabe distinguir «este nó não serve agora»
    // de «não tens autorização», e volta a pedir — o balanceador já o manda
    // para outro pod.
    if state.draining.load(std::sync::atomic::Ordering::Relaxed) && !state.hub.tem_sala(room_id) {
        return Err(ApiError::ServiceUnavailable(
            "Este nó está a encerrar. A tentar noutro…".into(),
        ));
    }
    let username = claims.name.unwrap_or_else(|| "anonymous".into());
    let sfu_mode = claims.topo.as_deref() == Some("sfu");
    let is_host = claims.owner;
    let must_wait = claims.wait && !claims.owner;
    let user_id = claims.sub;
    let is_bot = claims.is_bot;
    // can_admit depends on role. Let's say host can admit by default.
    let can_admit = is_host;
    let reconnect = query.reconnect.clone();
    Ok(ws.on_upgrade(move |socket| {
        handle_socket(
            state, socket, room_id, user_id, username, sfu_mode, is_host, must_wait, can_admit,
            is_bot, reconnect,
        )
    }))
}

/// Cria uma sala-filha de grupo (herda topologia/E2EE da principal).
async fn breakout_new_child(
    state: &Arc<AppState>,
    owner: Uuid,
    parent_name: &str,
    topology: &str,
    e2ee: bool,
    label: &str,
) -> Option<BreakoutChild> {
    match crate::rooms::insert_room(
        &state.db,
        owner,
        &format!("{label} — {parent_name}"),
        topology,
        false,
        e2ee,
        "normal",
    )
    .await
    {
        Ok(r) => Some(BreakoutChild {
            id: r.id,
            code: r.code,
            label: label.to_string(),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "breakout insert_room failed");
            None
        }
    }
}

/// Estado atual dos grupos (com quem está em cada um) para os anfitriões
/// na sala principal — chamado após qualquer mudança.
fn broadcast_breakout_state(state: &Arc<AppState>, parent_id: Uuid) {
    let Some(set) = state.breakouts.get(&parent_id) else {
        state.hub.broadcast_all(
            parent_id,
            ServerMsg::BreakoutsCreated {
                rooms: vec![],
                ends_at: None,
            },
        );
        return;
    };
    let infos: Vec<BreakoutInfo> = set
        .children
        .iter()
        .map(|c| BreakoutInfo {
            code: c.code.clone(),
            label: c.label.clone(),
            people: state
                .hub
                .roster_named(c.id)
                .into_iter()
                .map(|(_, n, _)| n)
                .collect(),
        })
        .collect();
    let ends_at = set.ends_at;
    for (id, host) in state.hub.roster(parent_id) {
        if host {
            state.hub.send_to(
                parent_id,
                id,
                ServerMsg::BreakoutsCreated {
                    rooms: infos.clone(),
                    ends_at,
                },
            );
        }
    }
}

/// Se `room_id` for filha de um conjunto de grupos, devolve o id da sala-mãe.
fn breakout_parent_of(state: &Arc<AppState>, room_id: Uuid) -> Option<Uuid> {
    state
        .breakouts
        .iter()
        .find(|e| e.value().children.iter().any(|c| c.id == room_id))
        .map(|e| *e.key())
}

/// Anfitrião pediu salas de grupo: cria N salas, distribui os participantes
/// round-robin, avisa os anfitriões e (opcional) agenda o retorno automático.
async fn breakouts_create(
    state: &Arc<AppState>,
    room_id: Uuid,
    owner: Uuid,
    count: u32,
    minutes: Option<u32>,
) {
    let count = count.clamp(2, 8) as usize;
    let parent: Option<(String, String, bool, String, String)> =
        sqlx::query_as("SELECT code, topology, e2ee, name, format FROM rooms WHERE id = $1")
            .bind(room_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let Some((parent_code, topology, e2ee, name, format)) = parent else {
        return;
    };
    // Salas de grupo só em reuniões de formato 'training' (defesa no servidor —
    // a UI já esconde, mas um cliente manipulado não deve conseguir criá-las).
    if format != "training" {
        tracing::warn!(%room_id, "breakouts pedidos numa sala não-treino — ignorado");
        return;
    }

    let mut children: Vec<BreakoutChild> = Vec::new();
    for i in 1..=count {
        match breakout_new_child(state, owner, &name, &topology, e2ee, &format!("Grupo {i}")).await
        {
            Some(c) => children.push(c),
            None => return,
        }
    }

    let ends_at = minutes
        .filter(|m| *m > 0)
        .map(|m| chrono::Utc::now().timestamp() + (m.min(180) as i64) * 60);
    let nonce = Uuid::new_v4();

    // Distribuir os não-anfitriões pelas salas, à vez.
    let guests: Vec<Uuid> = state
        .hub
        .roster(room_id)
        .into_iter()
        .filter(|(_, host)| !host)
        .map(|(id, _)| id)
        .collect();
    for (idx, peer) in guests.iter().enumerate() {
        let c = &children[idx % children.len()];
        state.hub.send_to(
            room_id,
            *peer,
            ServerMsg::BreakoutMove {
                code: c.code.clone(),
                label: c.label.clone(),
                back: false,
                ends_at,
            },
        );
    }

    state.breakouts.insert(
        room_id,
        BreakoutSet {
            parent_code,
            children,
            ends_at,
            nonce,
        },
    );
    broadcast_breakout_state(state, room_id);

    // Temporizador: no fim, devolve todos à principal (se esta geração ainda existir).
    if let Some(deadline) = ends_at {
        let state = state.clone();
        tokio::spawn(async move {
            let wait = (deadline - chrono::Utc::now().timestamp()).max(0) as u64;
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            let still_current = state
                .breakouts
                .get(&room_id)
                .map(|s| s.nonce == nonce)
                .unwrap_or(false);
            if still_current {
                tracing::info!(%room_id, "breakout timer expired — returning everyone");
                breakouts_close(&state, room_id);
            }
        });
    }
    tracing::info!(%room_id, count, ?minutes, "breakout rooms created");
}

/// Renomeia um grupo existente.
fn breakout_rename(state: &Arc<AppState>, room_id: Uuid, code: &str, label: &str) {
    let label = label.trim();
    if label.is_empty() || label.len() > 60 {
        return;
    }
    if let Some(mut set) = state.breakouts.get_mut(&room_id) {
        if let Some(c) = set.children.iter_mut().find(|c| c.code == code) {
            c.label = label.to_string();
        }
    }
    broadcast_breakout_state(state, room_id);
}

/// Acrescenta mais um grupo ao conjunto ativo.
async fn breakout_add(state: &Arc<AppState>, room_id: Uuid, owner: Uuid) {
    let parent: Option<(String, String, bool, String)> =
        sqlx::query_as("SELECT code, topology, e2ee, name FROM rooms WHERE id = $1")
            .bind(room_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let Some((_, topology, e2ee, name)) = parent else {
        return;
    };
    let n = state
        .breakouts
        .get(&room_id)
        .map(|s| s.children.len())
        .unwrap_or(0);
    if n == 0 || n >= 12 {
        return;
    }
    if let Some(child) = breakout_new_child(
        state,
        owner,
        &name,
        &topology,
        e2ee,
        &format!("Grupo {}", n + 1),
    )
    .await
    {
        if let Some(mut set) = state.breakouts.get_mut(&room_id) {
            set.children.push(child);
        }
        broadcast_breakout_state(state, room_id);
    }
}

/// Move um participante (por username) para outro grupo ou de volta à
/// principal. Procura-o na principal e em todas as filhas.
fn breakout_move_user(state: &Arc<AppState>, room_id: Uuid, name: &str, target_code: &str) {
    let Some(set) = state.breakouts.get(&room_id) else {
        return;
    };
    let back = target_code == set.parent_code;
    let label = if back {
        "Sala principal".to_string()
    } else {
        match set.children.iter().find(|c| c.code == target_code) {
            Some(c) => c.label.clone(),
            None => return,
        }
    };
    let ends_at = set.ends_at;
    // Salas onde ele pode estar: principal + todas as filhas.
    let mut locations: Vec<Uuid> = vec![room_id];
    locations.extend(set.children.iter().map(|c| c.id));
    drop(set);
    for loc in locations {
        if let Some((peer, _, _)) = state
            .hub
            .roster_named(loc)
            .into_iter()
            .find(|(_, n, host)| n == name && !host)
        {
            state.hub.send_to(
                loc,
                peer,
                ServerMsg::BreakoutMove {
                    code: target_code.to_string(),
                    label,
                    back,
                    ends_at,
                },
            );
            return;
        }
    }
}

/// Fecha os grupos: devolve toda a gente à sala principal.
fn breakouts_close(state: &Arc<AppState>, room_id: Uuid) {
    if let Some((_, set)) = state.breakouts.remove(&room_id) {
        for c in &set.children {
            state.hub.broadcast_all(
                c.id,
                ServerMsg::BreakoutMove {
                    code: set.parent_code.clone(),
                    label: "Sala principal".into(),
                    back: true,
                    ends_at: None,
                },
            );
        }
        // Anfitriões na sala principal limpam a lista.
        state.hub.broadcast_all(
            room_id,
            ServerMsg::BreakoutsCreated {
                rooms: vec![],
                ends_at: None,
            },
        );
        tracing::info!(%room_id, "breakout rooms closed");
    }
}

/// Consome o socket até fechar (ou o cliente desistir) enquanto espera admissão.
async fn drain_while_waiting(stream: &mut SplitStream<WebSocket>) {
    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Close(_) => break,
            Message::Text(text) => {
                if matches!(
                    serde_json::from_str::<ClientMsg>(&text),
                    Ok(ClientMsg::Leave)
                ) {
                    break;
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_socket(
    state: Arc<AppState>,
    socket: WebSocket,
    room_id: Uuid,
    user_id: Uuid,
    username: String,
    sfu_mode: bool,
    is_host: bool,
    must_wait: bool,
    can_admit: bool,
    is_bot: bool,
    reconnect: Option<String>,
) {
    // Gauge de ligações /ws ativas (dec automático no fim do handler).
    let _ws_guard = crate::metrics::WsGuard::signaling(state.metrics.clone());

    // RECLAMAÇÃO DE LUGAR (R91). Se vier um segredo e ele bater com um lugar
    // reservado dentro da janela, herda-se o `peer_id` e o PAPEL — e o
    // convidado não volta a cair na sala de espera.
    //
    // O que NÃO se herda: nada de media. O socket é novo, a `RTCPeerConnection`
    // é nova, e a negociação faz-se do zero. Tentar reaproveitar o estado de
    // media seria reabrir o glare que o R13 fechou.
    let reclamado = reconnect.as_deref().and_then(|seg| {
        state
            .hub
            .reclaim(room_id, seg, state.config.reconnect_grace())
    });
    let (peer_id, is_host, can_admit, must_wait, username) = match reclamado {
        Some(lugar) => {
            tracing::info!(%room_id, peer_id = %lugar.peer_id, "seat reclaimed");
            state
                .metrics
                .seats_reclaimed_total
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // `must_wait` cai para `false`: já tinha sido admitido antes de
            // cair. Voltar a pô-lo na fila é o defeito que isto corrige.
            (
                lugar.peer_id,
                lugar.is_host,
                lugar.can_admit,
                false,
                lugar.username,
            )
        }
        None => (Uuid::new_v4(), is_host, can_admit, must_wait, username),
    };
    let (mut sink, mut stream) = socket.split();
    // Fila de saída LIMITADA (ver `PeerTx`): um consumidor lento passa a
    // custar o próprio socket em vez da memória do nó inteiro.
    let (tx, mut rx, shutdown) = PeerTx::new(state.config.ws_queue_cap, state.metrics.clone());

    // Outbound: hub -> websocket. Um Ping periódico mantém a ligação viva
    // (proxies fecham WebSockets ociosos): sem tráfego, o socket cairia e —
    // como não há renegociação sem sinalização — a partilha de ecrã, o quadro
    // branco e o estado de media deixariam de chegar aos participantes.
    let writer = tokio::spawn(async move {
        let mut keepalive = tokio::time::interval(std::time::Duration::from_secs(25));
        keepalive.tick().await; // consome o tick imediato
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    let Some(msg) = msg else { break };
                    let text = match serde_json::to_string(&msg) {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                _ = keepalive.tick() => {
                    if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Sala de espera: convidados aguardam a decisão do anfitrião.
    // Uma sala bloqueada em runtime força espera mesmo sem waiting_room.
    let must_wait = must_wait || (!is_host && state.hub.is_locked(room_id));
    if must_wait {
        let (admit_tx, admit_rx) = oneshot::channel::<bool>();
        state
            .hub
            .add_waiting(room_id, peer_id, username.clone(), admit_tx);
        let _ = tx.send(ServerMsg::Waiting);
        tracing::info!(%room_id, %peer_id, %username, "guest waiting for admission");

        let admitted = tokio::select! {
            decision = admit_rx => decision.unwrap_or(false),
            _ = drain_while_waiting(&mut stream) => {
                // Desistiu/desligou enquanto esperava.
                state.hub.remove_waiting(room_id, peer_id);
                writer.abort();
                return;
            }
        };
        if !admitted {
            let _ = tx.send(ServerMsg::Denied);
            tracing::info!(%room_id, %peer_id, "guest denied");
            // Dá tempo ao writer de entregar a mensagem antes de fechar.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            writer.abort();
            return;
        }
    }

    if let Some(bus) = &state.redis_bus {
        let (polls, qa, wb, timer, (locked, host_share)) = tokio::join!(
            crate::redis_state::poll_get_all(bus.conn.clone(), room_id),
            crate::redis_state::qa_get_all(bus.conn.clone(), room_id),
            crate::redis_state::wb_get_all(bus.conn.clone(), room_id),
            crate::redis_state::timer_get(bus.conn.clone(), room_id),
            crate::redis_state::settings_get(bus.conn.clone(), room_id)
        );
        state
            .hub
            .apply_redis_state(room_id, polls, qa, wb, timer, locked, host_share);
    }

    let (peers, reconnect_secret) = state.hub.join(
        room_id,
        peer_id,
        user_id,
        username.clone(),
        is_host,
        can_admit,
        is_bot,
        tx.clone(),
    );
    let _ = tx.send(ServerMsg::Joined {
        peer_id,
        peers,
        reconnect: Some(Secret::new(reconnect_secret)),
    });
    let (locked, host_share_only) = state.hub.settings_of(room_id);
    if locked || host_share_only {
        let _ = tx.send(ServerMsg::RoomSettings {
            locked,
            host_share_only,
        });
    }
    // Ferramentas: quem entra a meio recebe sondagens/Q&A/temporizador atuais.
    let (polls, questions, timer) = state.hub.tools_snapshot(room_id);
    if !polls.is_empty() {
        let _ = tx.send(ServerMsg::Polls { polls });
    }
    if !questions.is_empty() {
        let _ = tx.send(ServerMsg::Qa { questions });
    }
    if timer.is_some() {
        let _ = tx.send(ServerMsg::Timer { ends_at: timer });
    }
    if let Some(by) = state.sfu.recording_by(room_id).await {
        let _ = tx.send(ServerMsg::ServerRecording { active: true, by });
    }
    let wb = state.hub.wb_snapshot(room_id);
    if !wb.is_empty() {
        let _ = tx.send(ServerMsg::WbState { strokes: wb });
    }
    if sfu_mode {
        if let Err(e) = state.sfu.add_peer(room_id, peer_id, tx.clone()).await {
            tracing::error!(%room_id, %peer_id, error = %e, "sfu add_peer failed");
            let _ = tx.send(ServerMsg::Error {
                message: "sfu unavailable".into(),
            });
        }
    }
    tracing::info!(%room_id, %peer_id, %username, sfu = sfu_mode, host = is_host, "peer joined");
    // Entrou numa sala de grupo? Atualiza o mapa de grupos dos anfitriões.
    if let Some(parent) = breakout_parent_of(&state, room_id) {
        broadcast_breakout_state(&state, parent);
    }

    // Inbound: websocket -> hub. Rate-limit por socket = TOKEN BUCKET
    // (600 burst / 300 sustained) — ver regressão R6 e os testes em rate_limit.rs.
    // Uma janela fixa apertada cortava o anfitrião na rajada de ICE. NÃO voltar
    // a janela fixa: o bucket absorve a rajada legítima.
    let mut rl = crate::rate_limit::TokenBucket::new(600.0, 300.0);
    loop {
        // O laço também acorda por `shutdown`: é assim que um transbordo da
        // fila de saída (consumidor lento) termina a sessão de forma ordenada
        // — com a saída do laço a correr a limpeza normal do peer, em vez de
        // deixar um peer na sala com o caminho de saída morto.
        let msg = tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(m)) => m,
                _ => break,
            },
            _ = shutdown.notified() => {
                tracing::warn!(%room_id, %peer_id, "sessão terminada: fila de saída em transbordo");
                break;
            }
        };
        if !rl.allow() {
            tracing::warn!(%peer_id, "flood de mensagens WS (token bucket esgotado) — a desligar");
            break;
        }
        match msg {
            Message::Text(text) => match serde_json::from_str::<ClientMsg>(&text) {
                Ok(
                    msg @ (ClientMsg::SfuOffer { .. }
                    | ClientMsg::SfuAnswer { .. }
                    | ClientMsg::SfuIce { .. }),
                ) if sfu_mode => {
                    if let Err(e) = state.sfu.on_client_msg(room_id, peer_id, msg).await {
                        tracing::warn!(%room_id, %peer_id, error = %e, "sfu message failed");
                    }
                }
                Ok(ClientMsg::BreakoutsCreate { count, minutes }) if is_host => {
                    breakouts_create(&state, room_id, user_id, count, minutes).await;
                }
                Ok(ClientMsg::BreakoutRename { code, label }) if is_host => {
                    breakout_rename(&state, room_id, &code, &label);
                }
                Ok(ClientMsg::BreakoutAdd) if is_host => {
                    breakout_add(&state, room_id, user_id).await;
                }
                Ok(ClientMsg::BreakoutMoveUser { name, code }) if is_host => {
                    breakout_move_user(&state, room_id, &name, &code);
                }
                Ok(ClientMsg::BreakoutsClose) if is_host => {
                    breakouts_close(&state, room_id);
                }
                Ok(ClientMsg::VideoInterest { peers, quality }) if sfu_mode => {
                    // Limite defensivo: o cliente não define quantas
                    // subscrições o servidor mantém abertas.
                    let mut peers = peers;
                    peers.truncate(64);
                    state
                        .sfu
                        .set_video_interest(room_id, peer_id, peers, quality)
                        .await;
                }
                Ok(ClientMsg::ScreenShare { on }) if sfu_mode => {
                    // Não-anfitrião só partilha com autorização do anfitrião
                    // (share-grant) — validado AQUI, não confiado no cliente.
                    if on && !is_host && !state.hub.share_allowed(room_id, peer_id) {
                        state.hub.send_to(
                            room_id,
                            peer_id,
                            ServerMsg::Error {
                                message: "A partilha de ecrã requer autorização do anfitrião"
                                    .into(),
                            },
                        );
                        continue;
                    }
                    state.sfu.set_screen(room_id, peer_id, on).await;
                    state.hub.set_presenter(room_id, peer_id, on);
                    // Aviso fiável a todos: quem parou de apresentar limpa já a
                    // apresentação nos recetores (sem esperar por eventos de
                    // track). Ao ligar, ajuda a preparar o palco (#1/#2).
                    state.hub.broadcast(
                        room_id,
                        peer_id,
                        ServerMsg::Presenting { from: peer_id, on },
                    );
                }
                Ok(ClientMsg::ServerRecord { active, e2ee_key }) if is_host && sfu_mode => {
                    if active {
                        // Chave E2EE (se cedida): 32 bytes AES-256 em base64.
                        use base64::Engine as _;
                        let key = e2ee_key
                            .as_ref()
                            .map(|s| s.expose())
                            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
                            .filter(|k| k.len() == 32);
                        if state
                            .sfu
                            .start_recording(
                                room_id,
                                user_id,
                                &username,
                                key,
                                &state.config.recordings_dir,
                            )
                            .await
                        {
                            state.hub.broadcast_all(
                                room_id,
                                ServerMsg::ServerRecording {
                                    active: true,
                                    by: username.clone(),
                                },
                            );
                        }
                    } else if let Some(session) = state.sfu.stop_recording(room_id).await {
                        crate::recorder::finalize(state.clone(), room_id, session);
                        state.hub.broadcast_all(
                            room_id,
                            ServerMsg::ServerRecording {
                                active: false,
                                by: username.clone(),
                            },
                        );
                    }
                }
                Ok(ClientMsg::PollClose { poll }) => {
                    // Fecho + revelação via hub (valida anfitrião); o resultado
                    // final persiste no meeting da sala (jsonb `polls`) — as
                    // sondagens/quizzes ficam guardadas com a reunião.
                    if let Some(p) = state.hub.close_poll(room_id, peer_id, poll) {
                        let db = state.db.clone();
                        tokio::spawn(async move {
                            let code: Option<(String,)> =
                                sqlx::query_as("SELECT code FROM rooms WHERE id = $1")
                                    .bind(room_id)
                                    .fetch_optional(&db)
                                    .await
                                    .ok()
                                    .flatten();
                            let Some((code,)) = code else { return };
                            let mut counts = vec![0u32; p.options.len()];
                            for v in p.votes.values() {
                                if let Some(c) = counts.get_mut(*v) {
                                    *c += 1;
                                }
                            }
                            let right = p
                                .correct
                                .map(|c| p.votes.values().filter(|&&v| v == c).count())
                                .unwrap_or(0);
                            let entry = serde_json::json!([{
                                "question": p.question,
                                "options": p.options,
                                "counts": counts,
                                "correct": p.correct,
                                "total_votes": p.votes.len(),
                                "total_right": right,
                                "total_wrong": p.votes.len().saturating_sub(right),
                                "by": p.by,
                            }]);
                            let _ = sqlx::query(
                                "UPDATE meetings SET polls = polls || $1::jsonb
                                 WHERE room_code = $2",
                            )
                            .bind(entry)
                            .bind(&code)
                            .execute(&db)
                            .await;
                        });
                    }
                    if let Some(b) = state.redis_bus.as_ref() {
                        let b = b.clone();
                        tokio::spawn(async move {
                            crate::redis_state::poll_close(b.conn.clone(), room_id, poll).await;
                        });
                    }
                }
                Ok(client_msg) => {
                    if !state
                        .hub
                        .handle(room_id, peer_id, client_msg, state.redis_bus.as_ref())
                    {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(ServerMsg::Error {
                        message: "invalid message".into(),
                    });
                }
            },
            Message::Close(_) => break,
            _ => {}
        }
    }

    if sfu_mode {
        // Última pessoa a sair leva a gravação órfã para finalização.
        if let Some(session) = state.sfu.remove_peer(room_id, peer_id).await {
            tracing::info!(%room_id, "room empty — finalizing server recording");
            crate::recorder::finalize(state.clone(), room_id, session);
        }
    }
    // O lugar fica RESERVADO durante a janela de graça (R91) em vez de se
    // perder já. Quem sair de propósito não passa por aqui com reserva: o
    // cliente apaga o segredo antes de fechar, por isso a reserva expira sem
    // ninguém a reclamar e transforma-se numa saída normal.
    //
    // A varredura que a transforma em saída corre em `main.rs`; aqui só se
    // marca. Fazer o `leave` com um `sleep` neste ponto prenderia a tarefa do
    // socket durante a janela inteira, e uma sala com muita rotação acumularia
    // tarefas adormecidas sem tecto.
    if !state.hub.disconnect(room_id, peer_id) {
        state.hub.leave(room_id, peer_id);
    }
    if let Some(parent) = breakout_parent_of(&state, room_id) {
        broadcast_breakout_state(&state, parent);
    }
    writer.abort();
    tracing::info!(%room_id, %peer_id, "peer socket closed — seat reserved");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Capacidade generosa nos testes: nenhum teste deste módulo exercita o
    /// transbordo (esse tem os seus próprios, em `bounded_queue`).
    const TEST_CAP: usize = 512;

    fn peer() -> (Uuid, PeerTx, mpsc::Receiver<ServerMsg>) {
        let (tx, rx, _shutdown) =
            PeerTx::new(TEST_CAP, Arc::new(crate::metrics::Metrics::default()));
        (Uuid::new_v4(), tx, rx)
    }

    fn drain(rx: &mut mpsc::Receiver<ServerMsg>) {
        while rx.try_recv().is_ok() {}
    }

    // ---------------------------------------------------------------
    //  Formato do fio (o que o cliente manda tem de ser aceite)
    // ---------------------------------------------------------------
    //
    // Um `ClientMsg` que não desserializa é DESCARTADO EM SILÊNCIO pelo
    // handler — não há erro, não há log, a funcionalidade simplesmente não
    // acontece. É a classe de falha mais cara que há neste ficheiro, e a
    // única defesa é testar o formato exacto que o cliente escreve.

    #[test]
    fn o_segredo_nunca_aparece_num_debug() {
        // A armadilha concreta: o `ClientMsg` deriva `Debug` e a chave E2EE
        // cedida pelo anfitrião viaja lá dentro. Um `tracing::debug!(?msg)`
        // acrescentado por boas razões punha a chave AES-256 da sala no
        // ficheiro de log, em base64, pronta a ler.
        let chave = "c2VjcmV0by1xdWUtbmFvLXBvZGUtYXBhcmVjZXI=";
        let msg = ClientMsg::ServerRecord {
            active: true,
            e2ee_key: Some(Secret(chave.to_string())),
        };
        let s = format!("{msg:?}");
        assert!(!s.contains(chave), "a chave apareceu no Debug: {s}");
        assert!(
            s.contains("redigido"),
            "e tem de ficar claro que foi redigido"
        );
    }

    #[test]
    fn o_segredo_continua_a_desserializar_do_json_do_cliente() {
        // A redacção não pode partir o fio: o cliente manda a chave como uma
        // string simples e tem de continuar a ser aceite.
        let raw = r#"{"type":"server-record","active":true,"e2ee_key":"QUJD"}"#;
        let msg: ClientMsg = serde_json::from_str(raw).expect("o cliente escreve isto");
        match msg {
            ClientMsg::ServerRecord { active, e2ee_key } => {
                assert!(active);
                assert_eq!(e2ee_key.as_ref().map(|s| s.expose()), Some("QUJD"));
            }
            _ => panic!("variante errada"),
        }
    }

    #[test]
    fn gravar_sem_ceder_chave_continua_a_funcionar() {
        // Sala sem E2EE: o campo não vem, e isso não é um erro.
        let raw = r#"{"type":"server-record","active":true}"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        match msg {
            ClientMsg::ServerRecord { e2ee_key, .. } => assert!(e2ee_key.is_none()),
            _ => panic!("variante errada"),
        }
    }

    #[test]
    fn video_interest_aceita_a_sugestao_de_qualidade() {
        let id = Uuid::new_v4();
        let raw =
            format!(r#"{{"type":"video-interest","peers":["{id}"],"quality":{{"{id}":"q"}}}}"#);
        let msg: ClientMsg = serde_json::from_str(&raw).expect("o cliente escreve isto");
        match msg {
            ClientMsg::VideoInterest { peers, quality } => {
                assert_eq!(peers, vec![id]);
                let q = quality.expect("a sugestão tem de chegar");
                assert_eq!(q.get(&id).map(|s| s.as_str()), Some("q"));
            }
            _ => panic!("desserializou para a variante errada"),
        }
    }

    #[test]
    fn video_interest_sem_qualidade_continua_a_ser_aceite() {
        // Cliente com a app em cache antiga: tem de continuar a funcionar.
        let id = Uuid::new_v4();
        let raw = format!(r#"{{"type":"video-interest","peers":["{id}"]}}"#);
        let msg: ClientMsg = serde_json::from_str(&raw).expect("cliente antigo");
        match msg {
            ClientMsg::VideoInterest { quality, .. } => assert!(quality.is_none()),
            _ => panic!("variante errada"),
        }
    }

    // ---------------------------------------------------------------
    //  Filas de saída limitadas (Programa I §3.2 — backpressure)
    // ---------------------------------------------------------------

    fn tiny_queue(
        cap: usize,
    ) -> (
        PeerTx,
        mpsc::Receiver<ServerMsg>,
        Arc<tokio::sync::Notify>,
        Arc<crate::metrics::Metrics>,
    ) {
        let m = Arc::new(crate::metrics::Metrics::default());
        let (tx, rx, sd) = PeerTx::new(cap, m.clone());
        (tx, rx, sd, m)
    }

    fn ephemeral() -> ServerMsg {
        ServerMsg::Reaction {
            from: Uuid::new_v4(),
            username: "a".into(),
            emoji: "👍".into(),
        }
    }

    fn protocol() -> ServerMsg {
        ServerMsg::SfuOffer { sdp: "v=0".into() }
    }

    #[tokio::test]
    async fn full_queue_drops_ephemeral_and_keeps_the_socket() {
        let (tx, _rx, shutdown, m) = tiny_queue(4);
        for _ in 0..4 {
            assert!(tx.send(ephemeral()), "a fila ainda tinha espaço");
        }
        // Cheia: a reacção seguinte perde-se, mas o socket sobrevive — perder
        // um emoji não justifica derrubar a sessão de ninguém.
        assert!(!tx.send(ephemeral()));
        assert_eq!(m.ws_queue_dropped_total.load(Relaxed), 1);
        assert_eq!(m.ws_slow_consumer_kills_total.load(Relaxed), 0);

        // E ninguém pediu para fechar o socket.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), shutdown.notified())
                .await
                .is_err(),
            "descartar uma mensagem efémera não pode fechar o socket"
        );
    }

    #[tokio::test]
    async fn full_queue_closes_the_socket_on_a_protocol_message() {
        let (tx, _rx, shutdown, m) = tiny_queue(2);
        assert!(tx.send(protocol()));
        assert!(tx.send(protocol()));

        // Cheia com sinalização por entregar: entregar meio protocolo deixaria
        // o cliente a acreditar num estado que o servidor já não tem. Fecha-se.
        assert!(!tx.send(protocol()));
        assert_eq!(m.ws_slow_consumer_kills_total.load(Relaxed), 1);
        assert_eq!(m.ws_queue_dropped_total.load(Relaxed), 0);

        tokio::time::timeout(std::time::Duration::from_millis(200), shutdown.notified())
            .await
            .expect("o laço do socket tinha de ser acordado para terminar");
    }

    #[tokio::test]
    async fn a_stalled_socket_is_only_killed_once() {
        let (tx, _rx, _shutdown, m) = tiny_queue(1);
        assert!(tx.send(protocol()));
        // A sala continua a difundir para um peer que já está a ser fechado.
        for _ in 0..20 {
            assert!(!tx.send(protocol()));
        }
        assert_eq!(
            m.ws_slow_consumer_kills_total.load(Relaxed),
            1,
            "um socket em transbordo é UMA morte, não uma por difusão"
        );
    }

    #[tokio::test]
    async fn draining_the_queue_lets_delivery_resume() {
        let (tx, mut rx, _shutdown, _m) = tiny_queue(2);
        assert!(tx.send(protocol()));
        assert!(tx.send(protocol()));
        assert!(!tx.send(protocol()));
        // O cliente recupera (a rede voltou) e o writer drena.
        drain(&mut rx);
        assert!(tx.send(protocol()), "fila drenada volta a aceitar");
    }

    #[tokio::test]
    async fn high_water_mark_tracks_the_worst_backlog() {
        let (tx, mut rx, _shutdown, m) = tiny_queue(8);
        for _ in 0..5 {
            tx.send(protocol());
        }
        assert_eq!(m.ws_queue_high_water.load(Relaxed), 5);
        // A marca de água é máxima histórica: drenar não a baixa.
        drain(&mut rx);
        tx.send(protocol());
        assert_eq!(m.ws_queue_high_water.load(Relaxed), 5);
    }

    #[test]
    fn only_ephemeral_messages_are_droppable() {
        // O que se pode perder: substituído pela versão final, ou efémero.
        assert!(ephemeral().is_droppable());
        assert!(ServerMsg::TranscriptInterim {
            from: Uuid::new_v4(),
            username: "a".into(),
            text: "par".into()
        }
        .is_droppable());

        // O que NUNCA se pode perder: protocolo e estado autoritativo.
        assert!(!protocol().is_droppable());
        assert!(!ServerMsg::Kicked.is_droppable());
        assert!(!ServerMsg::Denied.is_droppable());
        assert!(!ServerMsg::RoomSettings {
            locked: true,
            host_share_only: false
        }
        .is_droppable());
        assert!(!ServerMsg::PeerLeft {
            peer_id: Uuid::new_v4()
        }
        .is_droppable());
        // A legenda FINAL alimenta a ata — não é descartável, ao contrário da parcial.
        assert!(!ServerMsg::Transcript {
            from: Uuid::new_v4(),
            username: "a".into(),
            text: "final".into()
        }
        .is_droppable());
    }

    #[tokio::test]
    async fn join_announces_and_returns_roster() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, mut rx_a) = peer();
        let (b, tx_b, _rx_b) = peer();

        let (roster_a, _) = hub.join(room, a, a, "alice".into(), true, true, false, tx_a);
        assert!(roster_a.is_empty());

        // O anúncio de entrada vai para TODA a sala, incluindo quem entra — ver
        // `joiner_also_receives_its_own_announcement`. A primeira mensagem da
        // Alice é a dela própria, e é preciso descartá-la antes de esperar pela
        // do Bob. Foi esta mudança de semântica que deixou este teste (e mais
        // três) marcados como `#[ignore]`, a não proteger nada.
        drain(&mut rx_a);

        let (roster_b, _) = hub.join(room, b, b, "bob".into(), false, false, false, tx_b);
        assert_eq!(roster_b.len(), 1);
        assert_eq!(roster_b[0].peer_id, a);
        assert_eq!(roster_b[0].username, "alice");
        assert!(roster_b[0].host, "alice é anfitriã");

        match rx_a.recv().await.unwrap() {
            ServerMsg::PeerJoined { peer } => {
                assert_eq!(peer.peer_id, b);
                assert!(!peer.host);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// O anúncio de entrada vai para TODA a sala — incluindo quem entra.
    ///
    /// Não é uma opinião sobre o desenho: é o que `broadcast_all_local` faz, e
    /// os quatro testes que estiveram anos marcados como `#[ignore]` assumiam o
    /// contrário. Ficar escrito impede que a próxima pessoa a lê-los conclua
    /// que o hub está partido — e obriga quem mudar isto a mudar aqui também.
    #[tokio::test]
    async fn joiner_also_receives_its_own_announcement() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, mut rx_a) = peer();
        hub.join(room, a, a, "alice".into(), false, false, false, tx_a);

        match rx_a.recv().await.unwrap() {
            ServerMsg::PeerJoined { peer } => assert_eq!(
                peer.peer_id, a,
                "a primeira mensagem de quem entra é o anúncio DELE PRÓPRIO"
            ),
            other => panic!("esperava o próprio anúncio, veio: {other:?}"),
        }
    }

    #[tokio::test]
    async fn offer_is_routed_to_target_only() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, _rx_a) = peer();
        let (b, tx_b, mut rx_b) = peer();
        let (c, tx_c, mut rx_c) = peer();
        hub.join(room, a, a, "a".into(), false, false, false, tx_a);
        hub.join(room, b, b, "b".into(), false, false, false, tx_b);
        hub.join(room, c, c, "c".into(), false, false, false, tx_c);
        drain(&mut rx_b);
        drain(&mut rx_c);

        hub.handle(
            room,
            a,
            ClientMsg::Offer {
                to: b,
                sdp: "sdp-offer".into(),
            },
            None,
        );

        match rx_b.recv().await.unwrap() {
            ServerMsg::Offer { from, sdp } => {
                assert_eq!(from, a);
                assert_eq!(sdp, "sdp-offer");
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(
            rx_c.try_recv().is_err(),
            "c must not receive a targeted offer"
        );
    }

    #[tokio::test]
    async fn chat_broadcasts_to_everyone_else() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, mut rx_a) = peer();
        let (b, tx_b, mut rx_b) = peer();
        hub.join(room, a, a, "alice".into(), false, false, false, tx_a);
        hub.join(room, b, b, "bob".into(), false, false, false, tx_b);
        drain(&mut rx_a);

        hub.handle(
            room,
            b,
            ClientMsg::Chat {
                text: "olá!".into(),
            },
            None,
        );

        match rx_a.recv().await.unwrap() {
            ServerMsg::Chat {
                from,
                username,
                text,
            } => {
                assert_eq!(from, b);
                assert_eq!(username, "bob");
                assert_eq!(text, "olá!");
            }
            other => panic!("unexpected: {other:?}"),
        }
        // `rx_b` ainda tem o anúncio de entrada do próprio b — descarta-se antes
        // de afirmar que o chat não volta ao remetente, senão o que se apanha é
        // o `PeerJoined` e não um eco.
        drain(&mut rx_b);
        assert!(
            rx_b.try_recv().is_err(),
            "sender must not echo its own chat"
        );
    }

    #[tokio::test]
    async fn leave_notifies_and_cleans_empty_rooms() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, mut rx_a) = peer();
        let (b, tx_b, _rx_b) = peer();
        hub.join(room, a, a, "a".into(), false, false, false, tx_a);
        hub.join(room, b, b, "b".into(), false, false, false, tx_b);
        drain(&mut rx_a);

        hub.leave(room, b);
        match rx_a.recv().await.unwrap() {
            ServerMsg::PeerLeft { peer_id } => assert_eq!(peer_id, b),
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(hub.room_size(room), 1);

        hub.leave(room, a);
        assert_eq!(hub.room_size(room), 0);
        assert!(
            hub.rooms.is_empty(),
            "empty rooms must be garbage-collected"
        );
    }

    #[tokio::test]
    async fn rooms_are_isolated() {
        let hub = SignalingHub::default();
        let room1 = Uuid::new_v4();
        let room2 = Uuid::new_v4();
        let (a, tx_a, _rx_a) = peer();
        let (b, tx_b, mut rx_b) = peer();
        hub.join(room1, a, a, "a".into(), false, false, false, tx_a);
        hub.join(room2, b, b, "b".into(), false, false, false, tx_b);

        hub.handle(
            room1,
            a,
            ClientMsg::Chat {
                text: "room1 only".into(),
            },
            None,
        );
        // Sem descartar o anúncio de entrada do próprio b, o que este teste
        // apanhava era essa mensagem — e acusava uma fuga entre salas que não
        // existe. O invariante a provar é que o chat da sala 1 não chega à 2.
        drain(&mut rx_b);
        hub.handle(
            room1,
            a,
            ClientMsg::Chat {
                text: "outra vez só na sala 1".into(),
            },
            None,
        );
        assert!(rx_b.try_recv().is_err(), "messages must never cross rooms");
        assert!(!hub.send_to(room1, b, ServerMsg::PeerLeft { peer_id: a }));
    }

    #[tokio::test]
    async fn waiting_guest_is_admitted_by_host_only() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (host, tx_h, mut rx_h) = peer();
        let (other, tx_o, _rx_o) = peer();
        hub.join(room, host, host, "host".into(), true, true, false, tx_h);
        hub.join(
            room,
            other,
            other,
            "other".into(),
            false,
            false,
            false,
            tx_o,
        );
        drain(&mut rx_h);

        let guest = Uuid::new_v4();
        let (admit_tx, mut admit_rx) = oneshot::channel();
        hub.add_waiting(room, guest, "guest".into(), admit_tx);

        // Anfitrião é notificado.
        match rx_h.recv().await.unwrap() {
            ServerMsg::WaitingJoin { peer } => assert_eq!(peer.peer_id, guest),
            other => panic!("unexpected: {other:?}"),
        }

        // Um não-anfitrião a tentar admitir é ignorado.
        hub.handle(room, other, ClientMsg::Admit { to: guest }, None);
        assert!(admit_rx.try_recv().is_err(), "non-host must not admit");

        // O anfitrião admite.
        hub.handle(room, host, ClientMsg::Admit { to: guest }, None);
        assert_eq!(admit_rx.await, Ok(true));
    }

    #[tokio::test]
    async fn waiting_guest_can_be_denied() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (host, tx_h, mut rx_h) = peer();
        hub.join(room, host, host, "host".into(), true, true, false, tx_h);
        drain(&mut rx_h);

        let guest = Uuid::new_v4();
        let (admit_tx, admit_rx) = oneshot::channel();
        hub.add_waiting(room, guest, "guest".into(), admit_tx);

        hub.handle(room, host, ClientMsg::Deny { to: guest }, None);
        assert_eq!(admit_rx.await, Ok(false));
    }

    #[tokio::test]
    async fn host_joining_later_sees_waiting_queue() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let guest = Uuid::new_v4();
        let (admit_tx, _admit_rx) = oneshot::channel();
        hub.add_waiting(room, guest, "guest".into(), admit_tx);

        let (host, tx_h, mut rx_h) = peer();
        hub.join(room, host, host, "host".into(), true, true, false, tx_h);
        // O `join` entrega DUAS mensagens ao anfitrião: o anúncio de entrada
        // (que chega também a quem entra) e, a seguir, a fila de espera já
        // acumulada. Um `recv()` único apanha o anúncio e conclui, erradamente,
        // que o anfitrião não vê quem estava à espera; um `drain` seguido de
        // `recv()` consome as duas e fica pendurado. Recolhe-se o que há e
        // afirma-se sobre o conjunto.
        let mut vistas = Vec::new();
        while let Ok(m) = rx_h.try_recv() {
            vistas.push(m);
        }
        assert!(
            vistas.iter().any(|m| matches!(
                m,
                ServerMsg::WaitingJoin { peer } if peer.peer_id == guest
            )),
            "o anfitrião que entra DEPOIS tem de ver quem já esperava; recebeu: {vistas:?}"
        );
    }

    #[tokio::test]
    async fn reaction_and_hand_are_broadcast() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, mut rx_a) = peer();
        let (b, tx_b, _rx_b) = peer();
        hub.join(room, a, a, "alice".into(), false, false, false, tx_a);
        hub.join(room, b, b, "bob".into(), false, false, false, tx_b);
        drain(&mut rx_a);

        hub.handle(
            room,
            b,
            ClientMsg::Reaction {
                emoji: "🎉".into()
            },
            None,
        );
        match rx_a.recv().await.unwrap() {
            ServerMsg::Reaction {
                username, emoji, ..
            } => {
                assert_eq!(username, "bob");
                assert_eq!(emoji, "🎉");
            }
            other => panic!("unexpected: {other:?}"),
        }

        hub.handle(room, b, ClientMsg::Hand { raised: true }, None);
        match rx_a.recv().await.unwrap() {
            ServerMsg::Hand { from, raised } => {
                assert_eq!(from, b);
                assert!(raised);
            }
            other => panic!("unexpected: {other:?}"),
        }
        // Estado da mão fica no roster para quem entrar depois.
        let (c, tx_c, _rx_c) = peer();
        let (roster, _) = hub.join(room, c, c, "carol".into(), false, false, false, tx_c);
        let bob = roster.iter().find(|p| p.peer_id == b).unwrap();
        assert!(bob.hand);
    }

    #[tokio::test]
    async fn host_can_mute_and_kick_but_others_cannot() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (host, tx_h, _rx_h) = peer();
        let (a, tx_a, mut rx_a) = peer();
        let (b, tx_b, mut rx_b) = peer();
        hub.join(room, host, host, "host".into(), true, true, false, tx_h);
        hub.join(room, a, a, "a".into(), false, false, false, tx_a);
        hub.join(room, b, b, "b".into(), false, false, false, tx_b);
        drain(&mut rx_a);
        drain(&mut rx_b);

        // Não-anfitrião não silencia ninguém.
        hub.handle(room, a, ClientMsg::ForceMute { to: b }, None);
        assert!(rx_b.try_recv().is_err());

        hub.handle(room, host, ClientMsg::ForceMute { to: b }, None);
        assert!(matches!(rx_b.recv().await.unwrap(), ServerMsg::ForceMuted));

        hub.handle(room, host, ClientMsg::Kick { to: b }, None);
        assert!(matches!(rx_b.recv().await.unwrap(), ServerMsg::Kicked));
        assert_eq!(hub.room_size(room), 2, "expulso sai da sala");
        // O kick difunde peer-left aos restantes.
        assert!(matches!(
            rx_a.recv().await.unwrap(),
            ServerMsg::PeerLeft { .. }
        ));
        // Mensagens do expulso passam a ser ignoradas.
        hub.handle(
            room,
            b,
            ClientMsg::Chat {
                text: "ainda cá estou?".into(),
            },
            None,
        );
        assert!(rx_a.try_recv().is_err());
    }

    // ---------- Reclamação de lugar (R91) ----------

    /// O caso que motivou tudo: um `F5` a meio da reunião.
    ///
    /// Antes, o socket caía, o `peer_id` morria com ele, e o anfitrião voltava a
    /// entrar como participante comum — ou, se fosse convidado, ia outra vez para a
    /// sala de espera. Estas quatro asserções são o produto, não a biblioteca.
    #[tokio::test]
    async fn um_lugar_reservado_devolve_o_papel_a_quem_volta() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, _rx_a) = peer();
        let (b, tx_b, mut rx_b) = peer();
        let (_, segredo) = hub.join(room, a, a, "anfitriã".into(), true, true, false, tx_a);
        hub.join(room, b, b, "b".into(), false, false, false, tx_b);
        drain(&mut rx_b);

        assert!(hub.disconnect(room, a), "o lugar tem de ficar reservado");
        match rx_b.recv().await.unwrap() {
            ServerMsg::PeerReconnecting { peer_id } => assert_eq!(peer_id, a),
            other => panic!("os outros têm de ver «a voltar», não uma saída: {other:?}"),
        }

        let lugar = hub
            .reclaim(room, &segredo, std::time::Duration::from_secs(45))
            .expect("o segredo certo dentro da janela tem de reclamar");
        assert_eq!(lugar.peer_id, a, "o mesmo lugar, não um novo");
        assert!(
            lugar.is_host,
            "o papel de anfitriã não se perde numa quebra"
        );
        assert!(lugar.can_admit, "nem a autorização de admitir");
        assert_eq!(lugar.username, "anfitriã");
    }

    /// Um segredo errado não entra no lugar de ninguém. Sem esta recusa, quem
    /// conhecesse o link da sala herdava o papel de anfitrião do primeiro que
    /// caísse — que é exactamente a promoção por conhecer um link que o segredo
    /// existe para impedir.
    #[tokio::test]
    async fn segredo_errado_ou_vazio_nao_reclama_nada() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, _rx) = peer();
        let (_, segredo) = hub.join(room, a, a, "a".into(), true, true, false, tx_a);
        hub.disconnect(room, a);

        assert!(
            hub.reclaim(room, "", std::time::Duration::from_secs(45))
                .is_none(),
            "vazio"
        );
        assert!(
            hub.reclaim(room, "nao-e-o-segredo", std::time::Duration::from_secs(45))
                .is_none(),
            "errado"
        );
        // E o certo continua a servir depois das tentativas falhadas: uma recusa
        // não pode consumir o lugar.
        assert!(hub
            .reclaim(room, &segredo, std::time::Duration::from_secs(45))
            .is_some());
    }

    /// O segredo é de UMA sala. Sem isto, o mesmo segredo abriria um lugar noutra
    /// reunião — que é atravessar a fronteira de sala, a invariante nº 1.
    #[tokio::test]
    async fn um_segredo_nao_serve_noutra_sala() {
        let hub = SignalingHub::default();
        let (r1, r2) = (Uuid::new_v4(), Uuid::new_v4());
        let (a, tx_a, _rx_a) = peer();
        let (b, tx_b, _rx_b) = peer();
        let (_, seg1) = hub.join(r1, a, a, "a".into(), true, true, false, tx_a);
        hub.join(r2, b, b, "b".into(), false, false, false, tx_b);
        hub.disconnect(r1, a);
        hub.disconnect(r2, b);

        assert!(
            hub.reclaim(r2, &seg1, std::time::Duration::from_secs(45))
                .is_none(),
            "o segredo da sala 1 não pode reclamar um lugar na sala 2"
        );
        assert!(hub
            .reclaim(r1, &seg1, std::time::Duration::from_secs(45))
            .is_some());
    }

    /// Quem está VIVO não é substituível. Um segredo copiado não pode expulsar o
    /// dono do lugar enquanto ele está ligado — só reclama quem caiu.
    #[tokio::test]
    async fn nao_se_reclama_o_lugar_de_quem_esta_ligado() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, _rx) = peer();
        let (_, segredo) = hub.join(room, a, a, "a".into(), true, true, false, tx_a);
        assert!(
            hub.reclaim(room, &segredo, std::time::Duration::from_secs(45))
                .is_none(),
            "sem `disconnect` não há lugar reservado para reclamar"
        );
    }

    /// Passada a janela, o lugar deixa de ser reclamável E sai da sala. As duas
    /// metades importam: se só deixasse de ser reclamável, o participante ficava no
    /// roster para sempre e a sala nunca esvaziava.
    #[tokio::test]
    async fn fora_da_janela_o_lugar_expira_e_sai() {
        let hub = SignalingHub::default();
        let room = Uuid::new_v4();
        let (a, tx_a, _rx_a) = peer();
        let (b, tx_b, mut rx_b) = peer();
        let (_, segredo) = hub.join(room, a, a, "a".into(), true, true, false, tx_a);
        hub.join(room, b, b, "b".into(), false, false, false, tx_b);
        hub.disconnect(room, a);
        drain(&mut rx_b);

        // Janela de zero: tudo o que caiu já passou dela.
        let nula = std::time::Duration::from_secs(0);
        assert!(
            hub.reclaim(room, &segredo, nula).is_none(),
            "fora da janela não se reclama"
        );
        assert_eq!(hub.expire_disconnected(nula), 1, "um lugar tem de expirar");
        match rx_b.recv().await.unwrap() {
            ServerMsg::PeerLeft { peer_id } => assert_eq!(peer_id, a),
            other => panic!("ao expirar, os outros veem uma SAÍDA: {other:?}"),
        }
        assert_eq!(
            hub.expire_disconnected(nula),
            0,
            "expirar duas vezes o mesmo lugar seria uma saída a dobrar"
        );
    }

    /// O segredo nunca aparece num `Debug`, que é onde os segredos costumam
    /// escapar (R43). Vale a pena a asserção: o campo é novo e o tipo é fácil de
    /// trocar por `String` num refactor distraído.
    #[test]
    fn o_segredo_de_reclamacao_nao_aparece_em_debug() {
        let s = Secret::new("segredo-muito-secreto".into());
        let texto = format!("{s:?}");
        assert!(!texto.contains("segredo-muito-secreto"), "veio: {texto}");
    }
}
