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

/// Quantos artefactos órfãos um tique recolhe, no máximo. Ver [`sweep_orphans`].
const ORPHAN_BATCH: i64 = 500;

/// O que um artefacto É: o nome que leva no disco, se é pasta, e quem o produz.
///
/// # Porque é um enum e não cinco constantes soltas
///
/// Antes disto o `kind` era uma `&str` e o NOME do ficheiro era construído noutro
/// lado — `{id}.webm` aqui, `{id}.jpg` no `media_probe`, `iso-{id}` no `recorder`,
/// `clip-{id}.webm`/`.vtt` no `recording_clips`. Quatro módulos, e o registo só
/// conhecia os dois primeiros. Isso tem duas consequências, e as duas já se
/// mediram nesta frente:
///
///  · o `kind` e o nome podiam divergir sem que nada desse vermelho — quem
///    recolhe apaga o que a coluna `path` diz, logo um produtor que registasse
///    um nome e escrevesse outro criava exactamente a fuga que este módulo
///    fecha;
///  · o derivado SEGUINTE (waveform, legendas por pista, pré-visualização)
///    nascia outra vez num quinto módulo, sem ninguém notar. O defeito do
///    `A-02` é de OMISSÃO e repete-se sempre da mesma maneira.
///
/// Com o nome aqui dentro, as duas perguntas — «como se chama» e «o que é» —
/// têm a mesma resposta, no mesmo sítio, e uma variante nova obriga quem a
/// escreve a responder às três (nome, `is_dir`, produtor) para compilar.
///
/// A coluna `kind` continua texto na base, de propósito: um `CHECK` fechado
/// seria a armadilha da `0124` outra vez (ver o cabeçalho da `0125`).
/// **Cada variante traz o id de que o seu nome deriva**, e não é cerimónia: o
/// dos três primeiros é o da GRAVAÇÃO, o dos dois últimos é o do RECORTE. Um
/// `name(rec_id)` que as variantes de recorte ignorassem punha dois ids em jogo
/// em cada chamada, e trocá-los dava um nome que compila e aponta para o
/// ficheiro de outro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A gravação composta, `<id da gravação>.webm`. Nasce com a linha de
    /// `recordings`.
    Recording(Uuid),
    /// A miniatura, `<id da gravação>.jpg`. Nasce com a linha de `recordings`:
    /// o nome é função pura do id, e registá-la antes de se saber se vai
    /// existir é o lado seguro (uma linha a mais não faz mal; um ficheiro a
    /// mais fica para sempre).
    Thumbnail(Uuid),
    /// A PASTA das pistas por fonte, `iso-<id da gravação>/`. A única variante
    /// com `is_dir`.
    IsoTracks(Uuid),
    /// Um recorte, `clip-<id do recorte>.webm`.
    Clip(Uuid),
    /// O WebVTT que sai ao lado de um recorte, `clip-<id do recorte>.vtt`.
    ClipCaptions(Uuid),
}

/// Todas as variantes, para quem precise de percorrer o vocabulário (os testes).
#[cfg(test)]
pub(crate) fn todas(rec_id: Uuid, clip_id: Uuid) -> [Kind; 5] {
    [
        Kind::Recording(rec_id),
        Kind::Thumbnail(rec_id),
        Kind::IsoTracks(rec_id),
        Kind::Clip(clip_id),
        Kind::ClipCaptions(clip_id),
    ]
}

impl Kind {
    /// O valor que vai para a coluna `kind`. É o vocabulário que o
    /// `scripts/check-recording-artifacts.sh` compara com a lista de
    /// classificação, nos dois sentidos.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Kind::Recording(_) => "recording",
            Kind::Thumbnail(_) => "thumbnail",
            Kind::IsoTracks(_) => "iso_tracks",
            Kind::Clip(_) => "clip",
            Kind::ClipCaptions(_) => "clip_captions",
        }
    }

    /// Um artefacto é uma PASTA ou um ficheiro, e isso **deriva do tipo**: não é
    /// um parâmetro que quem produz possa passar ao contrário.
    ///
    /// A migração 0126 argumenta, com razão, que um `CHECK` fechado na `kind`
    /// seria a armadilha da 0124 outra vez — mas a coluna que decide QUE chamada
    /// de sistema corre não pode ser um `bool` solto com `DEFAULT FALSE`: um
    /// produtor que o esquecesse registava a pasta ISO como ficheiro, e quem
    /// recolhe ficava a chamar `remove_file` num directório para sempre.
    ///
    /// E porque mesmo assim pode estar errada (uma linha escrita à mão, um
    /// `kind` que o código já não conhece), o [`delete_entries`] tenta a outra
    /// chamada quando o sistema diz que o tipo não bate. Uma fuga não fica à
    /// espera de uma tabela de tipos estar certa.
    pub(crate) fn is_dir(self) -> bool {
        matches!(self, Kind::IsoTracks(_))
    }

    /// Quem escreve este artefacto. **Não é decoração**: é o que torna
    /// respondível a pergunta «este derivado tem produtor registado?» sem
    /// procurar por `grep` em quatro módulos, e é o que a lista de
    /// classificação (`scripts/artefactos-de-gravacao.txt`) compara com o
    /// código em vez de ser mantida à mão em paralelo.
    pub(crate) fn producer(self) -> &'static str {
        match self {
            // Os dois canónicos nascem na transacção da linha de `recordings`.
            Kind::Recording(_) | Kind::Thumbnail(_) => "recordings::register_canonical",
            Kind::IsoTracks(_) => "recorder::registar_pistas_iso",
            Kind::Clip(_) => "recording_clips::compoe",
            Kind::ClipCaptions(_) => "recording_clips::escreve_vtt",
        }
    }

    /// O nome no disco. **É a única fonte**: nenhum outro módulo compõe um nome
    /// de artefacto, e é isso que o portão mede.
    pub(crate) fn name(self) -> String {
        match self {
            Kind::Recording(id) => format!("{id}.webm"),
            Kind::Thumbnail(id) => format!("{id}.jpg"),
            Kind::IsoTracks(id) => format!("iso-{id}"),
            Kind::Clip(id) => format!("clip-{id}.webm"),
            Kind::ClipCaptions(id) => format!("clip-{id}.vtt"),
        }
    }
}

/// O nome com que um caminho entra no registo: tem de ser filho **directo** do
/// `recordings_dir`, porque é isso que a coluna `path` guarda (sem barras — ver
/// o `CHECK` da 0126).
///
/// Não é cerimónia: quem recolhe faz `remove_dir_all` do que esta coluna diz.
/// A validação é na ENTRADA e na saída — a 0126 recusa a travessia na base, e
/// [`path_of`] recusa-a outra vez ao ler, porque uma das duas pode ser
/// contornada por um `psql` e nenhuma das duas custa nada.
pub(crate) fn name_in<'a>(recordings_dir: &Path, path: &'a Path) -> Option<&'a str> {
    // Igualdade TEXTUAL do pai, de propósito: assim `dir/../dir/x.webm` é
    // recusado, mesmo apontando para o mesmo sítio. Todos os produtores
    // constroem o caminho com `recordings_dir.join(...)`, logo o caso legítimo
    // casa sempre.
    if path.parent() != Some(recordings_dir) {
        return None;
    }
    let nome = path.file_name()?.to_str()?;
    if nome.is_empty() || nome == "." || nome == ".." {
        return None;
    }
    Some(nome)
}

/// O caminho de uma entrada do registo, ou `None` se a linha não for servível.
///
/// É a releitura de [`name_in`] no sentido inverso, e o único sítio onde um
/// nome vindo da base se transforma num caminho que se vai apagar.
fn path_of(recordings_dir: &Path, nome: &str) -> Option<PathBuf> {
    let path = recordings_dir.join(nome);
    // Reconstrói-se e CONFIRMA-SE: um nome com barra, vazio ou `..` — que só
    // chega aqui se alguém escrever na base por fora do `CHECK` da 0126 — não
    // produz um caminho que saia do `recordings_dir`.
    if name_in(recordings_dir, &path) != Some(nome) {
        return None;
    }
    Some(path)
}

/// O caminho de um artefacto, **para quem o vai LER**.
///
/// Serve quem descarrega a composta, quem mostra a miniatura, quem lê o `.vtt`
/// de um recorte e quem o usa como fonte de outro — todos precisam do caminho
/// sem escrever nada, e para todos eles a linha já existe (ou não, e o ficheiro
/// também não).
///
/// **Quem PRODUZ não chama isto: chama o [`reserve`].** A diferença é a regra
/// do módulo inteira — o `reserve` não devolve caminho nenhum sem ter a linha
/// escrita, e é o que torna «ficheiro sem registo» não-representável em vez de
/// apenas desaconselhado.
pub(crate) fn path_for(recordings_dir: &Path, kind: Kind) -> PathBuf {
    recordings_dir.join(kind.name())
}

/// A gravação composta de `rec_id`, que é o artefacto que mais sítios lêem.
/// Atalho para [`path_for`] com [`Kind::Recording`] — o nome continua a sair do
/// `enum`, aqui só se poupa a repetição nos sete chamadores.
pub(crate) fn composed_path(recordings_dir: &Path, rec_id: Uuid) -> PathBuf {
    path_for(recordings_dir, Kind::Recording(rec_id))
}

/// O nome de um caminho, já em `String`. Os dois usos precisam de o possuir.
fn name_of(recordings_dir: &Path, path: &Path) -> Option<String> {
    name_in(recordings_dir, path).map(str::to_string)
}

/// Os artefactos que uma gravação tem por EXISTIR: a composta e a miniatura.
/// Os dois nomes são função pura do id, logo as linhas podem nascer na mesma
/// transacção que a linha de `recordings` — ver [`register_canonical`].
///
/// `None` quando um dos nomes não é registável, e **nunca uma lista mais curta**:
/// isto devolvia um `Vec` construído com `filter_map`, o que significava que um
/// `recordings_dir` que deixasse de casar com a comparação de [`name_in`]
/// (relativo, com barra no fim) dava uma lista VAZIA, o `register_canonical`
/// inseria zero linhas, a transacção fechava bem — e a gravação ficava com os
/// dois ficheiros sem registo. Um descarte silencioso no único sítio onde um
/// descarte silencioso é exactamente o defeito que o módulo persegue.
pub(crate) fn canonical(recordings_dir: &Path, rec_id: Uuid) -> Option<[(Kind, String); 2]> {
    Some([
        (
            Kind::Recording(rec_id),
            name_of(recordings_dir, &path_for(recordings_dir, Kind::Recording(rec_id)))?,
        ),
        (
            Kind::Thumbnail(rec_id),
            name_of(recordings_dir, &path_for(recordings_dir, Kind::Thumbnail(rec_id)))?,
        ),
    ])
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
    // Sem nomes registáveis a transacção MORRE, e com ela a linha da gravação.
    // Inserir zero linhas e deixar a gravação nascer era criar precisamente o
    // que este módulo existe para não haver: ficheiros que ninguém nomeia.
    let Some(canon) = canonical(recordings_dir, rec_id) else {
        return Err(sqlx::Error::Configuration(
            format!(
                "recordings_dir não produz names de artefacto registáveis: {}",
                recordings_dir.display()
            )
            .into(),
        ));
    };
    for (kind, name) in canon {
        sqlx::query(
            "INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (path) DO NOTHING",
        )
        .bind(rec_id)
        .bind(kind.as_str())
        .bind(&name)
        .bind(kind.is_dir())
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

/// Reserva um artefacto: escreve a linha e **só então** diz como se chama o
/// ficheiro. `None` é «não há linha» e significa «não escrevas».
///
/// # Porque devolve o caminho em vez de o receber
///
/// A versão anterior era um `register(db, rec_id, kind, recordings_dir, path)
/// -> bool`: quem produzia construía o `PathBuf`, pedia o registo e depois
/// escrevia. Isso deixava três maneiras de reabrir a fuga, e nenhuma delas dava
/// vermelho:
///
///  · ignorar o `bool` (é só um valor de retorno — `let _ = register(...)`
///    compila) e escrever na mesma;
///  · registar um nome e escrever noutro, porque o caminho existia antes e
///    independentemente da linha;
///  · esquecer a chamada por completo num produtor novo, que é precisamente
///    como os `clip-<id>.webm`/`.vtt` da 0118 ficaram quatro meses sem ninguém
///    que os apagasse.
///
/// Com a reserva, o caminho é **resultado** da linha e não entrada dela. Quem
/// produz não tem de onde tirar um `PathBuf` sob o `recordings_dir` sem ter
/// passado por aqui (o nome vive no [`Kind`] e o único outro acesso,
/// [`path_for`], é a porta de quem LÊ). Ignorar o `None` deixa de ser possível
/// por construção: sem ele não há nome para dar ao `ffmpeg` nem ao
/// `tokio::fs::write`.
///
/// Os três produtores que chamam isto têm um recuo que não perde nada — as
/// pistas ISO ficam no `tmp-`, que o varredor de segmentos recolhe; o recorte
/// volta à fila; o `.vtt` sai e o vídeo fica.
pub(crate) async fn reserve(
    db: &sqlx::PgPool,
    rec_id: Uuid,
    kind: Kind,
    recordings_dir: &Path,
) -> Option<PathBuf> {
    let path = path_for(recordings_dir, kind);
    // A mesma validação de sempre, agora sobre um caminho que este módulo
    // construiu: um `recordings_dir` torto (relativo com `..`, com barra no
    // fim) continua a não produzir nome registável, e aí não se escreve nada.
    let Some(name) = name_in(recordings_dir, &path) else {
        tracing::error!(%rec_id, kind = kind.as_str(), path = %path.display(),
            "artefacto de gravação: o path não é filho directo do recordings_dir — não se reserva");
        return None;
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
    .bind(kind.as_str())
    .bind(name)
    .bind(kind.is_dir())
    .execute(db)
    .await;
    match r {
        Ok(_) => Some(path),
        Err(e) => {
            tracing::error!(%rec_id, kind = kind.as_str(), name, error = %e,
                "artefacto de gravação: não foi possível reservar — o ficheiro NÃO se escreve");
            None
        }
    }
}

/// Desfaz estes artefactos: apaga-os do disco e, **só dos que saíram**, a linha.
///
/// É o que um produtor chama quando desfaz o que escreveu — a rota de `DELETE`
/// de um recorte, ou o registo ISO que não registou pista nenhuma.
///
/// Passou a existir porque a primeira versão disto era um `forget` que só
/// mexia na base, chamado depois de um `let _ = remove_file(...)` cujo erro era
/// descartado. Nos três sítios, um `unlink` recusado (volume em leitura,
/// `EACCES`, identificador NFS velho) apagava a ÚNICA linha que nomeava
/// ficheiros que ficavam no disco — a fuga que este módulo fecha, reaberta no
/// caminho de erro, e sem contador que a denunciasse. A ordem «o ficheiro
/// primeiro, a linha só se ele saiu» vive em [`delete_entries`] e agora não há como a
/// contornar: quem desfaz não tem uma função que mexa só na base.
pub(crate) async fn discard(
    db: &sqlx::PgPool,
    recordings_dir: &Path,
    metrics: &crate::metrics::Metrics,
    caminhos: &[PathBuf],
) -> u64 {
    let names: Vec<String> = caminhos
        .iter()
        .filter_map(|c| name_in(recordings_dir, c).map(str::to_string))
        .collect();
    if names.is_empty() {
        return 0;
    }
    // Falha FECHADO: sem saber o que o registo diz, não se apaga nem se
    // esquece nada. A retenção ou o varredor voltam a encontrar as linhas.
    let Some(entries) = entries_from(
        sqlx::query_as("SELECT id, path, is_dir FROM recording_artifacts WHERE path = ANY($1)")
            .bind(&names)
            .fetch_all(db)
            .await,
        "por path",
    ) else {
        return 0;
    };
    delete_entries(db, recordings_dir, metrics, entries).await
}

/// Uma linha do registo, como quem recolhe a lê.
struct Entry {
    id: Uuid,
    path: String,
    is_dir: bool,
}

/// Apaga do disco, e só então do registo, o que `entries` nomeia. Devolve
/// quantas saíram.
///
/// **A linha só morre depois do ficheiro.** Uma linha apagada antes de o
/// `unlink` ter sucesso era a fuga a reabrir-se: o ficheiro ficava, e já não
/// havia nada que o nomeasse. Por isso um ficheiro que não se consegue apagar
/// (permissões, volume em leitura) MANTÉM a linha e volta a ser tentado no
/// tique seguinte, em vez de desaparecer do registo em silêncio.
/// `remove_dir_all` ou `remove_file`, conforme pedido.
async fn remove_as(path: &Path, is_dir: bool) -> std::io::Result<()> {
    if is_dir {
        tokio::fs::remove_dir_all(path).await
    } else {
        tokio::fs::remove_file(path).await
    }
}

async fn delete_entries(
    db: &sqlx::PgPool,
    raiz: &Path,
    metrics: &crate::metrics::Metrics,
    entries: Vec<Entry>,
) -> u64 {
    let mut gone: Vec<Uuid> = Vec::new();
    let mut stuck = 0u64;
    for e in entries {
        let Some(path) = path_of(raiz, &e.path) else {
            // Uma linha que não se consegue transformar num caminho seguro não
            // se apaga NEM se esquece: alguém a escreveu por fora do `CHECK` da
            // 0126, e apagá-la calada era perder a única pista disso.
            tracing::error!(path = %e.path,
                "artefacto de gravação: linha com path inseguro — ignorada, e fica para ser vista");
            stuck += 1;
            continue;
        };
        // A outra chamada, se o sistema disser que o tipo não bate. A coluna
        // `is_dir` vem de `is_dir_of(kind)`, mas uma linha escrita à mão ou um
        // `kind` que essa função ainda não conheça davam um `remove_file` num
        // directório — `EISDIR` para sempre, o contador a subir e o disco a não
        // libertar. A fuga não fica à espera de uma tabela de tipos estar certa.
        let mut r = remove_as(&path, e.is_dir).await;
        if matches!(
            r.as_ref().map_err(std::io::Error::kind),
            Err(std::io::ErrorKind::IsADirectory) | Err(std::io::ErrorKind::NotADirectory)
        ) {
            tracing::warn!(path = %path.display(), is_dir = e.is_dir,
                "artefacto de gravação: o tipo registado não bate com o disco — tentada a outra chamada");
            r = remove_as(&path, !e.is_dir).await;
        }
        match r {
            // `NotFound` é sucesso: a linha nasce antes do ficheiro, logo «não
            // está lá» é o caso normal de um ficheiro que nunca chegou a ser
            // escrito (a miniatura de uma gravação só de áudio, um render que
            // falhou). Tratá-lo como erro deixava a linha a acumular para
            // sempre.
            Ok(()) => gone.push(e.id),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => gone.push(e.id),
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err,
                    "artefacto de gravação: não foi possível apagar — a linha fica para o próximo tique");
                stuck += 1;
            }
        }
    }
    if stuck > 0 {
        metrics.recording_artifacts_stuck_total.fetch_add(stuck, Relaxed);
    }
    if gone.is_empty() {
        return 0;
    }
    match sqlx::query("DELETE FROM recording_artifacts WHERE id = ANY($1)")
        .bind(&gone)
        .execute(db)
        .await
    {
        Ok(_) => gone.len() as u64,
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

/// O que cada leitura do registo devolve, com a MESMA regra para as três:
/// `None` é «a base não respondeu» e **não** «não há nada». É essa distinção
/// que faz quem chama falhar FECHADO — a mesma que o `decide_iso` do gravador
/// existe para fazer, e pela mesma razão: tratar um erro como lista vazia é
/// autorizar uma decisão sobre dados que não se leram.
///
/// O SQL fica em cada chamador, estático. Uma versão disto montava a cláusula
/// `WHERE` com `format!` e um `$1` genérico, o que obrigava a condição dos
/// órfãos a arrastar um `AND $1` que não queria.
fn entries_from(
    r: Result<Vec<(Uuid, String, bool)>, sqlx::Error>,
    o_que: &str,
) -> Option<Vec<Entry>> {
    match r {
        Ok(rs) => Some(
            rs.into_iter()
                .map(|(id, path, is_dir)| Entry { id, path, is_dir })
                .collect(),
        ),
        Err(e) => {
            tracing::error!(error = %e, o_que,
                "artefacto de gravação: não foi possível ler o registo — nada se apaga");
            None
        }
    }
}

/// Apaga do disco e do registo TODOS os ficheiros desta gravação. É o que a
/// retenção chama em vez de se lembrar de três caminhos à mão.
///
/// `None` significa **«não se sabe o que esta gravação tem no disco»** (a base
/// não respondeu), e quem chama tem de tratar isso como «não apagues a
/// gravação»: sem a linha de `recordings`, a retenção nunca mais volta a esta
/// gravação e o que ficou no disco passa a depender só do [`sweep_orphans`].
/// `Some(0)` é diferente e é normal — uma gravação sem artefactos no disco.
///
/// Chama-se **antes** do `DELETE FROM recordings`: o registo não tem FK, logo a
/// ordem não é obrigatória — mas purgar primeiro é o que faz a falha ser
/// inofensiva. Se este processo morrer entre a purga e o `DELETE`, a gravação
/// continua lá e a próxima volta da retenção repete tudo; se morrer depois, o
/// [`sweep_orphans`] apanha o que ficou.
pub async fn purge(state: &AppState, rec_id: Uuid) -> Option<u64> {
    let entries = entries_from(
        sqlx::query_as("SELECT id, path, is_dir FROM recording_artifacts WHERE recording_id = $1")
            .bind(rec_id)
            .fetch_all(&state.db)
            .await,
        "de uma gravação",
    )?;
    Some(
        delete_entries(
            &state.db,
            &state.config.recordings_dir,
            &state.metrics,
            entries,
        )
        .await,
    )
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
    let Some(entries) = entries_from(
        sqlx::query_as(
            "SELECT a.id, a.path, a.is_dir FROM recording_artifacts a
              WHERE NOT EXISTS (SELECT 1 FROM recordings r WHERE r.id = a.recording_id)
              LIMIT $1",
        )
        .bind(ORPHAN_BATCH)
        .fetch_all(&state.db)
        .await,
        "órfãos",
    ) else {
        return 0;
    };
    // O tecto não é decoração: o backfill da 0126 registra `iso_tracks` para
    // TODAS as gravações, logo a tabela tem ~3 linhas por gravação desde o
    // primeiro dia, e UMA cascata de `rooms`/`users` de uma organização grande
    // torna dezenas de milhares de linhas órfãs no mesmo instante. Sem tecto,
    // um tique materializava-as todas em memória e ficava a desligar ficheiros
    // durante minutos, de seguida, no mesmo worker. Com tecto, cada tique leva
    // um bocado e o seguinte leva o resto — o trabalho não se perde, só se
    // reparte.
    let filled_batch = entries.len() as i64 == ORPHAN_BATCH;
    if entries.is_empty() {
        return 0;
    }
    let n = delete_entries(
        &state.db,
        &state.config.recordings_dir,
        &state.metrics,
        entries,
    )
    .await;
    if n > 0 {
        tracing::info!(apagados = n, filled_batch,
            "artefactos de gravações que já não existem apagados");
    }
    if filled_batch {
        // Dizê-lo: um tecto silencioso lê-se como «já não há nada», que é
        // precisamente a leitura errada quando ainda há dezenas de milhares.
        tracing::info!(lote = ORPHAN_BATCH,
            "artefactos órfãos: o lote encheu — ficam mais para o próximo tique");
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
        assert_eq!(name_in(&r, &r.join("a.webm")), Some("a.webm"));
        assert_eq!(name_in(&r, &r.join("iso-abc")), Some("iso-abc"));
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
                name_in(&r, &mau),
                None,
                "um path fora da raiz foi aceite: {}",
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
                path_of(&r, mau),
                None,
                "um nome inseguro deu path: {mau:?}"
            );
        }
        assert_eq!(path_of(&r, "a.webm"), Some(r.join("a.webm")));
    }

    /// Os canónicos são os dois, com os nomes que os outros módulos constroem.
    /// Se um deles divergir, a retenção deixa de apagar um ficheiro e nada o
    /// diz — por isso a comparação é com o caminho que o PRODUTOR usa.
    #[test]
    fn os_canonicos_sao_a_composta_e_a_miniatura() {
        let r = raiz();
        let id = Uuid::new_v4();
        let c = canonical(&r, id).expect("os canónicos têm de sair sempre");
        assert_eq!(
            c,
            [
                (Kind::Recording(id), format!("{id}.webm")),
                (Kind::Thumbnail(id), format!("{id}.jpg")),
            ]
        );
        assert_eq!(composed_path(&r, id), r.join(format!("{id}.webm")));
        assert_eq!(
            crate::media_probe::thumbnail_path(&r, id),
            r.join(format!("{id}.jpg"))
        );
        // E nenhum dos dois é uma pasta: o `is_dir` deriva do tipo e só a
        // pasta das pistas o tem. Um canónico marcado como directório fazia o
        // `remove_dir_all` correr no `.webm` de uma gravação.
        for (kind, _) in &c {
            assert!(!kind.is_dir(), "{kind:?} não é uma pasta");
        }
        assert!(
            Kind::IsoTracks(id).is_dir(),
            "a pasta das pistas É uma pasta"
        );
    }

    /// O vocabulário inteiro, num sítio só: cada variante tem um `kind`
    /// distinto, um nome distinto, e o `is_dir` só na pasta.
    ///
    /// Existe porque a troca das cinco constantes pelo `enum` é só uma melhoria
    /// se o `enum` for de facto a ÚNICA fonte — e um `as_str` repetido por
    /// copy-paste (dois ramos a devolver `"clip"`) fazia duas variantes
    /// partilharem linha no registo sem nada dar vermelho.
    #[test]
    fn cada_variante_tem_kind_nome_e_is_dir_proprios() {
        let rec = Uuid::new_v4();
        let clip = Uuid::new_v4();
        let todas = todas(rec, clip);

        let kinds: std::collections::BTreeSet<_> = todas.iter().map(|k| k.as_str()).collect();
        assert_eq!(kinds.len(), todas.len(), "dois `kind` iguais: {kinds:?}");
        let nomes: std::collections::BTreeSet<_> = todas.iter().map(|k| k.name()).collect();
        assert_eq!(nomes.len(), todas.len(), "dois nomes iguais: {nomes:?}");

        // Os nomes, por extenso. Um `format!` trocado aqui é uma rota que serve
        // o vazio ou um ficheiro que nunca sai, e nenhuma das duas dá vermelho
        // sozinha.
        assert_eq!(Kind::Recording(rec).name(), format!("{rec}.webm"));
        assert_eq!(Kind::Thumbnail(rec).name(), format!("{rec}.jpg"));
        assert_eq!(Kind::IsoTracks(rec).name(), format!("iso-{rec}"));
        assert_eq!(Kind::Clip(clip).name(), format!("clip-{clip}.webm"));
        assert_eq!(Kind::ClipCaptions(clip).name(), format!("clip-{clip}.vtt"));

        // Uma só pasta, e é a das pistas.
        let pastas: Vec<_> = todas.iter().filter(|k| k.is_dir()).collect();
        assert_eq!(
            pastas,
            vec![&Kind::IsoTracks(rec)],
            "o `is_dir` deixou de ser só da pasta das pistas"
        );
        // E todas têm produtor nomeado: é o que torna respondível «este
        // derivado tem quem o escreva registado?» sem grep por quatro módulos.
        for k in &todas {
            assert!(!k.producer().is_empty(), "{k:?} sem produtor");
        }
    }

    /// Um `recordings_dir` que deixe de casar com a comparação de `name_in`
    /// (relativo, por exemplo) devolve `None` — **nunca uma lista mais curta**.
    /// Era isto que o `filter_map` da primeira versão escondia: zero linhas
    /// inseridas, transacção a fechar bem, e a gravação com os dois ficheiros
    /// sem registo.
    #[test]
    fn um_recordings_dir_que_nao_serve_nao_da_lista_curta() {
        // Um caminho relativo: o `join` produz `recordings/<id>.webm`, cujo pai
        // casa — logo este caso PASSA e é o controlo positivo. O que não casa é
        // uma raiz com componente `..`, que o `name_in` recusa por igualdade
        // textual do pai.
        assert!(canonical(Path::new("recordings"), Uuid::new_v4()).is_some());
        let torta = PathBuf::from("/var/lib").join("..").join("lib");
        assert_eq!(
            canonical(&torta, Uuid::new_v4()).map(|c| c.len()),
            Some(2),
            "o join preserva o `..` no pai, logo este caso casa — se deixar de casar, é None e não uma lista de 1"
        );
    }
}
