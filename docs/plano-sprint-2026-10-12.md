# Sprint 3 — produção no `ngola-lda`, identidade provada e correio a sair

**Datas:** 2026-10-12 a 2026-10-23 (duas semanas) · **Estado medido a 2026-10-13** contra
`origin/develop` @ `4bf279e1` e contra o cluster `delonix-lda` (só leitura) · **Origem:** consolidação das seis sessões de trabalho abertas no
Meet a 2026-10-10 — produção (ADR-0020/0021), integração Odoo e revisão do SSO, specs
OpenAPI, DelonixPhone e `delonix-push`, Frente 1 (filas e correio), separação
frontoffice/backoffice.

**Objectivo:** `meet.ngolacloud.com` de pé no cluster `delonix-lda`, com login por SSO que
não se deixa tomar e correio do operador a sair.

Este documento não substitui o [plano de lacunas](plano-lacunas-2026-10-04.md) nem o
[plano de continuidade](plano-continuidade-2026-10-06.md): junta numa só fila o que seis
sessões faziam em paralelo sem se verem, e diz onde se tocavam.

## Estado a 2026-10-13 — o resumo

| Frente | Estado | O que falta |
|---|---|---|
| **Dia 0** | **fechado** | — |
| **A — produção** | código **fundido** (A1, A2, A3, A7); **a servir** desde 2026-10-13 em `meet.ngolacloud.com` e `meet.ngolacloud.ao`, só por HTTPS | abrir uma sala num browser; sarar o Postgres; `PUBLIC_URL`, HSTS, imagem nova; A4, A5, A6, A8 |
| **B — identidade** | **fundida** no Meet (B1, B3–B7) | B2: a PR do Kaeso (#346) por fundir |
| **C — Odoo** | C1, C2, C3 **provadas em laboratório** (Odoo 16 + Meet reais) | C4, e repetir contra o Odoo de produção (D4) |

### A produção, medida a 2026-10-13

De manhã a instalação existia e **não servia**: `https://meet.ngolacloud.com/` dava 404, as
HTTPRoutes estavam `Accepted=False` (`NotAllowedByListeners`) e o release Helm `failed`.
À tarde **serve**. O que mudou, por ordem:

| Passo | Onde | O que fez |
|---|---|---|
| O Gateway da plataforma aceita o namespace do Meet | angolardevops/delonix-deploy#224 e #247, aplicadas com `--tags gateway` | `allowedRoutes` dos listeners `http` e `https` passou de `Same` a um selector com `delonix-system` e `ngolacloud-meet`. É o único `192.168.1.73` para onde o router encaminha 80/443 — um Gateway próprio (`own`) ficava noutro endereço, sem entrada da Internet |
| A entrada do Meet cola-se a esse Gateway | #307 | `gateway.mode: attach`; Redis sem Sentinel |
| Os dois domínios | #311 | `extraHosts`: `meet.ngolacloud.com` (canónico) e `meet.ngolacloud.ao` |
| Só HTTPS | #312 | as rotas presas ao listener `https`; antes disso a aplicação respondia 200 em `http://` |

Medido depois do último `helm upgrade` (release `meet`, revisão 3, `deployed`):

| O que se mediu | Valor |
|---|---|
| `https://meet.ngolacloud.com/` e `https://meet.ngolacloud.ao/` | **200**, a aplicação; `/healthz` 200; `/admin/` 200 |
| `http://` nos dois nomes | **301** para `https://` |
| HTTPRoutes `delonix-meet` e `delonix-ws` | `Accepted=True`, os dois nomes, `sectionName: https` |
| a plataforma, antes e depois | `api` e `console` 200, `admin` 403 — iguais |
| `delonix-server` | 3 pods `Running`, sem reinícios (o dos 102 reinícios saiu no rolling) |
| Postgres `meet-pg` | **2 de 3** prontas; primário `meet-pg-2` |
| etiquetas de Pod Security do namespace | **ausentes** |
| quota `delonix-meet-quota` | 24Gi de requests, 16 192Mi usados |
| imagem | `7b992902` — atrás da `develop` |

**Não validado:** uma sala aberta num browser (o `/ws`, a media, o TURN). É o que separa
«a página abre» de «o produto funciona».

**Três defeitos que só a medição apanhou**, e que valem como regra:

1. O template do Gateway rendia bem em Jinja2 simples e mal no Ansible (`trim_blocks`): saía
   sem `apiVersion` e com os dois namespaces numa linha. O teste do role usava o ambiente
   errado. Apanhado a renderizar com o inventário de produção e `kubectl diff` **antes** de
   aplicar.
2. `--tags gateway` sozinho saía 0 sem tocar em nada (`skipping: no hosts matched`). Uma
   tag «própria» só está provada quando corre sozinha.
3. O Meet respondia em claro no `:80`: uma HTTPRoute sem `sectionName` cola-se a todos os
   listeners do Gateway a que se liga.

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
| SSO (C1, C2, A1–A4) no Meet | **fundida** (#299): migrações **0109/0110**, `domain_verify.rs` no HARNESS, as duas rotas novas no `isolamento.mjs` |
| SSO no Kaeso (O-1, O-2) | PR **aberta**: `CompllexusDevelopers/Kaeso-Multicompany#346` |
| #251 silêncio na pista (80 ms) | **fora do sprint** até decisão |
| Worktrees fundidos | oito destruídos, com as branches locais e remotas |

## 2. Frente A — produção no `ngola-lda` (ADR-0021)

**Decidido (2026-10-10, D1): uma só quota, a do chart**, com a plataforma (CNPG, Redis) somada.
O `00-namespace.yaml` fica com o Namespace e as etiquetas de Pod Security e larga o
`ResourceQuota` e o `LimitRange` — com as duas, o Kubernetes aplica a mais restritiva e o
Postgres (12Gi) é recusado pela do chart (8Gi).

| # | Trabalho | Estado (2026-10-13) |
|---|---|---|
| A1 | Rebase da #269 e a quota única | **fundida** (#269). Quota: 12 CPU / 24Gi de requests, 52 / 64Gi de limits, medida no render e nos ficheiros da plataforma (`check-helm.sh`) |
| A2 | A plataforma num cluster com inquilinos | **fundida** (#300). O `Cluster` do CNPG aceite pelo webhook real; depois a #302 corrigiu o `storageClass` |
| A3 | Gateway API no chart | **fundida** (#300) — mas **a rota em produção não está aceite** (ver o topo): falta instalar no modo `own` |
| A4 | TLS por DNS-01 | **por fazer** — falta saber quem gere o DNS de `ngolacloud.com` |
| A5 | MinIO e depois a observabilidade | **por fazer** |
| A6 | TURN público | **em curso noutra sessão**: #308 (o relay do coturn nos manifestos), aberta |
| A7 | `web-admin` no mesmo host | **fundida** (#301): na imagem `web`, em `/admin/`; provada no browser contra um servidor real |
| A8 | Relay SMTP de produção | **por fazer** — depende da D3 |
| A9 | **Sarar a instalação** (novo) | **em parte**: rota aceite e release `deployed` (2026-10-13, deploy#224/#247, #307, #311, #312). **Por fazer**: `meet-pg` 3/3, Pod Security no namespace, o servidor a esperar pela base em vez de morrer, `PUBLIC_URL` (as ligações dos e-mails dependem dela), HSTS, e publicar uma imagem da `develop` |

**Prazo escrito:** o `barmanObjectStore` desaparece no CNPG 1.31.0 e o operador é 1.30.1.

## 3. Frente B — identidade

| # | Trabalho | Estado (2026-10-13) |
|---|---|---|
| B1 | SSO do Meet (C1, C2, A1–A4) | **fundida** (#299) |
| B2 | SSO no Kaeso (O-1, O-2) | PR **aberta** (Kaeso #346), 30/30 testes; falta o parecer do arquitecto, o `pt.po` e a pergunta da allowlist |
| B3 | Prova do endereço de email (D7) | **fundida** (#303): migração 0111, `PUBLIC_URL`, o token só vai no email |
| B4 | Reposição pela própria pessoa (E3) | **fundida** (#304): migração 0112; a mesma resposta para qualquer endereço; só endereços provados |
| B5 | Botão de reposição na consola | **fundida** (#305); provada no browser de ponta a ponta |
| B6 | Achados do SSO vindos do laboratório (novo) | **fundida** (#306): endereço público, IdP em baixo, recusa de volta ao login, auditoria |
| B7 | A provisão pelo operador dá o domínio por provado (novo, decisão D6) | **fundida** (#309), migração 0113 |

Nenhum email saiu de facto: sem a D3 o correio está desligado, e a B3 e a B4 respondem
`mail.disabled`.

## 4. Frente C — Odoo de ponta a ponta

Provada a 2026-10-10 num **laboratório**: um Odoo 16 com os addons da Kaeso #346 e o servidor
do Meet, ambos reais, em bases novas. Falta repeti-la contra o Odoo de produção (D4).

| # | Trabalho | Estado (2026-10-13) |
|---|---|---|
| C1 | Login por SSO a partir de um Odoo real | **provada** no browser: conta do Odoo entra no Meet por JIT. O domínio foi dado por provado à mão (`.test` não tem DNS) — a #309, já fundida, fecha esse passo |
| C2 | Provisão da organização pelo `nk_delonix_meet` | **provada**: org criada com o domínio e a config SSO num só pedido |
| C3 | A acta a chegar ao Odoo | **provada** depois de corrigir o addon: pedia `/notes`, que o Meet renomeou para `/minutes` a 2026-09-17. A acta em si foi simulada na base |
| C4 | Dois utilizadores pelo túnel | **por fazer** — precisa de TURN público (A6) |

Requisito de instalação que o laboratório confirmou: sem `DELONIX_OIDC_MASTER_KEY` no ambiente
do Odoo, o SSO não funciona (falha fechada, por desenho).

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
| D6 | A provisão pelo operador dá o domínio da organização por provado | **sim** (2026-10-10), na #309 |
| D7 | Quem gere o DNS de `ngolacloud.com` (para o DNS-01 da A4) | **deixou de bloquear** (2026-10-13): o certificado é o do Gateway da plataforma, que já cobre os dois nomes |
| D8 | As imagens do MinIO e do Redis só existem de graça em `bitnamilegacy`, sem correcções de segurança: subscrição, outro chart ou imagens próprias | por dar |
| D9 | A allowlist de modelos do Kaeso deve cobrir os `nk.oidc.*` (hoje só cobre o ciclo People) | por dar |
| D10 | A produção serve os dois domínios, com o `.com` canónico | **sim** (2026-10-13), na #311 |
| D11 | Quem é administrador da plataforma (`PLATFORM_ADMIN_USER_IDS`) — sem isso ninguém entra no `/admin/` | por dar |

## 8. Como se fecha um item

Medido contra `origin/develop`, nunca contra a árvore local. Cada PR diz o que provou e o que
não validou. Uma tarefa não está terminada enquanto o worktree dela existir.
