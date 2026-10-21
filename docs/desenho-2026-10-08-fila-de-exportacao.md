# Desenho — a fila de exportação no servidor

**Data:** 2026-10-08. **Medido contra** a `develop` com os trabalhos 1 a 5 da
frente 1 fundidos ou em revisão.
**Trabalho nº6** do [levantamento](levantamento-2026-10-07-trabalho-assincrono.md).
**Sem código:** este documento é o passo de medição, como no
[nº2](desenho-2026-10-07-uma-fila-so.md). A decisão do que entra é do dono.

## 1. O nº6 não é o que o levantamento parecia dizer

O levantamento listou-o entre os seis trabalhos de **durabilidade**, com a
citação do `ExportsPanel.tsx:10` — «o que NÃO está, porque é do servidor e o
servidor ainda não o tem: a fila de transcodificação partilhada e o seu
nó/CPU». Isso está certo, mas a natureza é outra:

| | Trabalhos 1 a 5 | Trabalho 6 |
|---|---|---|
| O que fazem | **tiram** um defeito que perdia trabalho já existente | **acrescenta** uma capacidade que o produto não tem |
| Prova | reiniciar e o trabalho sobreviver | uma exportação correr no servidor |
| Raio | backend | produto: armazenamento, quota, DLP, retenção, UI |

**O que a medição mostrou, e que é o nó:** as fontes dos projectos de edição
são `Blob` **no IndexedDB do browser**, e por desenho —
`web/src/studio/edit/bd.ts` di-lo: «`fontes`: os BLOBS, escritos UMA vez e
nunca alterados (é isso que faz o projecto não destrutivo)». A tabela
`studio_sources` (migração 0081) **não guarda media**: é o registo dos
telemóveis emparelhados como câmaras (modelo, plataforma, tally, ligado).

Logo o servidor **não tem o que transcodificar**. A fila é a fatia fácil; o
que falta é o material chegar lá.

## 2. As três fatias, por ordem de dependência

### Fatia A — as fontes chegam ao servidor

A que decide se as outras duas são possíveis, e a que tem o raio maior.

| Questão | O que já existe | O que falta |
|---|---|---|
| Caminho de upload | `recordings::upload` com `MAX_RECORDING_BYTES = 512 MiB` e `DefaultBodyLimit` por rota (`lib.rs:667`) | um recurso próprio: um projecto tem **N** fontes, e 512 MiB por corpo não serve uma aula de 40 min em várias pistas → **upload em partes** (ou `multipart` com retoma) |
| Quota | `usage::enforce_recording_quota` | decidir se as fontes contam para a MESMA quota das gravações. Se contarem, uma exportação pode falhar por quota a meio de um projecto já enviado |
| DLP | `dlp::censor` no texto (transcrições, actas) | **não há DLP de media**. Uma fonte enviada é media opaca: o que entra, entra |
| Retenção | `recorder::retention_sweep` por organização | as fontes são material **de trabalho**, não um produto final — a retenção delas é outra regra, e sem ela o volume cresce sem fim |
| Armazenamento | `objectos.rs` (MinIO/S3) existe e está **provado** contra um MinIO real (`objectos_minio.rs`), mas **não está ligado ao gravador** | é aqui que as fontes deviam ir, não para o disco do pod — e ligá-lo é um trabalho em si (está anotado no ADR-0020) |

**O risco a nomear:** sem a Fatia A, qualquer fila de exportação no servidor só
sabe exportar o que já está na biblioteca — ou seja gravações, não projectos.
Isso é útil, mas **não é o que o `ExportsPanel` declara em falta**.

### Fatia B — a fila de transcodificação

**É a fatia fácil, e já está desenhada.** A peça do nº2 serve tal e qual:

- `Queue` numa tabela `studio_exports` (pedido, predefinição, projecto, estado),
  com `tenant_column: Some("org_id")` — a justiça entre organizações é o que
  impede uma org a exportar dez projectos de ocupar o CPU de todas;
- `Retry` com tecto e backoff; `Lease` **curta e renovada**, como a composição
  (R309) — um transcode é longo e a reserva não se dimensiona pelo pior caso;
- o runner (`jobs::Filas`) com `CancellationToken`, e as vagas repartidas por
  inquilino com o `fair_slots` que a composição já usa;
- o progresso lido do `-progress` do ffmpeg, como o `recorder::spawn_progress_writer`.

O que ela acrescenta ao que existe: **nada de novo em mecanismo**. É uma
declaração de fila e um `ffmpeg` com os argumentos da predefinição.

**O que ela NÃO resolve sozinha:** o `ffmpeg_threads` está em **2** por omissão
e o tecto de vagas é partilhado com a composição de gravações. Uma fila de
exportação a sério disputa CPU com as chamadas VIVAS do mesmo pod — por isso o
desenho sério é um **nó de transcodificação separado**, que é o que o
`ExportsPanel` diz («a fila de transcodificação partilhada **e o seu nó/CPU**»).
Pôr isto no pod do SFU é um erro conhecido antes de o cometer.

### Fatia C — o resultado volta

| O que | Como |
|---|---|
| Descarga | o padrão do `recordings::download` (link assinado, `Range`) |
| MinIO | a Fatia A já o obriga; aqui é o destino do produto final |
| Histórico do servidor | hoje o histórico é **deste dispositivo** (`edit/bd.ts`, loja `exportacoes`) — passa a ser da organização |
| «Descarregar todos» | listagem paginada por cursor (`core::page`), nunca um `LIMIT` fixo |

## 3. O que eu recomendo, e porquê

**Não começar pela Fatia B, por mais tentador que seja.** É a única que eu podia
entregar hoje com a peça das filas, e seria trabalho honesto que **não resolve o
que o `ExportsPanel` declara** — exportaria gravações da biblioteca, não
projectos do Estúdio. Entregar a parte fácil de um recurso e chamar-lhe «fila de
exportação no servidor» era o tipo de relato que o
`check-capability-claims.sh` existe para perseguir.

**A ordem honesta é A → B → C**, e a Fatia A precisa de três decisões que não
são minhas:

1. **As fontes contam para a quota de armazenamento da organização?** Se sim, um
   projecto grande pode esgotar a quota das gravações.
2. **Qual é a retenção das fontes?** São material de trabalho; sem regra, o
   volume cresce sem fim.
3. **O transcode corre no pod do SFU ou num nó próprio?** No pod, disputa CPU
   com chamadas vivas. Num nó próprio, é infraestrutura nova.

## 4. O que fica provado deste documento

Nada de comportamento — é medição. O que ele estabelece:

- as fontes **não estão** no servidor, e a tabela que o nome sugeria
  (`studio_sources`) não as guarda;
- a Fatia B **não precisa de mecanismo novo**: a peça do nº2 chega;
- há três decisões de produto a montante, e a maior é onde corre o ffmpeg.

**Fora de âmbito deste documento:** as cenas do Estúdio
(`web/src/studio/cenas.ts`) e os projectos de edição, que o levantamento juntou
ao nº6 e que são outra coisa — estado de UI por sincronizar, do mesmo tipo do
que o #263 fez aos diagramas, e que não depende de nada disto.
