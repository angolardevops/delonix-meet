# Desenho — o kit e o DRY da formatação

**Data:** 2026-10-08. **Medido contra** a `develop` `305d5ee4` (frente 1 fundida).
**Frente 2** das três de 2026-10-07. Fecha o que a
[auditoria de UI de 2026-10-06](auditoria-2026-10-06-ui.md) identificou e não
entrou nas PRs #255–#258.
**Sem código:** este documento é o passo de medição.

## 1. O achado que muda a prioridade

A auditoria tratou isto como **DRY** — código repetido, dívida de arrumação.
A medição mostra outra coisa: **o produto contradiz-se a si mesmo sobre o mesmo
número.**

Há **três** formatadores de bytes, com **três respostas diferentes**:

| Ficheiro | Base | Unidades | 1 500 000 000 bytes dá |
|---|---|---|---|
| `studio/exports/predefinicoes.ts:64` (`tamanhoLegivel`) | **1000** (SI) | GB/MB/KB | **«1,5 GB»** |
| `pages/admin/orgShared.ts:77` (`formatBytes`) | **1024** | B…TB, 0 casas até GB | **«1 GB»** |
| `pages/recordings/format.ts:9` (`formatBytes`) | **1024** | só GB/MB, via `Intl` | **«1,4 GB»** |

A mesma gravação mostra **1,5 GB** no Estúdio, **1 GB** no backoffice do
operador e **1,4 GB** na biblioteca. Quem comprou armazenamento e vê três
números para o mesmo ficheiro não está a olhar para dívida de código: está a
olhar para o produto a mentir-lhe sobre o que lhe cobra.

**Isto sobe a prioridade acima de «arrumação».** E a escolha da base **não é**
minha: 1000 vs 1024 é o que distingue «GB» de «GiB», e a quota da organização
(`usage::enforce_recording_quota`) conta bytes — o número que a UI mostra tem de
ser o mesmo que a quota usa.

**Segunda divergência, menor:** duas `formatDateTime` que formatam diferente —
`{day, month, hour, minute, second}` no backoffice contra
`{dateStyle: medium, timeStyle: short}` na biblioteca.

### O que NÃO é duplicação, e eu quase contei como tal

Uma primeira passagem deu «6 definições de `duracao`». Olhando, **cinco são
variáveis locais** (`const duracao = fim - inicio`) e só
`pages/studio/tv/pecas.tsx:346` é um formatador. O `relogio`
(`studio/captions/legendas.ts:60`) é de legendas (`mm:ss.mmm`) e não compete com
nada. Um `grep` por nome mede nomes, não semântica.

## 2. O kit: o que falta e quanto custa

O `web/src/ui/kit.tsx` tem **24** peças (`Alert`, `Avatar`, `AvatarStack`,
`Button`, `Card`, `Checkbox`, `Dialog`, `Empty`, `Field`, `IconButton`, `Meter`,
`SectionHead`, `Segmented`, `Select`, `Skeleton`, `Spinner`, `StatusBadge`,
`Tabs`, `Tag`, `TextArea`, `TextInput`, `Toggle`, …). Faltam as três que a
auditoria nomeou:

| Peça | Duplicação medida | O que o kit resolve |
|---|---|---|
| **`Table`** | **17 ficheiros** com `<table` à mão | cabeçalho, estado vazio (`Empty` já existe), carregamento (`Skeleton` já existe), e o que nenhuma tem hoje: `scope="col"`, `caption` e ordenação acessível |
| **`Confirm`** | **9** usos de `confirm(` | o `window.confirm` nativo **não é estilizável, não é traduzível e bloqueia o fio** — e este produto tem quatro línguas. O `Dialog` já existe; falta a casca de confirmação |
| **`Copy`** | **7** usos de `clipboard.writeText` | o estado «copiado», o fallback quando a API não existe (http sem TLS, que é o laboratório), e o anúncio a um leitor de ecrã |

**O `Confirm` é o que tem mais valor por linha:** nove sítios onde o produto
fala em inglês do browser a um cliente que escolheu português.

## 3. A catraca do ESLint: 71, e o que a compõe

`scripts/eslint-baseline.txt` = **71**, medido hoje: **19 reportados + 52
silenciados**. A catraca conta as duas coisas, e o comentário do portão explica
porquê — «um disable é um problema escondido, e sem o contar bastava silenciar
para a catraca descer». É honesto e tira-me a saída fácil.

Os 19 reportados, por regra:

| Nº | Regra |
|---|---|
| 5 | `no-useless-assignment` |
| 5 | `react-hooks/exhaustive-deps` |
| 2 | `no-useless-escape` |
| 2 | (sem regra: erros de análise) |
| 1 cada | `no-misleading-character-class`, `preserve-caught-error`, `no-control-regex`, `no-irregular-whitespace`, `no-regex-spaces` |

Os 52 silenciados concentram-se em: `studio/EditPanel.tsx` (5), e depois pares
em `room/use{Transcription,Recording,Prejoin,Layout}.ts`,
`pages/studio/SalaDoEstudio.tsx`, `pages/recordings/recordingMedia.ts`,
`pages/RecordingPlayer.tsx`.

**O que NÃO prometo:** tirar os 52 não é um refactor de arrumação. Cada
`exhaustive-deps` silenciado é uma decisão sobre quando um efeito deve voltar a
correr, e nos ficheiros da SALA (`room/*`) isso é território das regressões
R1/R2 — um efeito a correr de mais reabre uma negociação de WebRTC. **Mexer
nesses 11 sem um teste de dois browsers é arriscar media.**

## 4. A ordem que proponho

Por valor e risco crescente, e cada passo com prova própria:

| # | Trabalho | Prova | Catraca |
|---|---|---|---|
| 1 | **Um formatador de bytes só**, com a base decidida pelo dono, nos três sítios | teste de unidade com os mesmos bytes a dar o MESMO texto nos três caminhos | — |
| 2 | **`Confirm` no kit** + os 9 sítios | os nove em quatro línguas; nenhum `confirm(` fica | — |
| 3 | **`Copy` no kit** + os 7 sítios | o fallback sem `navigator.clipboard` (laboratório em http) | — |
| 4 | **`Table` no kit** + os 17 ficheiros, em lotes | `scope="col"` e `caption` em todos; um ecrã medido com leitor de ecrã | — |
| 5 | **Os 19 reportados**, fora dos ficheiros da sala | catraca 71 → ~52 | ↓ |
| 6 | **Os 52 silenciados**, começando pelos que NÃO são da sala | cada um com a razão escrita, ou o efeito corrigido | ↓ |

**O passo 1 precisa de uma decisão tua** e é a razão de este documento existir
antes do código: **1000 ou 1024?** O número que a UI mostra tem de ser o mesmo
que a quota conta, e hoje são três números diferentes.

## 5. Fora de âmbito

- **Os cinco ecrãs de TV no browser** e o **leitor de ecrã**, que a memória
  junta a esta frente: são recurso e acessibilidade, não DRY.
- **Os 11 `exhaustive-deps` dos ficheiros da sala** (`room/*`), sem um teste de
  dois browsers que prove que a media não parte.
