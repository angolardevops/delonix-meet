//! O registo dos ficheiros derivados de uma gravação (migração 0126).
//!
//! # Porque existe
//!
//! Uma gravação deixa no `recordings_dir` cinco coisas — a composta, a
//! miniatura, a pasta das pistas ISO, e por cada recorte um `.webm` e um
//! `.vtt`. Antes desta tabela, nenhuma estava registada em sítio nenhum e
//! **cada caminho que apaga tinha de se lembrar de todas, uma a uma**: a
//! retenção fazia três `let _ = remove_file(...)` à mão, e o derivado seguinte
//! era esquecido outra vez. Foi medido assim duas vezes (2026-10-18, commit
//! `ece92e6a4`): a pasta `iso-<gravação>` que um processo morto deixava a meio
//! do registo não era recolhida por ninguém, e os `clip-<id>.webm`/`.vtt`
//! sobreviviam à gravação porque o `ON DELETE CASCADE` da 0118 levava as
//! LINHAS e fazia a fuga parecer fechada.
//!
//! # A regra, e é uma só
//!
//! **A linha nasce ANTES do ficheiro.** Uma linha sem ficheiro é inofensiva —
//! quem recolhe faz um `remove_file` que não encontra nada e segue. Um ficheiro
//! sem linha é a fuga: ninguém o consegue pedir e ninguém o apaga. Todo o
//! produtor escreve primeiro e só depois põe bytes no disco, e é por isso que a
//! janela de um processo morto deixa de existir em vez de encolher.
//!
//! # Varrer um registo não é varrer o disco
//!
//! A versão anterior desta frente inferia a propriedade de uma pasta a partir
//! do NOME e da sua idade, e foi revertida por três razões medidas (estão no
//! comentário do `recorder::sweep_orphan_segments`). A pior: «a base não
//! conhece este id» não é um erro, logo uma base restaurada para um ponto
//! anterior — ou outro ambiente apontado ao mesmo volume `ReadWriteMany` —
//! autorizava `remove_dir_all` em todas as pastas do volume.
//!
//! Aqui a autoridade está invertida, e é o que torna [`sweep_orphans`] seguro
//! onde a varredura do disco não era: **só se apaga um caminho que esta tabela
//! nomeia**. Um ficheiro que o registo não conhece nunca é tocado — é o próprio
//! registo que é a quarentena. No cenário da base restaurada, as linhas e as
//! gravações voltam juntas (estão as duas na base), logo não há órfão nenhum
//! para apagar; e um ambiente que partilhe o volume não tem nas SUAS linhas os
//! ficheiros do outro, logo não lhes chega.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;

use uuid::Uuid;

use crate::AppState;

/// A gravação composta, `<id>.webm`. Nasce com a linha de `recordings`.
pub(crate) const KIND_RECORDING: &str = "recording";
/// A miniatura, `<id>.jpg`. Nasce com a linha de `recordings`: o nome é uma
/// função pura do id, e registá-la antes de se saber se vai existir é o lado
/// seguro (uma linha a mais não faz mal; um ficheiro a mais fica para sempre).
pub(crate) const KIND_THUMBNAIL: &str = "thumbnail";
/// A PASTA das pistas por fonte, `iso-<id>/`. A única entrada com `is_dir`.
pub(crate) const KIND_ISO_TRACKS: &str = "iso_tracks";
/// Um recorte, `clip-<id do recorte>.webm`.
pub(crate) const KIND_CLIP: &str = "clip";
/// O WebVTT que sai ao lado de um recorte, `clip-<id do recorte>.vtt`.
pub(crate) const KIND_CLIP_CAPTIONS: &str = "clip_captions";

/// O nome com que um caminho entra no registo: tem de ser filho **directo** do
/// `recordings_dir`, porque é isso que a coluna `path` guarda (sem barras — ver
/// o `CHECK` da 0126).
///
/// Não é cerimónia: quem recolhe faz `remove_dir_all` do que esta coluna diz.
/// A validação é na ENTRADA e na saída — a 0126 recusa a travessia na base, e
/// [`caminho_de`] recusa-a outra vez ao ler, porque uma das duas pode ser
/// contornada por um `psql` e nenhuma das duas custa nada.
pub(crate) fn nome_em<'a>(recordings_dir: &Path, caminho: &'a Path) -> Option<&'a str> {
    // Igualdade TEXTUAL do pai, de propósito: assim `dir/../dir/x.webm` é
    // recusado, mesmo apontando para o mesmo sítio. Todos os produtores
    // constroem o caminho com `recordings_dir.join(...)`, logo o caso legítimo
    // casa sempre.
    if caminho.parent() != Some(recordings_dir) {
        return None;
    }
    let nome = caminho.file_name()?.to_str()?;
    if nome.is_empty() || nome == "." || nome == ".." {
        return None;
    }
    Some(nome)
}

/// O caminho de uma entrada do registo, ou `None` se a linha não for servível.
///
/// É a releitura de [`nome_em`] no sentido inverso, e o único sítio onde um
/// nome vindo da base se transforma num caminho que se vai apagar.
fn caminho_de(recordings_dir: &Path, nome: &str) -> Option<PathBuf> {
    let caminho = recordings_dir.join(nome);
    // Reconstrói-se e CONFIRMA-SE: um nome com barra, vazio ou `..` — que só
    // chega aqui se alguém escrever na base por fora do `CHECK` da 0126 — não
    // produz um caminho que saia do `recordings_dir`.
    if nome_em(recordings_dir, &caminho) != Some(nome) {
        return None;
    }
    Some(caminho)
}

/// A gravação composta de `rec_id`. É UMA função porque o nome é construído em
/// quatro sítios — quem compõe, quem carrega, quem serve e quem apaga — e um
/// deles a divergir é um download partido ou um ficheiro que nunca sai.
pub fn composed_path(recordings_dir: &Path, rec_id: Uuid) -> PathBuf {
    recordings_dir.join(format!("{rec_id}.webm"))
}

/// Os artefactos que uma gravação tem por EXISTIR: a composta e a miniatura.
/// Os dois nomes são função pura do id, logo as linhas podem nascer na mesma
/// instrução que a linha de `recordings` — ver [`insercao_canonica`].
pub(crate) fn canonicos(recordings_dir: &Path, rec_id: Uuid) -> Vec<(&'static str, String, bool)> {
    [
        (KIND_RECORDING, composed_path(recordings_dir, rec_id)),
        (
            KIND_THUMBNAIL,
            crate::media_probe::thumbnail_path(recordings_dir, rec_id),
        ),
    ]
    .into_iter()
    .filter_map(|(kind, caminho)| {
        nome_em(recordings_dir, &caminho).map(|n| (kind, n.to_string(), false))
    })
    .collect()
}

/// Registra os artefactos canónicos de uma gravação, **dentro da transacção
/// que cria a linha de `recordings`**.
///
/// Porque é na mesma transacção: os dois ficheiros nascem por a gravação
/// nascer, e a regra deste módulo é «a linha antes do ficheiro». Fora da
/// transacção havia uma janela — e, pior, havia uma decisão a tomar quando a
/// segunda escrita falhasse: escrever o ficheiro sem registo é a fuga, não o
/// escrever é perder uma gravação por um soluço da base. Juntas, a pergunta não
/// se põe. O id tem de vir de fora (`Uuid::new_v4()`) e não do `DEFAULT` da
/// coluna, porque os dois nomes derivam dele.
pub(crate) async fn register_canonical(
    tx: &mut sqlx::PgConnection,
    recordings_dir: &Path,
    rec_id: Uuid,
) -> sqlx::Result<()> {
    for (kind, nome, is_dir) in canonicos(recordings_dir, rec_id) {
        sqlx::query(
            "INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (path) DO NOTHING",
        )
        .bind(rec_id)
        .bind(kind)
        .bind(&nome)
        .bind(is_dir)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

/// Registra um artefacto. **Chama-se ANTES de escrever o ficheiro.**
///
/// `false` significa «não há linha», e quem produz tem de tratar isso como
/// «não escrevas»: um ficheiro sem linha é a fuga que este módulo fecha. Os
/// dois produtores que chamam isto têm um caminho de recuo que não perde nada
/// (as pistas ISO ficam no `tmp-`, que o varredor de segmentos recolhe; o
/// recorte volta à fila).
pub(crate) async fn register(
    db: &sqlx::PgPool,
    rec_id: Uuid,
    kind: &str,
    recordings_dir: &Path,
    caminho: &Path,
    is_dir: bool,
) -> bool {
    let Some(nome) = nome_em(recordings_dir, caminho) else {
        tracing::error!(%rec_id, kind, caminho = %caminho.display(),
            "artefacto de gravação: o caminho não é filho directo do recordings_dir — não se registra");
        return false;
    };
    // `ON CONFLICT (path) DO NOTHING`: uma composição repetida ou uma retoma
    // voltam a passar por aqui, e o que importa afirmar é que há linha — não
    // que ela nasceu agora.
    let r = sqlx::query(
        "INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (path) DO NOTHING",
    )
    .bind(rec_id)
    .bind(kind)
    .bind(nome)
    .bind(is_dir)
    .execute(db)
    .await;
    match r {
        Ok(_) => true,
        Err(e) => {
            tracing::error!(%rec_id, kind, nome, error = %e,
                "artefacto de gravação: não foi possível registar — o ficheiro NÃO se escreve");
            false
        }
    }
}

/// Esquece as linhas destes caminhos. Para quem apaga um ficheiro por sua conta
/// (a rota de `DELETE` de um recorte) e quer deixar o registo verdadeiro.
///
/// Não apaga nada do disco: é só escrituração. Uma linha que sobrevivesse ao
/// ficheiro era inofensiva — mas deixar o registo a mentir é como as tabelas
/// desta casa se tornam inúteis.
pub(crate) async fn forget(db: &sqlx::PgPool, recordings_dir: &Path, caminhos: &[PathBuf]) {
    let nomes: Vec<String> = caminhos
        .iter()
        .filter_map(|c| nome_em(recordings_dir, c).map(str::to_string))
        .collect();
    if nomes.is_empty() {
        return;
    }
    if let Err(e) = sqlx::query("DELETE FROM recording_artifacts WHERE path = ANY($1)")
        .bind(&nomes)
        .execute(db)
        .await
    {
        tracing::warn!(error = %e, "artefacto de gravação: não foi possível esquecer as linhas");
    }
}

/// Uma linha do registo, como quem recolhe a lê.
struct Entrada {
    id: Uuid,
    path: String,
    is_dir: bool,
}

/// Apaga do disco, e só então do registo, o que `entradas` nomeia. Devolve
/// quantas saíram.
///
/// **A linha só morre depois do ficheiro.** Uma linha apagada antes de o
/// `unlink` ter sucesso era a fuga a reabrir-se: o ficheiro ficava, e já não
/// havia nada que o nomeasse. Por isso um ficheiro que não se consegue apagar
/// (permissões, volume em leitura) MANTÉM a linha e volta a ser tentado no
/// tique seguinte, em vez de desaparecer do registo em silêncio.
async fn apaga(state: &AppState, entradas: Vec<Entrada>) -> u64 {
    let raiz = &state.config.recordings_dir;
    let mut saidos: Vec<Uuid> = Vec::new();
    let mut presos = 0u64;
    for e in entradas {
        let Some(caminho) = caminho_de(raiz, &e.path) else {
            // Uma linha que não se consegue transformar num caminho seguro não
            // se apaga NEM se esquece: alguém a escreveu por fora do `CHECK` da
            // 0126, e apagá-la calada era perder a única pista disso.
            tracing::error!(path = %e.path,
                "artefacto de gravação: linha com caminho inseguro — ignorada, e fica para ser vista");
            presos += 1;
            continue;
        };
        let r = if e.is_dir {
            tokio::fs::remove_dir_all(&caminho).await
        } else {
            tokio::fs::remove_file(&caminho).await
        };
        match r {
            // `NotFound` é sucesso: a linha nasce antes do ficheiro, logo «não
            // está lá» é o caso normal de um ficheiro que nunca chegou a ser
            // escrito (a miniatura de uma gravação só de áudio, um render que
            // falhou). Tratá-lo como erro deixava a linha a acumular para
            // sempre.
            Ok(()) => saidos.push(e.id),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => saidos.push(e.id),
            Err(err) => {
                tracing::warn!(caminho = %caminho.display(), error = %err,
                    "artefacto de gravação: não foi possível apagar — a linha fica para o próximo tique");
                presos += 1;
            }
        }
    }
    if presos > 0 {
        state
            .metrics
            .recording_artifacts_stuck_total
            .fetch_add(presos, Relaxed);
    }
    if saidos.is_empty() {
        return 0;
    }
    match sqlx::query("DELETE FROM recording_artifacts WHERE id = ANY($1)")
        .bind(&saidos)
        .execute(&state.db)
        .await
    {
        Ok(_) => saidos.len() as u64,
        Err(e) => {
            // Os ficheiros já saíram; as linhas ficam e o próximo tique
            // encontra-as, faz `unlink` de nada (`NotFound` = sucesso) e
            // limpa-as. É idempotente de propósito.
            tracing::warn!(error = %e,
                "artefacto de gravação: ficheiros apagados mas as linhas ficaram — o próximo tique limpa-as");
            0
        }
    }
}

/// Apaga do disco e do registo TODOS os ficheiros desta gravação. É o que a
/// retenção chama em vez de se lembrar de três caminhos à mão.
///
/// Chama-se **antes** do `DELETE FROM recordings`: o registo não tem FK, logo a
/// ordem não é obrigatória — mas purgar primeiro é o que faz a falha ser
/// inofensiva. Se este processo morrer entre a purga e o `DELETE`, a gravação
/// continua lá e a próxima volta da retenção repete tudo; se morrer depois, o
/// [`sweep_orphans`] apanha o que ficou.
pub async fn purge(state: &AppState, rec_id: Uuid) -> u64 {
    let entradas: Vec<Entrada> = match sqlx::query_as::<_, (Uuid, String, bool)>(
        "SELECT id, path, is_dir FROM recording_artifacts WHERE recording_id = $1",
    )
    .bind(rec_id)
    .fetch_all(&state.db)
    .await
    {
        Ok(rs) => rs
            .into_iter()
            .map(|(id, path, is_dir)| Entrada { id, path, is_dir })
            .collect(),
        Err(e) => {
            // Falha FECHADO: sem saber o que apagar, não se apaga nada. O
            // chamador não apaga a gravação e a próxima volta tenta outra vez.
            tracing::error!(%rec_id, error = %e,
                "artefacto de gravação: não foi possível ler o registo — nada se apaga");
            return 0;
        }
    };
    apaga(state, entradas).await
}

/// Varre o registo: os artefactos cuja gravação já não existe.
///
/// É o caminho dos ficheiros que ninguém mandou apagar — o `ON DELETE CASCADE`
/// que levou o recorte (0118) ou a gravação (cascata de `rooms`/`users`) e
/// deixou os bytes atrás. Não olha para o disco em momento nenhum: a lista do
/// que pode apagar sai da tabela, e essa é a diferença que torna isto seguro
/// onde a varredura por nome de pasta não era (ver o topo deste módulo).
///
/// Falha FECHADO: um erro da base devolve 0 e não apaga nada.
pub async fn sweep_orphans(state: &AppState) -> u64 {
    let entradas: Vec<Entrada> = match sqlx::query_as::<_, (Uuid, String, bool)>(
        "SELECT a.id, a.path, a.is_dir FROM recording_artifacts a
          WHERE NOT EXISTS (SELECT 1 FROM recordings r WHERE r.id = a.recording_id)",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(rs) => rs
            .into_iter()
            .map(|(id, path, is_dir)| Entrada { id, path, is_dir })
            .collect(),
        Err(e) => {
            tracing::error!(error = %e,
                "artefactos órfãos: não foi possível ler o registo — nada se apaga");
            return 0;
        }
    };
    if entradas.is_empty() {
        return 0;
    }
    let n = apaga(state, entradas).await;
    if n > 0 {
        tracing::info!(apagados = n, "artefactos de gravações que já não existem apagados");
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raiz() -> PathBuf {
        PathBuf::from("/var/lib/delonix/recordings")
    }

    #[test]
    fn um_filho_directo_e_registavel() {
        let r = raiz();
        assert_eq!(nome_em(&r, &r.join("a.webm")), Some("a.webm"));
        assert_eq!(nome_em(&r, &r.join("iso-abc")), Some("iso-abc"));
    }

    /// O que esta função existe para travar. Quem recolhe faz `remove_dir_all`
    /// do que o registo diz: um caminho que saia da raiz é uma árvore do
    /// sistema apagada.
    #[test]
    fn o_que_sai_da_raiz_nao_e_registavel() {
        let r = raiz();
        for mau in [
            PathBuf::from("/etc/passwd"),
            r.join("sub").join("a.webm"),
            r.join("..").join("a.webm"),
            // O mesmo sítio por um caminho torto: a igualdade do pai é
            // textual de propósito, e é isto que ela recusa.
            r.join("..").join("recordings").join("a.webm"),
            r.clone(),
            PathBuf::from("a.webm"),
        ] {
            assert_eq!(
                nome_em(&r, &mau),
                None,
                "um caminho fora da raiz foi aceite: {}",
                mau.display()
            );
        }
    }

    /// A releitura: um nome que a base não devia conseguir guardar (o `CHECK`
    /// da 0126 recusa-o) não produz um caminho para apagar, se alguém o
    /// escrever por fora com um `psql`.
    #[test]
    fn um_nome_torto_vindo_da_base_nao_da_caminho() {
        let r = raiz();
        for mau in ["../../etc", "/etc/passwd", "sub/a.webm", "", ".", ".."] {
            assert_eq!(
                caminho_de(&r, mau),
                None,
                "um nome inseguro deu caminho: {mau:?}"
            );
        }
        assert_eq!(caminho_de(&r, "a.webm"), Some(r.join("a.webm")));
    }

    /// Os canónicos são os dois, com os nomes que os outros módulos constroem.
    /// Se um deles divergir, a retenção deixa de apagar um ficheiro e nada o
    /// diz — por isso a comparação é com o caminho que o PRODUTOR usa.
    #[test]
    fn os_canonicos_sao_a_composta_e_a_miniatura() {
        let r = raiz();
        let id = Uuid::new_v4();
        let c = canonicos(&r, id);
        assert_eq!(c.len(), 2, "{c:?}");
        assert_eq!(c[0], (KIND_RECORDING, format!("{id}.webm"), false));
        assert_eq!(c[1], (KIND_THUMBNAIL, format!("{id}.jpg"), false));
        assert_eq!(composed_path(&r, id), r.join(format!("{id}.webm")));
        assert_eq!(
            crate::media_probe::thumbnail_path(&r, id),
            r.join(format!("{id}.jpg"))
        );
    }
}
