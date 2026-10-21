# Sprint consolidado — Delonix Meet, 2026-10-20

Estado medido nesta máquina a 2026-10-20, ao consolidar **seis sessões abertas**
e **quatro frentes de trabalho** que só existiam em disco local. Este ficheiro
substitui os handoffs dispersos como ponto único de retoma.

Base: a `develop` do remoto do Meet @ `765be1307` (PR #378). O nome do remoto
nesta máquina está no script da §6 — não se escreve aqui porque o
`check-docs-drift.sh` recusa, com razão, qualquer `delonix-meet-*` citado num
documento que não seja uma skill ou um agente.

---

## 1. O que estava aberto, e em que estado

As quatro frentes estavam **todas limpas sobre a `develop`** — zero commits
atrás, sem rebase a fazer. Nenhuma estava no remoto.

| Frente | Ramo | Commits | Contrato | Veredicto |
|---|---|---|---|---|
| Moldura de transmissão | `integra/cena-de-marca` | 9 | 2/2 ✅ | Entregue e medido por píxeis (33 verificações, 8 mutações recusadas) |
| Tecto de admissão | `integra/tecto-nao-falha-aberto` | 6 | 3/3 ✅ | Entregue; três defeitos pré-existentes apanhados pelo caminho |
| Gravação ISO (`A-02` fatia 1) | `integra/gravacao-iso` | 4 | 3/3 ✅ | Entregue; registo de artefactos (migração 0126) fecha a janela do processo morto |
| Soberania do ASR | `integra/legendas-soberanas` | 2 (1 própria) | 3/3 ✅ | Entregue; recuo passa a Whisper WASM, Web Speech exige três «sins» |
| Registo por `enum Kind` | `integra/gravacao-iso-registo-kind` | 1 | **0/3** ⏳ | **Por acabar.** Commitado como WIP nesta consolidação para sair da máquina |

«Medição contra os 3 grandes» (`integra/paridade-medida`) **já estava fundida** —
zero commits à frente. A fila de lacunas que ela produziu vive em
[`.claude/paridade/INDEX.md`](../.claude/paridade/INDEX.md) e é a fonte da §5.

---

## 2. A colisão que esta consolidação resolveu

Duas migrações `0125`, e **de propósito**: a sessão do ASR escolheu o número já
usado pela sessão do `A-02` para que o `sqlx` falhasse **alto** no arranque, em
vez de aplicar uma delas fora de ordem em silêncio. O handoff deixou escrito que
quem fundisse em segundo renumerava.

```
0125_recording_tracks_screen_audio.sql   (A-02)        fica
0125_org_allow_web_speech.sql            (legendas)    →  0127
```

As **seis** referências em prosa que apontavam à migração das legendas
acompanharam (`org.rs`, `api.ts`, `SettingsCard.tsx`,
`legendas_politica_do_motor.rs`, `bff.json`, `paridade-medida`). As que apontam
à da gravação **não** foram tocadas, e os `0.0125` do `money.rs` e do
`trunkForm.test.ts` tão-pouco: a substituição foi ancorada em `migração 0125` e
medida nos dois sentidos. `HARNESS.md` passou a `0001–0127`; a contagem da skill
`delonix-meet` passou a 127, que é o que o comando dela devolve.

**Medido:** `ls server/migrations | grep -c '^0125'` = **1**, total **127**,
última `0127_org_allow_web_speech.sql`.

---

## 3. O que esta máquina NÃO conseguiu fazer

O `git push` é **recusado pelo hook de segurança desta máquina**,
incondicionalmente. É o mesmo bloqueio que o handoff da sessão «Medição contra
os 3 grandes» já tinha encontrado a 2026-10-18.

Por isso **nenhum ramo local foi apagado** pelo harness: sem envio, eles eram a
única cópia de 21 commits. Os comandos estão na §6.

> **Resolvido a 2026-10-20, pelo dono.** O envio aconteceu e a limpeza correu.
> A `develop` remota está em `7d730e2bb`, igual à local; os cinco ramos
> `integra/*` ficaram no remoto como rede de segurança e saíram todos da
> máquina. Sobram localmente a `develop` e a `main`, mais os ramos dos outros
> produtos, cada um com a sua worktree. As worktrees do Meet passaram de 11
> para 8.
>
> Fica a **promoção** por fazer — ver a §6.

---

## 4. O sprint — ordem de execução

### S0 · Pôr o trabalho no remoto *(bloqueia tudo o resto)*
Correr a §6. Enquanto isto não acontecer, cada fatia abaixo arrisca-se a ser
feita duas vezes em máquinas diferentes.

### S1 · Acabar o `enum Kind` / `reserve`
Ramo `integra/gravacao-iso-registo-kind`, contrato
`.claude/sessions/integra-gravacao-iso--20436200.md`, **0/3**.

Os cinco nomes de artefacto (`{id}.webm`, `{id}.jpg`, `iso-{id}/`,
`clip-{id}.webm`, `clip-{id}.vtt`) vivem em quatro módulos e o registo só
conhece dois. Passam a derivar de um `enum Kind` que carrega o nome, o `is_dir`
e o produtor; o `register(db, …, path)` dá lugar a
`reserve(db, rec_id, Kind) -> Option<PathBuf>`, que insere a linha e só devolve
o caminho se ela existir.

Por fazer: o portão `check-recording-artifacts.sh` ainda lê as constantes
`KIND_*` em vez do `enum` e **não foi visto a recusar**; nem todos os produtores
passaram ao `reserve`; e a evidência da fatia anterior
(`artefactos_de_gravacao` 9/9, `gravacao_iso` 6/6, `recording_clips` 9/9) não foi
**re**medida contra Postgres real depois da troca.

### S2 · Os ajudantes de teste duplicados
`tem_ffmpeg`, `fonte`, `vence_a_retencao`, `recorta` e a listagem do
`recordings_dir` estão copiados por três ficheiros e pertencem a `tests/common`.
É a outra metade da dívida que a revisão do `82ad54d5a` levantou.

### S3 · `C-01` — o disco que ninguém lê
O `${recordings_dir}` do FreeSWITCH escreve `.wav` que **nenhuma linha serve**.
Está nomeado em `scripts/artefactos-de-gravacao.txt`. Agora que o registo de
artefactos existe (migração 0126), ligá-lo é fatia pequena e fecha o defeito
gémeo do que o `A-02` corrigiu.

### S4 · A organização VERDADEIRA da sala
`resolve_tenant` **não sabe** a organização da sala: devolve a primeira
organização do dono por ordem de UUID (`orgs.sort(); orgs.first()`). Para um dono
em duas organizações, a política de soberania que governa a sala é a daquela
cujo UUID for menor — que pode não ser a que a exigiu.

Era aproximação aceitável para a justiça de capacidade (ADR-0017); **com uma
decisão de soberania por cima, deixou de ser.** Está escrito no `signaling.rs`,
por corrigir. A saída é a org na linha da `rooms`.

### S5 · Propagar a política às salas vivas
O `PATCH /api/orgs` escreve a coluna e **não toca nas salas em curso** — a marca
da sala vem da última entrada. Fechar isto é um evento no `pubsub` consumido por
cada nó. A peça existe (`pubsub.rs`); o trabalho não.

### Higiene, a qualquer momento
`storage-truenas/` (de Julho, outro projecto) e `.kimchi/` estão à solta na raiz
do repositório e **não estão ignorados**. Decidir: mover, ignorar, ou apagar.

---

## 5. Depois do sprint — a fila de paridade

Da ordem sugerida em [`.claude/paridade/INDEX.md`](../.claude/paridade/INDEX.md),
com `A-01` fechado e `A-02` na fatia 2:

1. **`B-06`** (entrada na sala é O(n) em série) e **`B-10`** (SCIM trunca acima de
   200) — as duas mais baratas e já medidas; prompt escrito.
2. **`A-02` fatia 2** — composição multi-faixa por cópia (`-itsoffset`,
   ADR-0014 §2.3), com o risco de CPU do `B-05` ao lado.
3. **`A-03`** — push com a app fechada. Sem ele, tudo o resto vale menos.
4. **`A-04`** — *medir* onde a sala quebra, antes de decidir.

Defeitos vizinhos nomeados e por fazer: o socket morto em silêncio do Lobby
(`web/src/pages/Lobby.tsx:76-80` não atribui `onclose`), a âncora absoluta que
falta aos **dois** compositores, e a ausência de uma rota de `DELETE` de gravação
(o registo da 0126 já está pronto para ela).

---

## 6. Comandos para o dono *(o hook recusa-mos)*

Estão num script, não aqui, para serem corridos e não copiados:
[`scripts/consolidado-2026-10-20-enviar.sh`](../scripts/consolidado-2026-10-20-enviar.sh).

```bash
cd /home/walter/workspaces/ngolacloud/delonix-meet

./scripts/consolidado-2026-10-20-enviar.sh enviar      # ✅ feito 2026-10-20
./scripts/consolidado-2026-10-20-enviar.sh confirmar   # ✅ lê os SHA do remoto
./scripts/consolidado-2026-10-20-enviar.sh limpar      # ✅ feito 2026-10-20
./scripts/consolidado-2026-10-20-enviar.sh promover    # ⬅ o que falta
```

O `limpar` **recusa-se a apagar** enquanto o `confirmar` não achar todos os
ramos no remoto — e diz quais faltam. Foi a salvaguarda que impediu os 21
commits de desaparecerem com um `branch -D` dado cedo demais.

### O modo `promover`

Prepara a promoção da `develop` à `main` do Meet — **764 commits**, migrações
`0001-0127` — e **recusa-se** a dar-lhe luz verde em dois casos, ambos vistos a
disparar por código de saída:

1. a `develop` local diverge da remota (havia trabalho por enviar);
2. a `main` tem commits de trabalho que a `develop` não tem (promover perdê-los-ia).

Quando passa, imprime o que a promoção carrega e o `gh pr create` que a abre —
e **não abre a PR nem empurra nada**, de propósito: neste repositório promove-se
por PR `develop` → `main` (ver #176 e #200), e carregar no botão é do dono.

As duas referências que ele compara são sobreponíveis por variável de ambiente
(`PROMOVER_DEV_REF`, `PROMOVER_MAIN_REF`) para que o controlo negativo corra a
função **real** contra uma referência forjada, em vez de medir uma cópia
alterada do script — que seria medir outra coisa.

---

## 7. Prompt para retomar noutra máquina

Copiar tal e qual para uma sessão nova, depois de clonar o repositório e correr
a §6 nesta máquina:

> Vou retomar o Delonix Meet a partir da consolidação de 2026-10-20.
>
> Começa por ler `docs/sprint-consolidado-2026-10-20.md` — é o ponto único de
> retoma e descreve seis sessões e quatro frentes que foram fundidas na
> `develop`. Confirma primeiro que a `develop` do remoto já contém esses 25
> commits (o topo deve ser `fix(consolidado): a confirmação dava a develop por
> boa só porque ela existia`). Se não contiver, **pára e diz-me** — a
> consolidação não saiu da outra máquina e qualquer trabalho aqui duplica-se.
>
> Depois executa o sprint pela ordem da §4, uma fatia por sessão, cada uma com
> o seu contrato em `.claude/sessions/` aberto por `/escopo`:
>
> - **S1** — acabar o `enum Kind`/`reserve` do registo de artefactos. O
>   trabalho começado está no ramo `integra/gravacao-iso-registo-kind`, num
>   commit marcado `WIP(gravação)`, e o contrato é
>   `.claude/sessions/integra-gravacao-iso--20436200.md`, **0/3 critérios, zero
>   medidos**. Lê esse contrato antes de tocar no código: os três critérios já
>   dizem como se prova cada coisa, incluindo que o portão
>   `check-recording-artifacts.sh` tem de ser VISTO a recusar por código de
>   saída.
> - **S2** a **S5** — ver a §4. Não as faças todas de seguida sem me perguntar.
>
> Regras desta casa, que o repositório impõe e eu confirmo: um critério prova-se
> observando comportamento (comando a passar, saída medida), nunca lendo o diff;
> um portão novo só conta depois de o veres **recusar** por código de saída, com
> controlo positivo ao lado para não ser falso positivo; e trabalho que o
> objectivo da sessão não cobre abre sessão nova em vez de alargar o contrato.
>
> Antes de medires o que quer que seja, levanta o Postgres 17 em :5435
> (`delonix compose`) — a bateria de gravação e a da política de legendas falam
> com base real, e sem ele os números não valem nada.
>
> Não empurres nada sem mo dizeres primeiro.

---

## 8. Lições desta consolidação

1. **Um portão correu na pasta errada e deu verde.** O `cargo build --workspace`
   na raiz devolveu **exit 0** com `could not find Cargo.toml` — o workspace vive
   em `server/`. Um portão que não encontra o que mede falha ABERTO; mede-se o
   que ele imprimiu, não o código de saída sozinho.
2. **Renomear por palavra teria corrompido dinheiro.** `0125` aparecia em
   `0.0125` no `money.rs` e no `trunkForm.test.ts`. A âncora foi `migração 0125`,
   com controlo nos dois sentidos — as seis trocadas a zero, as da gravação e as
   do dinheiro intactas.
3. **Uma colisão deliberada é melhor documentação do que um comentário.** Duas
   `0125` param o arranque; uma `0126` silenciosa ter-se-ia aplicado fora de
   ordem sem ninguém ver.
