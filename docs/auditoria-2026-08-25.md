# Auditoria inicial e matriz de lacunas — Delonix Meet

> **Data:** 2026-08-25 · **Árvore medida:** `762e98e` (branch `feat/console-ui-template`)
> **Método:** tudo o que está aqui foi **medido** contra a árvore, não lido da documentação.
> Onde não foi possível medir, está escrito que não foi — e porquê.

---

## 0. Como ler isto

O relatório tem duas metades e as duas são obrigatórias: **o que ficou provado** e
**o que não foi validado**. Um relatório só com a parte boa é o mesmo relato
desonesto que perseguimos no código.

Três avisos de leitura:

1. **«Existe» ≠ «é produto».** Uma capacidade que só existe na CLI, num stub, ou
   num ecrã sem servidor por trás, aparece aqui como lacuna, não como feito.
2. **Os números são reprodutíveis.** Cada um tem o comando ao lado.
3. **A auditoria é da árvore local**, que está 15 commits à frente da
   `origin/main`. A `origin/main` NÃO tem a migração 0033 nem a correcção de
   autoridade de autenticação (R25).

---

## 1. Linha de base medida

| Medida | Valor | Como se obteve |
|---|---|---|
| Backend Rust | 17 606 linhas, 33 módulos | `wc -l server/src/*.rs` |
| Frontend TS/React | 16 978 linhas, 46 ficheiros | `wc -l web/src/**` |
| Migrações | 0001–0033, contíguas | `ls server/migrations` |
| `cargo test --release` | **46 passados, 0 falhados, 4 ignorados** | medido antes de mexer |
| `tsc --noEmit` | limpo | medido |
| `vitest` | 14 passados (3 ficheiros) | medido |
| Fitness functions | 3 (docs, afinidade, RLS) — a 3.ª faz *skip* sem cluster | medido |
| **CI** | **nenhum** — `.github/workflows/` não existia | medido |
| Avisos de clippy | 32 | `cargo clippy --all-targets --all-features` |
| `cargo fmt --check` | **falhava** (árvore não formatada) | medido |
| Avisos npm altos/críticos | **7** (6 altos + 1 crítico) | `npm audit` |
| Avisos RustSec | **1 vulnerabilidade + 3 avisos** | `cargo audit` (424 crates) |
| Testes E2E de browser | **0** | não existe Playwright/Selenium |
| Filas de saída ilimitadas | **5 de 5** | `grep -rn unbounded server/src` |

**O que isto diz.** O pipeline não estava partido — estava **estreito**. O que
existia passava; o problema era o pouco que existia. Nada corria
automaticamente, e as duas maiores classes de risco (saturação de filas e
dependências vulneráveis) não tinham medição nenhuma.

---

## 2. Matriz de lacunas

Legenda do estado: **✅ real** (implementado, integrado, autorizado, testado) ·
**🟡 parcial** (existe, mas falta-lhe uma metade que o torna produto) ·
**🔴 ausente** (não existe código) · **📄 só documentado** (a doc afirma, o código não).

### Programa I — Confiabilidade e prontidão

| Capacidade | Estado comprovado | Lacuna | Risco | Teste de aceitação |
|---|---|---|---|---|
| CI (Rust/TS/SQL/manifests) | 🔴 → ✅ **feito nesta sessão** | — | — | `make test` exit 0 num worktree limpo |
| `cargo fmt` / clippy / audit / SBOM | 🔴 → ✅ **feito nesta sessão** | dívida herdada escrita e travada por catraca | — | catracas vistas a falhar |
| Filas limitadas + backpressure | 🔴 → ✅ **feito nesta sessão** | — | — | 7 testes novos (R32/R33) |
| Nunca bloquear o executor Tokio | 🟡 **melhorado nesta sessão** | `BufWriter` de 64 KiB corta as syscalls de uma-por-pacote-RTP para uma-por-64-KiB, mas a escrita **continua síncrona no executor**. A correcção completa (thread dedicada + fila limitada por track) fica por fazer: mexe no caminho de fecho, onde errar dá gravação truncada em silêncio, e não há ffmpeg nem media real nesta máquina para a validar | Médio (era Alto) | gravar com o volume saturado e medir latência de RTP |
| `ffmpeg` fora do executor + limites | 🟡 → ✅ **feito nesta sessão** | timeout (`FFMPEG_TIMEOUT_SECS`), `-threads` (`FFMPEG_THREADS`), `-nostdin`, `kill_on_drop`, e o processo é morto e colhido ao exceder. **Falta sandbox e limite de memória** (precisa de cgroups/setrlimit) | Baixo (era Alto) | 3 testes em `run_bounded` (acaba a tempo / estoura o tecto / falha) |
| Máquina de estados de chamada | 🔴 | não existe. Não há `connecting/degraded/reconnecting/recovering/failed` | Alto | estado observável em cada transição |
| **ICE restart** | 🔴 | **zero ocorrências de `restartIce()` no frontend** | **Crítico** — a recuperação de rede depende de reload da página (`Room.tsx:853`, `dx_reconnect_at`) | cortar a rede 10 s e a chamada volta sem reload |
| Reconnect token / silent rejoin | 🔴 | não existe | Alto | refresh do browser não perde a sessão de media |
| Backoff exponencial | 🟡 | existe só no `/rtc` (`presence.ts:93`, 2 s→30 s) e **sem jitter**; o `/ws` não tem | Médio | N clientes a reentrar não sincronizam |
| HA do SFU (registry, drain, placement) | 🔴 | há afinidade por sala (ADR-0001) — que **não é HA**. Morrer o pod mata as salas | **Crítico** | `kubectl delete pod` e as salas recuperam noutro nó |
| Testes de fiabilidade com browsers reais | 🔴 | 0 testes E2E; os `sfu_e2e` usam `RTCPeerConnection` de servidor, não browsers | Alto | matriz 1:1/5/10/25/50 |

### Programa II — Qualidade de áudio e vídeo

| Capacidade | Estado comprovado | Lacuna | Risco |
|---|---|---|---|
| Simulcast 3 camadas (q/h/f) | ✅ | — | — |
| Selecção de camada adaptativa | 🟡 | `wanted_rid(kind, room_size, shift)` (`sfu.rs:283`) usa **só** tamanho da sala e um *shift* por perda. **Não** entra: tamanho do tile, orador activo, palco, pin, aba em background, RTT, jitter, CPU, bateria, data-saver, preferência | **Alto** — desperdício de banda na rede-alvo | 
| `video-interest` (não enviar a tiles invisíveis) | ✅ | binário (subscrever/não), não é sugestão de qualidade | — |
| Suspender vídeo em background preservando áudio | 🔴 | não existe | Médio |
| Perfis de qualidade nomeados | 🔴 | não há audio-only / data-saver / 180p…1080p / screen-texto vs movimento | Médio |
| Pipeline de áudio com IA no browser | 🟡 | `@sapphi-red/web-noise-suppressor` está nas dependências; **não há** de-reverberação, voice isolation, VAD, normalização, limiter, nem perfis reunião/aula/podcast/música | Médio |
| Delonix Call Quality Score (0–100) | 🔴 | não existe | Médio |
| Métricas de chamada | 🟡 | recolhe-se **3** (`rtt_ms`, `loss_pct`, `up_kbps` — migração 0025). Das ~25 pedidas faltam NACK, PLI, FIR, frames descartados, freeze duration, audio concealment, time-to-first-audio/video, join time, par de candidatos, uso de TURN, reconexões | Alto — sem isto não há SLO defensável |
| Observabilidade do SFU (Prometheus) | ✅ | boa: 13 contadores + 4 novos de filas | — |
| Testes em rede degradada (emulação) | 🔴 | nenhum | **Alto** — é o mercado-alvo |

### Programa III — Enterprise, IAM, governance

| Capacidade | Estado comprovado | Lacuna | Risco |
|---|---|---|---|
| OIDC genérico | 🟡 | `openidconnect` 4.0.1 com descoberta + código+PKCE (`auth.rs:588-720`) | Médio |
| Login com conta Odoo | ✅ | `odoo_sso.rs`, org nasce da empresa, directório sincroniza | — |
| SAML 2.0 | 🔴 | **zero código** (a palavra só aparece em `i18n.ts`) | Alto |
| SCIM 2.0 | 📄 | **zero código**; existe *postura* no ecrã de Analytics | **Alto** — o ecrã sugere uma capacidade que não existe |
| MFA / WebAuthn / passkeys / TOTP | 🔴 | **zero ocorrências** de qualquer um dos termos | **Crítico** para enterprise |
| RBAC | 🟡 | **2 papéis**: `admin` \| `member` (`org.rs:431`). O pedido são **15** (platform admin, security admin, compliance officer, co-host, presenter, recording editor, auditor read-only, …) | Alto |
| Autorização validada no servidor | ✅ | host controls em `signaling.rs`, não confiados no cliente | — |
| Isolamento multi-tenant | ✅ | `can_access_room`/`org::*`; RLS em `employee_groups` com fitness function | — |
| Auditoria | 🟡 | `audit.rs` escreve *best-effort* nos eventos-chave; **não é imutável** | Médio |
| Retenção | ✅ | *sweep* de retenção em `main.rs` | — |
| DLP | ✅ | `dlp.rs` (NIF, cartão, chaves de API, profanidade) com testes | — |
| Legal hold / eDiscovery / exportação de evidências | 🔴 | **zero** | Alto |
| Customer-managed keys / bloqueio geográfico | 🔴 | zero | Médio |
| Webhooks + guarda SSRF | ✅ | testado (`blocks_internal_ranges`, allowlist) | — |
| API keys por org | ✅ | hash + scopes | — |

### Programa IV — Produto completo

| Capacidade | Estado | Nota medida |
|---|---|---|
| PWA | ✅ | manifest + service worker |
| Desktop / Android / iOS / CallKit | 🔴 | zero |
| Webinar / town hall / registo / backstage / Q&A moderado | 🔴 | **zero** (`webinar`, `town hall`, `rtmp` = 0 ocorrências). Existe Q&A **de reunião**, não de evento |
| Streaming RTMP / CDN | 🔴 | zero |
| PSTN dial-in | 🟡 | `voice.rs` é **control plane apenas** — o próprio cabeçalho diz «a camada de media será ligada nas sub-fases 2/3». Salas de voz, PINs, DIDs e CDRs existem; **a chamada não completa** |
| SIP trunk / DTMF / IVR | 🟡 | API interna de IVR exposta, sem media por trás |
| Chat persistente / canais / ficheiros / tarefas | 🟡 | chat de sala persistido (migração 0018); não há canais nem ficheiros |
| Whiteboard persistente | ✅ | `whiteboards.rs` + estado partilhado em Redis |
| Breakouts, sondagens, Q&A, temporizador, reacções, mão | ✅ | com validação no servidor |
| Transcrição + ata (MoM) + tarefas | ✅ | Web Speech + Whisper WASM local; IA local via Ollama |
| Legendas traduzidas | ✅ | `ai.rs`, fail-open sem `OLLAMA_URL` |
| Pesquisa em gravações/transcrições | 🟡 | viewer mostra transcrição; não há pesquisa |

### Delonix Studio (§7 do mandato)

| Capacidade | Estado |
|---|---|
| **Todo o domínio Studio** | 🔴 **greenfield — zero código.** `grep -riE 'studio\|timeline\|EDL\|edit.decision'` em `server/src` e `web/src` não devolve nada do domínio |
| Gravação server-side | ✅ existe (`recorder.rs`: RTP→IVF/OGG→ffmpeg→webm, PTS reais do RTP) — **é a fundação**, não o Studio |
| Gravação local por participante (tracks isolados) | 🔴 |
| Upload em chunks resumível/idempotente com hash | 🔴 |
| Editor não-linear / EDL não-destrutiva | 🔴 |
| Edição por transcrição | 🔴 |
| AI audio/video enhancement no browser | 🟡 só efeitos de fundo (RVM ONNX + MediaPipe) |
| Armazenamento S3/MinIO | 🔴 — `storage.rs` suporta **só** TrueNAS NFS e Nextcloud WebDAV |
| Exportação por perfis | 🔴 |

---

## 3. Segurança — o que a auditoria de dependências encontrou

Medido, não estimado:

- **`protobufjs` (CRÍTICO)** ← `onnx-proto` ← `onnxruntime-web`. Corre no
  **browser**; o que faz *parse* são os nossos modelos ONNX self-hosted
  (`allowRemoteModels=false`), não input de terceiros — o que baixa muito o
  risco real, mas não o anula.
- **`sharp`, `@xenova/transformers`** — o `sharp` é dependência de **Node** e
  **não entra no bundle do browser**.
- **`postcss`, `nanoid`** — cadeia de **build** (vite), não vão para produção.
- **RUSTSEC-2023-0071** (`rsa` 0.9.10, Marvin Attack, **sem correcção
  publicada**) ← `openidconnect`. O ataque recupera chave **privada** por
  temporização de **decifragem**; nós só **verificamos** tokens com a chave
  **pública** do IdP e não guardamos nenhuma chave privada RSA.

Todos estão registados com a razão em `scripts/dep-audit-accepted.txt` e
`scripts/rustsec-accepted.txt`, e a catraca falha em qualquer aviso **novo**.

Encontrado e corrigido: **uma chave privada TLS seguida no git**
(`deploy/certs/wildcard.delonix.local.key`, mkcert, válida até Out/2028).
**`git rm --cached` não purga o histórico** — a chave continua alcançável em
commits anteriores e deve ser tratada como comprometida.

---

## 4. As três lacunas que mandam na ordem de execução

1. **Não há recuperação de chamada.** Zero `restartIce()`. Numa rede que oscila
   — o caso normal do mercado-alvo — a única recuperação é **recarregar a
   página**. Isto anula, na prática, boa parte do valor do resto.
2. **A afinidade por sala não é HA.** Concentrar as salas no mesmo pod resolve o
   *split-brain* e **agrava** o raio de dano: matar o pod mata as salas todas.
3. **A adaptação de qualidade ignora tudo o que interessa.** Decidir a camada
   por tamanho da sala é o oposto do que a rede-alvo precisa.

---

## 5. O que NÃO foi validado nesta auditoria

Tem de ser dito por inteiro:

- **Nada foi corrido contra infraestrutura real.** Sem Postgres, Redis, coturn
  ou cluster nesta máquina (`psql` e `redis-cli` não existem). Os testes que
  passaram são unitários e de integração **em processo**.
- **`check-tenant-rls.sh` faz *skip*** sem cluster — o isolamento RLS **não foi
  verificado** nesta sessão.
- **Nenhuma medida de rede degradada, carga, ou browser real.** Não há número
  defensável sobre join time, qualidade ou capacidade — e por isso **nenhuma
  meta de SLO do §10 do mandato pode ser publicada ainda**.
- **O CI foi validado localmente** (todos os passos correm e os portões foram
  vistos a falhar), mas **ainda não correu no GitHub Actions**.
- Os 4 testes de `signaling` continuam **ignorados** (semântica do hub evoluiu);
  não foram reescritos.
- A revisão de `sfu.rs`/`recorder.rs` foi **dirigida** (filas, ffmpeg, camadas),
  não uma revisão linha-a-linha dos 2 381 linhas dos dois.

---

## 6. Próxima prioridade

Pela ordem do mandato, e pelo risco medido:

1. **Escrita de ficheiro fora do executor Tokio** — o `ffmpeg` já ficou com
   tecto e travão de CPU nesta sessão; falta mover a escrita IVF/OGG para uma
   thread dedicada com fila limitada por track, **com teste contra uma gravação
   real** (não há ffmpeg nem media nesta máquina).
2. **Máquina de estados de chamada + ICE restart + rejoin silencioso.**
3. **Métricas de chamada completas** — sem elas não há Call Quality Score nem
   SLO com números.
4. **Testes em rede degradada** com emulação, e só depois publicar metas.
5. **HA do SFU** (registry, health/capacity, placement, drain).

Nada de features cosméticas antes destas.
