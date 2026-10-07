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

/// Prazo da reserva, **renovado pelo batimento** enquanto a composição vive.
///
/// A primeira versão disto punha a reserva a cobrir o pior caso do ffmpeg
/// (`FFMPEG_TIMEOUT_SECS` + folga = 75 min) — e isso estragava precisamente o
/// caso que a retoma existe para salvar: num rollout, o pod morria com uma
/// reserva de 75 minutos na mão, e o pod novo tinha de esperar esse tempo
/// inteiro antes de poder reivindicar. Trocar setenta minutos de espera por
/// setenta e cinco não é um remédio.
///
/// A reserva é por isso CURTA e renovada por quem a detém: enquanto a tarefa
/// vive, o batimento ([`RENEW_EVERY`]) empurra-a para a frente; quando o
/// processo morre, ninguém a renova e o trabalho fica reivindicável em três
/// minutos. É o mecanismo que o repo já tinha no `progress_at` — passa a valer
/// também para a posse.
pub const LEASE: Duration = Duration::from_secs(180);

/// De quanto em quanto tempo quem compõe renova a reserva. Seis vezes dentro
/// da [`LEASE`]: um soluço da base (ou um ffmpeg calado) não custa a posse.
///
/// Renova-se por RELÓGIO e não por progresso do ffmpeg: um ffmpeg que estanca
/// deixa de reportar, e se a renovação dependesse disso a reserva caía com o
/// processo ainda vivo — dois ffmpeg a escrever o mesmo `out.webm`.
pub const RENEW_EVERY: Duration = Duration::from_secs(30);

/// Idade a partir da qual um directório `tmp-*` sem dono se pode apagar.
///
/// Generosa de propósito, e sem relação com a [`LEASE`]: um directório pode
/// pertencer a uma composição que acabou de arrancar e cuja linha ainda não foi
/// escrita, ou a uma que está a correr há quase o tecto inteiro do ffmpeg.
/// Apagar cedo é perder uma gravação; apagar tarde é ocupar disco mais um dia.
pub fn orphan_grace(ffmpeg_timeout_secs: u64) -> Duration {
    Duration::from_secs(ffmpeg_timeout_secs.saturating_add(3600).clamp(3600, 24 * 3600))
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
    fn a_reserva_e_curta_e_renova_se_com_folga() {
        // O defeito que isto guarda: uma reserva longa fazia o pod novo esperar
        // mais do que a gravação já tinha esperado. Tem de ser minutos, não
        // horas, e muito menor do que o tecto do ffmpeg.
        assert!(LEASE <= Duration::from_secs(300), "a reserva voltou a ser longa");
        assert!(LEASE < Duration::from_secs(3600));
        // E a renovação tem de caber várias vezes dentro dela: uma só
        // oportunidade de renovar perdia a posse ao primeiro soluço.
        assert!(
            LEASE.as_secs() / RENEW_EVERY.as_secs() >= 4,
            "a renovação não tem folga dentro da reserva"
        );
    }

    #[test]
    fn a_folga_dos_orfaos_cobre_o_tecto_do_ffmpeg() {
        // Ao contrário da reserva, esta TEM de ser maior do que o tecto: um
        // directório de uma composição a correr não se apaga.
        assert!(orphan_grace(3600) > Duration::from_secs(3600));
        assert_eq!(orphan_grace(0), Duration::from_secs(3600));
        assert_eq!(orphan_grace(u64::MAX), Duration::from_secs(24 * 3600));
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
