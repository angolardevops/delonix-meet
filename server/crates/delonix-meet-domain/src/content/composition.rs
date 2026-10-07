//! Regras da fila de **composição** de gravações. Puras: o adaptador Postgres
//! aplica-as numa só instrução com `FOR UPDATE SKIP LOCKED`, como a fila da
//! transcrição ([`super::transcription`]).
//!
//! PORQUE EXISTE: a composição era um `tokio::spawn` nu cujo directório de
//! segmentos só vivia na memória da tarefa. Um reinício do servidor — ou seja,
//! qualquer rollout — deixava a gravação em `processing` sem ninguém capaz de
//! a retomar, e setenta minutos depois um varredor marcava-a `failed`. O
//! manifesto ([`ComposeManifest`]) é o que faltava: com ele na linha, outro
//! processo (ou o mesmo, depois de arrancar) sabe exactamente o que compor.
//!
//! O manifesto NÃO leva a chave E2EE. Os segmentos no disco já estão em claro
//! — a chave cedida pelo anfitrião é usada nos *writers*, à entrada, e morre
//! com a sessão. Retomar não precisa dela e nada se guarda que não se
//! guardasse antes.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Uma gravação que falha a compor este número de vezes sai da fila. Um input
/// malformado que rebente o ffmpeg não pode ocupar uma vaga para sempre.
pub const MAX_ATTEMPTS: i32 = 3;

/// Prazo da reserva de uma composição. Tem de cobrir o pior caso do ffmpeg
/// (`FFMPEG_TIMEOUT_SECS`, 1 h por omissão) mais a espera por vaga, senão dois
/// pods compunham a mesma gravação ao mesmo tempo.
const MIN_LEASE: u64 = 5 * 60;
const MAX_LEASE: u64 = 6 * 3600;

/// Prazo efectivo: o tecto do ffmpeg mais a folga da fila, preso a 5 min..6 h.
/// A folga é generosa de propósito — uma reserva que expira cedo é pior do que
/// uma que expira tarde, porque duplica trabalho em vez de o atrasar.
pub fn lease_duration(ffmpeg_timeout_secs: u64) -> Duration {
    Duration::from_secs(ffmpeg_timeout_secs.saturating_add(900).clamp(MIN_LEASE, MAX_LEASE))
}

/// Depois de uma falha: a gravação volta à fila da composição?
///
/// `retryable` distingue a falha que pode passar sozinha (o processo morreu, o
/// ffmpeg foi morto pelo tecto, a base soluçou) da que é estável e não melhora
/// com insistência — e é esta a razão pela qual a quota e o «nada gravado» NÃO
/// se repetem: ver [`is_retryable`].
pub fn should_retry(retryable: bool, attempts_so_far: i32) -> bool {
    retryable && attempts_so_far < MAX_ATTEMPTS
}

/// A falha pode passar sozinha? Decide-se pela causa, não pelo texto do erro
/// visível à pessoa.
///
/// Estáveis (não se repetem): a quota da organização, a ausência do ffmpeg no
/// servidor, e «nada gravado» — repetir dava exactamente o mesmo resultado e
/// gastava as tentativas que servem para o caso que importa.
pub fn is_retryable(erro: &str) -> bool {
    const ESTAVEIS: [&str; 3] = ["storage.quota_exceeded", "ffmpeg-ausente", "nothing recorded"];
    !ESTAVEIS.iter().any(|e| erro.contains(e))
}

/// Uma pista no manifesto: o que a composição precisa de saber dela.
///
/// `starts_at_ms` é JÁ o valor resolvido (`RecTrackMeta::starts_at_ms`), não o
/// instante da ligação: o primeiro pacote de uma pista é um dado de memória da
/// sessão e tinha de ficar guardado para o `-itsoffset` dar o mesmo resultado
/// numa segunda tentativa.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposeTrack {
    pub path: PathBuf,
    /// `"video"` ou `"audio"` (o adaptador compara com `ends_with("audio")`).
    pub kind: String,
    pub starts_at_ms: u64,
}

/// O que basta para compor uma gravação sem a sessão em memória.
///
/// Guarda-se em `recordings.compose_manifest` na MESMA instrução que cria a
/// linha em `processing`: se o processo morrer um instante depois, o que ficou
/// na base já é suficiente para retomar.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposeManifest {
    /// O directório `tmp-<uuid>` com os segmentos. Retomar começa por
    /// confirmar que ainda existe — num pod novo com disco efémero não existe,
    /// e aí a gravação é uma perda honesta, não uma tentativa infinita.
    pub dir: PathBuf,
    pub tracks: Vec<ComposeTrack>,
    /// Duração de parede da sessão, em ms: é o denominador do progresso.
    pub expected_ms: i64,
    /// Duração a gravar na linha, em segundos.
    pub duration_secs: Option<i32>,
    /// Quem gravou — a quota conta-se a esta pessoa.
    pub by_user: uuid::Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reserva_cobre_o_tecto_do_ffmpeg() {
        // O caso que importa: com o tecto por omissão a reserva é MAIOR do que
        // ele, senão um segundo pod pegava na gravação a meio da primeira.
        assert!(lease_duration(3600) > Duration::from_secs(3600));
        // Presa em baixo e em cima.
        assert_eq!(lease_duration(0), Duration::from_secs(900));
        assert_eq!(lease_duration(30), Duration::from_secs(930));
        assert_eq!(lease_duration(u64::MAX), Duration::from_secs(MAX_LEASE));
    }

    #[test]
    fn falhas_estaveis_nao_se_repetem() {
        assert!(!is_retryable("storage.quota_exceeded: a gravação não cabe"));
        assert!(!is_retryable("ffmpeg-ausente"));
        assert!(!is_retryable("nothing recorded"));
        // As que podem passar sozinhas.
        assert!(is_retryable("ffmpeg exited with signal: 9"));
        assert!(is_retryable("o processamento foi interrompido"));
    }

    #[test]
    fn as_tentativas_tem_tecto() {
        assert!(should_retry(true, 0));
        assert!(should_retry(true, MAX_ATTEMPTS - 1));
        assert!(!should_retry(true, MAX_ATTEMPTS));
        assert!(!should_retry(true, MAX_ATTEMPTS + 7));
        // Estável nunca volta, mesmo na primeira.
        assert!(!should_retry(false, 0));
    }

    #[test]
    fn o_manifesto_atravessa_json_igual() {
        let m = ComposeManifest {
            dir: PathBuf::from("/var/rec/tmp-abc"),
            tracks: vec![ComposeTrack {
                path: PathBuf::from("/var/rec/tmp-abc/0.ivf"),
                kind: "video".into(),
                starts_at_ms: 1234,
            }],
            expected_ms: 60_000,
            duration_secs: Some(60),
            by_user: uuid::Uuid::nil(),
        };
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<ComposeManifest>(&json).unwrap(), m);
    }
}
