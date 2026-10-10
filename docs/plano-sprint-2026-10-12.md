# Sprint 3 — produção no `ngola-lda`, identidade provada e correio a sair

**Datas:** 2026-10-12 a 2026-10-23 (duas semanas) · **Medido contra:** `origin/develop` @
`80269ea0` (2026-10-10) · **Origem:** consolidação das seis sessões de trabalho abertas no
Meet a 2026-10-10 — produção (ADR-0020/0021), integração Odoo e revisão do SSO, specs
OpenAPI, DelonixPhone e `delonix-push`, Frente 1 (filas e correio), separação
frontoffice/backoffice.

**Objectivo:** `meet.ngolacloud.com` de pé no cluster `delonix-lda`, com login por SSO que
não se deixa tomar e correio do operador a sair.

Este documento não substitui o [plano de lacunas](plano-lacunas-2026-10-04.md) nem o
[plano de continuidade](plano-continuidade-2026-10-06.md): junta numa só fila o que seis
sessões faziam em paralelo sem se verem, e diz onde se tocavam.

## 0. Onde as sessões se tocavam

| Fio comum | Sessões | O que isso obriga |
|---|---|---|
| **Identidade e prova** | SSO (prova de domínio por DNS TXT, A3) · correio (D7: email provado antes do E3) · reposição por administrador (#288, sem botão) | Um só épico, a frente B. A prova de domínio e a prova de endereço são coisas diferentes — uma org provar `empresa.ao` não prova `joao@empresa.ao` — e não se fundem sem decisão |
| **Odoo** | SSO e `nk_delonix_meet` (Kaeso) · validação a dois utilizadores parada à espera de um Odoo · `meeting.mom_ready` consumido pelo Odoo | Um teste de ponta a ponta, a frente C |
| **Produção** | #269 · quota do chart (#280) · `cluster.sh` (#295) · TURN só em `127.0.0.1` · deploy do `web-admin` · relay SMTP · GPU do Ollama | Tudo espera o cluster; a frente A vem primeiro |
| **Filas em Postgres** | correio (`jobs::Queue`) · `finish_stale` sem cron · índice quente `messages_due` do `delonix-push` | Mesmas lições: estado próprio para a posse (`sending`), reserva curta renovada por relógio |
| **Concorrência entre sessões** | três `0105` (#292) · ADR-0025 em duas PRs · SSO com `0106/0107` já ocupadas | Números de migração e de ADR escolhem-se **ao abrir a PR**, contra a `develop` desse momento |

## 1. Dia 0 — fila de fusão e arrumação (2026-10-10)

| Item | Estado |
|---|---|
| #295 `cluster.sh` com as ServiceAccounts | **fundida** |
| #290 prova da cadeia com o `delonix-push` real | **fundida** |
| #296 espinha do correio (ADR-0025, migração 0108) | **fundida** |
| #297 assistência remota | **fundida** como **ADR-0026** (as duas PRs reclamavam o 0025); o título só ficou certo nesta PR |
| #286 catraca do ESLint a zero | **fundida**, depois de juntar a `develop` (63 commits atrás; lint 0, `tsc` limpo, `vitest` 1191/1191, CI 7/7) |
| SSO (C1, C2, A1–A4) no Meet | PR **#299**, de `integra/sso-identidade` (1318 testes, 0 falhas): migrações **0109/0110**, `domain_verify.rs` no HARNESS, as duas rotas novas no `isolamento.mjs` |
| SSO no Kaeso (O-1, O-2) | **empurrado** para `CompllexusDevelopers/Kaeso-Multicompany` (`feat/nk-delonix-meet-port`), sem PR |
| #251 silêncio na pista (80 ms) | **fora do sprint** até decisão |
| Worktrees fundidos | oito destruídos, com as branches locais e remotas |

## 2. Frente A — produção no `ngola-lda` (ADR-0021)

**Decidido (2026-10-10): uma só quota, a do chart**, com a plataforma (CNPG, Redis) somada.
O `00-namespace.yaml` fica com o Namespace e as etiquetas de Pod Security e larga o
`ResourceQuota` e o `LimitRange` — com as duas, o Kubernetes aplica a mais restritiva e o
Postgres (12Gi) é recusado pela do chart (8Gi).

| # | Trabalho | Prova de fecho |
|---|---|---|
| A1 | Rebase da #269 sobre a `develop` e a quota única | `helm template` + `kubectl apply --dry-run=server` no cluster, Postgres `Ready` |
| A2 | PR da branch `plataforma-partilhada` (Gateway API, imagens, CNPG corrigido) | `meet-pg` `Cluster` aceite pelo webhook, `ScheduledBackup` a encontrá-lo |
| A3 | HTTPRoutes para o Envoy (o chart não tem nenhuma) | `curl` ao host público devolve a web |
| A4 | TLS por DNS-01 (o HTTP-01 dá `no route to host`) | certificado emitido por `delonix-letsencrypt` |
| A5 | MinIO e depois a observabilidade | ServiceMonitor com alvos > 0 (`check-observabilidade.sh` sem aviso) |
| A6 | TURN público | dois browsers em redes diferentes com media nos dois sentidos |
| A7 | Deploy do `web-admin` no mesmo host (PR4 do backoffice) | login de operador no browser |
| A8 | Relay SMTP de produção | uma mensagem real entregue — **depende da decisão D3** |

**Prazo escrito:** o `barmanObjectStore` desaparece no CNPG 1.31.0 e o operador é 1.30.1.

## 3. Frente B — identidade

| # | Trabalho | Prova de fecho |
|---|---|---|
| B1 | Fundir o SSO no Meet (`integra/sso-identidade`) | CI 7/7, `isolamento.mjs` com as rotas novas |
| B2 | Fundir o SSO no Kaeso (`feat/nk-delonix-meet-port`) | testes do Odoo 16 verdes no repo |
| B3 | D7 PR2 — `email_verified` | teste: endereço por provar não recebe reposição |
| B4 | D7 PR3 — reposição pela própria pessoa (E3) | teste de 2 orgs; o token só vai a endereço provado |
| B5 | Botão de reposição por administrador na consola | browser: admin repõe, alvo entra |

## 4. Frente C — Odoo de ponta a ponta

Depende de B1, B2 e da decisão D4.

| # | Trabalho | Prova de fecho |
|---|---|---|
| C1 | Login por SSO a partir de um Odoo real | browser: conta do Odoo entra no Meet |
| C2 | Provisão da organização pelo `nk_delonix_meet` | org criada com a config SSO num só pedido |
| C3 | `meeting.mom_ready` a chegar ao Odoo | a acta aparece no registo do Odoo |
| C4 | Validação com dois utilizadores pelo túnel | duas contas, duas máquinas, sala com media |

## 5. Se sobrar tempo

- Cron do `finish_stale` da telefonia (Frente 1, nº5): hoje só corre em handlers de leitura.
- As quatro rotas de pesquisa com contrato errado (`Vec<X>` mas devolvem objecto):
  `/api/meetings`, `/api/org/members`, `/api/whiteboards`, `/api/audit`.
- Índice `messages_due` do `delonix-push` (outro repo): gravações 2–3× mais rápidas sem ele.

## 6. Fora do sprint, com nome

E2 convites por email · SMTP por organização e o guarda de saída em TCP · SSO médios e baixos
(M1–M7, B1–B8) · exportação do Estúdio (nº6, três decisões) · IA de reuniões, fases 2–4 ·
planos/entitlements e ecrãs de FreeSWITCH/Kamailio no backoffice · canal Dart e licença
Belledonne do DelonixPhone (ADR-0022) · #251.

## 7. Decisões

| # | Decisão | Estado |
|---|---|---|
| D1 | Quota única no chart, com a plataforma somada | **sim** (2026-10-10) |
| D2 | #296 fica com o ADR-0025, #297 passa a 0026 | **sim** (2026-10-10) |
| D3 | Servidor SMTP do relay de produção (host, porta, utilizador, remetente) | por dar |
| D4 | URL e base do Odoo para a validação | por dar |
| D5 | Sprint de duas semanas a partir de 2026-10-12 | **sim** (2026-10-10) |

## 8. Como se fecha um item

Medido contra `origin/develop`, nunca contra a árvore local. Cada PR diz o que provou e o que
não validou. Uma tarefa não está terminada enquanto o worktree dela existir.
