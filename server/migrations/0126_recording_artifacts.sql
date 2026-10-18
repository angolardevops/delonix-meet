-- O registo dos ficheiros derivados de uma gravação.
--
-- PORQUE EXISTE. Uma gravação deixa no `recordings_dir` cinco coisas: a
-- composta (`<id>.webm`), a miniatura (`<id>.jpg`), a pasta das pistas ISO
-- (`iso-<id>/`), e por cada recorte um `clip-<id>.webm` e um `clip-<id>.vtt`.
-- Nenhuma delas estava registada em sítio nenhum, e cada caminho que apaga
-- tinha de se lembrar de todas, uma a uma. Mediu-se o resultado (2026-10-18,
-- commit `ece92e6a4`), e eram duas fugas com a MESMA causa:
--
--  1. O `registar_pistas_iso` renomeia `tmp-<sessão>` para `iso-<gravação>`
--     ANTES de escrever as linhas de `recording_tracks`. Um processo morto
--     nessa janela deixava a pasta sem nenhuma linha que a servisse, e o
--     `sweep_orphan_segments` só olha para `tmp-*`.
--  2. A `0118` dá `ON DELETE CASCADE` aos recortes, logo as LINHAS saem com a
--     gravação — e isso fazia a fuga parecer fechada. Os ficheiros
--     `clip-<id>.webm`/`.vtt` não eram apagados por varredor nenhum.
--
-- O que NÃO se repete: inferir a propriedade de um ficheiro a partir do NOME
-- da pasta e da sua idade. Essa versão foi escrita, medida e revertida — as
-- três razões estão no comentário do `sweep_orphan_segments`, e a pior é que
-- «a base não conhece este id» não é um erro: uma base restaurada para um
-- ponto anterior, ou outro ambiente apontado ao mesmo volume
-- `ReadWriteMany`, autorizava `remove_dir_all` em TODAS as pastas do volume
-- no primeiro tique. Esta tabela inverte a autoridade: só se apaga um caminho
-- que ESTA tabela nomeia. Um ficheiro que ela não conhece nunca é tocado — é
-- a diferença entre varrer o disco e varrer um registo.
--
-- SEM CHAVE ESTRANGEIRA para `recordings`, e é A decisão desta migração.
-- Um `REFERENCES recordings(id) ON DELETE CASCADE` levava estas linhas no
-- mesmo instante em que a gravação morre — e é exactamente então que são
-- precisas, porque é a linha que diz ao varredor que ficheiro apagar. Era a
-- fuga 2 outra vez, uma camada acima. `recording_id` pode portanto ficar
-- pendurado de propósito (mesma escolha, e mesma razão, do
-- `recording_tracks.publisher_id` na 0124: a gravação continua a ser dele
-- ainda que o dono tenha saído), e é isso que faz uma linha sobreviver às
-- cascatas de `rooms` e de `users` com o ficheiro que elas deixam atrás.
--
-- E a `kind` NÃO tem `CHECK`, também de propósito. A 0124 pôs
-- `CHECK (kind IN (...))` com três valores, o SFU produzia um quarto
-- (`screen-audio`), o `INSERT` morria num `tracing::error!` e o ficheiro
-- ficava no disco sem linha — custou a migração 0125 e um portão de
-- sincronia. Aqui o preço de um valor novo seria o mesmo e pior: um derivado
-- novo (waveform, legendas por pista, pré-visualização) que falhasse o
-- `INSERT` por vocabulário ficava sem registo, que é precisamente a fuga que
-- esta tabela fecha. `kind` é descritivo — serve quem opera e os contadores —
-- e quem recolhe só lê a `is_dir`.
CREATE TABLE recording_artifacts (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- A gravação a que o ficheiro pertence. SEM `REFERENCES` (ver acima).
    recording_id UUID NOT NULL,
    -- Para quem opera e para os contadores: 'recording', 'thumbnail',
    -- 'iso_tracks', 'clip', 'clip_captions'. Sem `CHECK` (ver acima).
    kind         TEXT NOT NULL,
    -- O nome DENTRO do `recordings_dir`, nunca um caminho absoluto e nunca com
    -- uma barra — a mesma regra do `recording_tracks.filename` da 0124, e pela
    -- mesma razão: um caminho na base é uma travessia de directório à espera
    -- de acontecer, e quem apaga aqui apaga árvores inteiras. Um derivado que
    -- queira viver numa subpasta registra a PASTA, como a `iso_tracks` faz.
    path         TEXT NOT NULL CHECK (
                     path <> '' AND path !~ '/' AND path <> '.' AND path <> '..'
                 ),
    -- `remove_dir_all` ou `remove_file`. É o produtor que sabe, e é a ÚNICA
    -- coisa que quem recolhe interpreta — não há mapa de `kind` para tipo que
    -- possa divergir do código.
    is_dir       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Um caminho no `recordings_dir` tem UM dono. Todos os nomes derivam de um
-- UUID (`<id>.webm`, `iso-<id>`, `clip-<id>.vtt`), logo duas gravações não
-- podem colidir — e é esta restrição que torna o registo idempotente: cada
-- produtor escreve com `ON CONFLICT (path) DO NOTHING` e uma composição
-- repetida, ou uma retoma, não duplica linhas nem precisa de saber se já
-- passou por aqui.
CREATE UNIQUE INDEX recording_artifacts_path_uidx ON recording_artifacts(path);

-- A purga de uma gravação (o caminho da retenção).
CREATE INDEX recording_artifacts_recording_idx ON recording_artifacts(recording_id);

-- BACKFILL: o registo nasce a conhecer o que já está no disco.
--
-- Sem isto, a retenção — que passa a apagar PELO registo em vez de
-- re-derivar três caminhos à mão — deixava de apagar os ficheiros de todas as
-- gravações anteriores a esta migração. Fechar uma fuga abrindo-a para trás
-- não é fechá-la.
--
-- Uma linha cujo ficheiro não exista no disco é inofensiva: quem recolhe faz
-- um `remove_file` que não encontra nada e segue. O contrário — um ficheiro
-- sem linha — é a fuga, e é por isso que se registra por excesso (todas as
-- gravações, não só as `ready`).
INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
SELECT id, 'recording', id::text || '.webm', FALSE FROM recordings
ON CONFLICT (path) DO NOTHING;

INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
SELECT id, 'thumbnail', id::text || '.jpg', FALSE FROM recordings WHERE has_thumbnail
ON CONFLICT (path) DO NOTHING;

-- A pasta das pistas, para TODAS as gravações — e não só para as que têm
-- linhas em `recording_tracks`, que seria o óbvio.
--
-- A razão é a fuga 1: uma pasta que um processo morto deixou a meio do registo
-- NÃO tem pistas: é precisamente isso que a torna órfã. Um backfill por
-- `recording_tracks` registava as pastas saudáveis e deixava no disco, para
-- sempre, exactamente aquelas que este trabalho existe para recolher. Como
-- `iso-<id>` é função pura do id, registar por excesso não custa nada (quem
-- recolhe faz um `remove_dir_all` que não encontra nada e segue) e varre o
-- que já lá está.
--
-- O que isto NÃO alcança, e fica dito: uma pasta `iso-<uuid>` cuja gravação já
-- saiu da tabela. Não há id para lhe pendurar uma linha, e descobri-la exigia
-- varrer o disco e apagar por ausência — a abordagem medida e revertida (ver o
-- `sweep_orphan_segments`). Essas são uma limpeza operacional de uma vez, com
-- olhos humanos no volume, não código.
INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
SELECT id, 'iso_tracks', 'iso-' || id::text, TRUE FROM recordings
ON CONFLICT (path) DO NOTHING;

-- Os recortes: o `.webm` existe quando o render acabou (`filename` escrito), e
-- o `.vtt` quando se pediu legendas a um recorte que ficou pronto.
INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
SELECT recording_id, 'clip', 'clip-' || id::text || '.webm', FALSE
  FROM recording_clips WHERE filename IS NOT NULL
ON CONFLICT (path) DO NOTHING;

INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
SELECT recording_id, 'clip_captions', 'clip-' || id::text || '.vtt', FALSE
  FROM recording_clips WHERE captions_lang IS NOT NULL AND status = 'ready'
ON CONFLICT (path) DO NOTHING;
