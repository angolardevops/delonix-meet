---
name: consolidado
description: >-
  Retoma o trabalho do Delonix Meet consolidado a 2026-10-14 no ramo
  `consolidado/2026-10-14`: o que está lá dentro e provado, o que ficou de
  fora e porquê, as sessões abertas e o passo seguinte de cada uma, e o que
  esta máquina precisa de instalar antes de medir qualquer coisa.
when_to_use: >-
  Quando o utilizador escrever apenas «consolidado» (ou `/consolidado`), numa
  máquina nova ou nesta, para continuar de onde o trabalho ficou. Também
  quando perguntar «onde é que isto ficou», «o que falta», «retoma o
  consolidado». NÃO a uses para desenhar trabalho novo — para isso o ponto de
  entrada é a `delonix-meet`.
---

# Consolidado de 2026-10-14 — retomar o Delonix Meet

## O que isto é

Um ponto único de retoma. A 2026-10-14 havia **20 worktrees vivos, 15 PRs
abertas e 4 lotes de trabalho por commitar** espalhados por uma só máquina. Tudo
o que fundia limpo foi juntado em **`consolidado/2026-10-14`**, empurrado, e o
que não fundia ficou de fora **com a razão escrita**. Esta skill é o mapa.

**Primeiro passo, sempre:** `git fetch origin && git log --oneline -1 origin/consolidado/2026-10-14`.
Se o ramo já não existir, foi fundido — segue para a `develop` e lê a secção
«Depois de fundir».

## Fronteira

- A **`delonix-meet`** é o ponto de entrada por domínio (que skill, que revisor,
  que portão). Esta skill não encaminha domínios: diz só **onde o trabalho ficou**.
- O estado datado de cada frente vive na skill do domínio e nas PRs. Esta não o
  duplica — aponta.

## A base: a `develop` é que manda, a `main` estava 630 commits atrás

Medido a 2026-10-14:

| ramo | estado |
|---|---|
| `origin/main` | `de2b1e8c`, de 2026-10-05 — **630 commits atrás** da develop |
| `origin/develop` | `34144a41` (PR #364) — a branch default, contém a `main` por inteiro |
| `origin/consolidado/2026-10-14` | 52 commits à frente da develop, **682 à frente da main** |

O `git diff origin/develop...origin/main` é **vazio**: a `main` não tem uma
linha que a `develop` não tenha. Os 2 commits que a `main` tem a mais são os
merges #176 e #200 da própria develop. **Por isso nunca se recria a `develop` a
partir da `main`** — apagava 630 commits.

## O que está DENTRO do consolidado (12 merges, por ordem)

| PR | ramo | o que traz |
|---|---|---|
| #354 #360 #361 #362 | `onda-c/fonte-de-media` (cadeia) | a onda A da auditoria do estúdio: a mesa ligada, teclas de efeito e ducking, mesa física por MIDI, ficheiro de vídeo como fonte |
| #363 | `onda-c/falar-com-bastidores` | falar com os bastidores premindo e segurando |
| #357 | `onda-b/codificador-do-directo` | o débito do directo sai do perfil e segue a rede, com WebCodecs |
| #355 | `tv/fase0-guiao` | guião de medição da Fase 0 da captura de TV |
| #350 | `auditoria/estudio-2026-10-14` | a auditoria do estúdio (mesa de som, emissão, paridade com o OBS) |
| #343 | `integra/emissor-nomes` | o emissor de produção não é conflito de nomes; o chart recusa `attach` |
| #366 | `plano/desempenho-producao` | porque é que a IA é 28× mais lenta, e o que se decide |
| #365 | `paridade/chat-retencao` | sem retenção definida, o chat da sala fica (P3 fase 1) |
| — | `ui/correio-polimento` | **salvo**: cartão da fila de saída do correio, nas quatro línguas |
| — | `delonix-meet-devops/laboratorio-na-develop` | **salvo**: `make cluster LAN_IP=<ip>` publica os ramais, e mede que ficam em loopback |
| — | `delonix-meet-devops/laboratorio-develop` | **salvo**: prova de acordar o telefone mais honesta, e `Dockerfile.server.lab` |

Os quatro «salvo» eram trabalho **não commitado** em worktrees de uma só
máquina: cada um foi commitado no seu ramo e empurrado, por isso já não se
perde. **Nenhum deles foi construído nem corrido depois de commitado** — está
dito no corpo de cada commit.

## O que ficou de FORA, e porquê

Três PRs **não fundem** no consolidado. Não é falta de qualidade: são dois
pares que mexem no MESMO ficheiro e cada um funde limpo na develop sozinho —
quem entrar primeiro obriga o outro a rebase. **A decisão de quem cede é do dono.**

| PR | ramo | bate com | ficheiro |
|---|---|---|---|
| #356 | `integra/camara-telefone` | a cadeia onda-c (#362/#363) | `web/src/pages/studio/tv/Fontes.tsx` |
| #352 | `fix/destino-guardado-no-ar` | #357 (`onda-b/codificador-do-directo`) | `web/src/pages/Studio.tsx` |
| #359 | `tv/inventario-captura` | a develop já mudada | `HARNESS.md`, `scripts/check-route-auth.sh` |
| — | `ics/participantes` | **a premissa é falsa** (ver abaixo) | — |

E uma por decisão anterior:

- **#251** `integra/silencio-na-pista` — **rascunho**, com os 80 ms antes do
  silêncio por decidir. Fica rascunho até essa decisão.

- **`ics/participantes`** — esteve dentro e **saiu**, por um teste do próprio
  autor. Fazia a descarga do `.ics` levar TODOS os participantes com esta
  justificação escrita no código: «quem a pede já pode ler a lista pela API, com
  o mesmo guarda». **Medido no CI: é falso** —
  `GET /api/meetings/{id}/invitees` devolve **403 `meeting.not_host`** a um
  convidado. A descarga passaria a ser uma **segunda porta para a lista de
  endereços de toda a gente**, a qualquer convidado. Não é um teste a corrigir:
  é um desenho a decidir — ou o `/invitees` abre ao convidado, ou a descarga
  leva só quem a pede. O ramo continua empurrado, com o teste que documenta o
  problema.

Fora do consolidado mas **empurrado e seguro**: `plano/plano-critico`,
`validacao/estudio-2026-10-14` (28 commits de validação), `estudio/camara-telefone`
(superseded por #356). Os 80+ ramos locais `claude/*`, `worktree-agent-*` e
`integra/*` antigos são **lixo já fundido** — não se consolida nada deles.

## O que esta máquina precisa antes de medir

Nada disto é opcional: sem isto, mede-se o ambiente em vez do código.

1. **`web/node_modules` não traz as fontes Archivo** — o `vite build` falha
   sempre até as instalares com `npm i --no-save` das Archivo. Copiar o
   `node_modules` de outro worktree é mais rápido que `npm ci`.
2. **A bateria de testes precisa de `DATABASE_URL` e `TEST_REDIS_URL`**, corre
   com `--no-fail-fast`, e leva 25–50 min. As bases de teste sobem **no
   `delonix`** (`scripts/motor.sh`), **não no docker**.
3. **`cargo fmt` sem `--manifest-path server/Cargo.toml`** mexe em ficheiros
   alheios e o `make fitness` morre aí. O `make fitness` **não corre** dois
   portões que o CI corre (isolamento-cobertura, browser-antes-do-e2e).
4. **`--test <alvo>` não corre os unitários** (`mod tests` em `server/src` fica
   de fora): corre também `--lib` e os crates antes de empurrar.
5. **Worktrees em `<workspace>/.worktrees/<repo>/<tarefa>`, nunca em `/tmp`** —
   o `/tmp` é esvaziado a cada arranque.
6. **O `gh` pode estar na conta errada**: um 403 em repos `angolardevops` com a
   conta `nukubico` activa resolve-se passando o token da `angolardevops` só no
   comando, sem `gh auth switch`.
7. **Nem a `develop` nem a `main` têm auto-merge armado** — o GitHub recusa.
   «Funde quando verde» fica à espera de um «funde» do dono.
8. **`gh pr checks --watch` sai 0 quando ainda não há checks** («no checks
   reported»): esperar que os checks APAREÇAM e contar os `pass`.

## Sessões abertas a 2026-10-14, e o passo seguinte de cada uma

| frente | onde está | passo seguinte |
|---|---|---|
| **Telemóvel como câmara** (#356) | PR aberta, **em conflito** com a onda-c | decidir quem rebase; falta ler um QR com um telefone a sério |
| **Auditoria do Estúdio, ondas A/B/C** (#350 #354 #357 #360–#363) | dentro do consolidado | **nada foi ouvido nem ligado num browser** — é essa a prova que falta |
| **Destino do directo** (#352) | PR aberta, **em conflito** com #357 | decidir quem rebase |
| **Captura de TV** (#355 #359) | #355 dentro, #359 em conflito | sem capturador HDMI ligado, **nada do percurso de media foi medido** |
| **Chat e retenção** (#365) | dentro do consolidado | fase 1 de 3 do P3 |
| **Desempenho da IA em produção** (#366) | dentro do consolidado | a acta é **impossível neste CPU** (piso ~1 640 s, 0,34 tok/s) até aos 16 GiB do Lenovo |
| **Gravação: silêncio na pista** (#251) | **rascunho** | decidir os 80 ms antes do silêncio |
| **`.ics` com participantes** | `ics/participantes`, **FORA** | decidir se o `/invitees` abre ao convidado ou se a descarga leva só quem a pede |
| **Correio: cartão da fila** | `ui/correio-polimento`, dentro | **nunca foi visto num browser** |
| **Laboratório na LAN** | dois ramos, dentro | o `delonix net ingress publish` liga em **127.0.0.1**, não na LAN: para voz na rede local usa-se o compose |

## Como se retoma noutra máquina

```bash
git clone https://github.com/angolardevops/delonix-meet.git
cd delonix-meet && git fetch origin
# o worktree da retoma — NUNCA em /tmp
git worktree add -b retoma/<tarefa> ../.worktrees/delonix-meet/<tarefa> origin/consolidado/2026-10-14
```

Depois, por esta ordem: (1) lê a `delonix-meet` para saber que skill e que
revisor o teu pedido tem; (2) instala o que a secção «o que esta máquina
precisa» diz; (3) **mede** antes de afirmar — contra `origin/develop`, com
`git show origin/develop:<path>`, nunca contra a árvore local.

## Depois de fundir

O consolidado é um **retrato para a `main`**, não um substituto de fundir na
`develop`. Fundi-lo na `main` **não fecha** as PRs que apontam à `develop`: para
as duas linhas não divergirem, funde-se o consolidado **também na `develop`**, e
só então as PRs lá dentro ficam vazias e podem fechar com a prova escrita.

As três que ficaram de fora (#352, #356, #359) continuam a ser trabalho real por
fundir. O rascunho #251 continua a precisar de uma decisão, não de código.
