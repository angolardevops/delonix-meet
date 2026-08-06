# AGENTS.md — Delonix Meet

> **Harness universal de agentes.** Lido por OpenAI Codex CLI, e por qualquer ferramenta de agente que siga a convenção `AGENTS.md`. O ficheiro-mãe é [`HARNESS.md`](HARNESS.md) (contexto completo); para Gemini é [`GEMINI.md`](GEMINI.md). Este ficheiro é o **resumo operacional partilhado** — os três devem manter-se coerentes.

---

## 1. O que é o Delonix Meet

Plataforma de videoconferência **corporativa, self-hosted e SaaS**, feita de raiz para mercados com requisitos de **soberania de dados, conformidade (BNA/LGPD) e self-hosting**. Alternativa séria a Zoom/Teams/Meet para empresas africanas e lusófonas, setor público e organizações que não podem pôr comunicação crítica em cloud estrangeira.

Princípios: **self-hosted first** · **security by design** · **enterprise sem lock-in** · **backend 100% Rust (sem GC)**.

## 2. Stack (estado real)

**Backend** `server/` (Rust): axum 0.7 (HTTP), webrtc-rs (SFU: DTLS/SRTP, simulcast, RTP fan-out), sqlx 0.7 (Postgres, migrações auto), tokio, argon2, jsonwebtoken (JWT access 15 min + refresh 30 d rotativo), reqwest 0.12 **rustls-tls** (NÃO 0.13). Redis pub/sub para multi-nó (presença + sinalização).

**Frontend** `web/src/` (React + TS + Vite): `webrtc.ts` (SfuCall), `signaling.ts` (WS `/ws`), `presence.ts` (WS `/rtc`), `e2ee.ts` (Insertable Streams AES-256-GCM), `media.ts` (efeitos de fundo RVM ONNX, Transcriber, MeetingRecorder), `pages/Room.tsx` (sala Meet-style).

**Infra**: Postgres (5435 dev), Redis (6379), coturn (3478/5349). Deploy: systemd + nginx (single-node) **e** Kubernetes (`deploy/k8s/`, ingress-nginx + cert-manager). Ver [`docs/reference/architecture.md`](docs/reference/architecture.md) para o mapa completo.

## 3. Invariantes de segurança (NUNCA quebrar)

1. **Segredos fail-closed**: `config.rs` faz panic sem `JWT_SECRET`/`TURN_SECRET`/`DATABASE_URL` fortes. `DELONIX_ALLOW_INSECURE=1` só em dev.
2. **Isolamento multi-tenant**: `rooms::can_access_room` / `room_access` e `org::*` escopam TUDO à(s) org(s) do utilizador. Nunca devolver dados cross-org.
3. **Room tokens curtos**: JWT separado, âmbito = 1 sala, expira em minutos. Sem token válido → WS recusado.
4. **SSRF em webhooks**: validar host (bloquear privados/loopback/link-local/metadata) na criação E na entrega. Sem redirects.
5. **Rate limit**: lockout de login por conta; rate limit por IP em `/api/v1`; WS com token bucket por socket (600 burst / 300 sustained — tolera a rajada de ICE).
6. **Cookie `dlx_refresh`** sempre `HttpOnly; SameSite=Strict; Secure` (exceto `COOKIE_INSECURE=1` em dev HTTP).
7. **E2EE real**: chave AES-256 gerada no cliente, nunca vai ao servidor exceto para gravação (key delegation explícita com confirm()).
8. **Validação no servidor**: host controls (lock/share-only/kick/promote-admit) validados em `signaling.rs`, nunca confiados no cliente.
9. **Uma conta tem UMA autoridade de autenticação, explícita**: `users.odoo_org_id` — a org que a gere, gravada quando a conta nasce de um Odoo e nunca reescrita por outra. NULL = conta local. Nunca resolver o provedor de autenticação por email nem por pertença a org (`LIMIT 1` sem ordem = autoridade por sorteio), e uma sincronização de directório nunca reclama uma conta que já existe — nem de outra org, nem local. Ver R25.

## 4. Arquitetura — decisões não óbvias

- **SFU próprio** (`sfu.rs`), não LiveKit/mediasoup — binário único, controlo total do pipeline RTP para E2EE + gravação side-car. Custo: simulcast/congestion à mão.
- **SFU é in-memory por pod.** O Redis propaga **sinalização/presença**, NÃO RTP. Em multi-réplica, TODOS os pares de uma sala têm de cair no MESMO pod → **afinidade por sala** no ingress (`upstream-hash-by: $arg_room`; o cliente envia `/ws?...&room=CODE`). Sem isto: media num só sentido, admissão e partilha de ecrã falham.
- **E2EE + gravação servidor**: key delegation — o anfitrião cede a chave AES-256 no `server-record`; o servidor decifra antes de gravar; a chave fica só em memória.
- **Gravação IVF/OGG → ffmpeg**: `recorder.rs` usa PTS em ms reais do RTP (o `IVFWriter` da lib usa contador de frames — errado, não reverter).
- **Transcrição**: distribuída e controlada pelo anfitrião — o anfitrião liga a Nota AI, TODOS os clientes transcrevem o próprio microfone (STT on-device) e difundem via `transcript`. Motor: Web Speech quando a Google é alcançável, senão **Whisper WASM local** (privado, self-hosted). Preferir local para soberania.

## 5. Workflow

```bash
make dev                       # infra + backend + frontend
bash deploy/run-dev-server.sh  # só backend (ALLOW_INSECURE)
cd server && cargo build --release   # SEMPRE rebuild após migração nova (touch src/main.rs força re-embed)
bash deploy/publish-web.sh     # compila o frontend e publica em /var/www/delonix (nginx)
```

Portas dev: backend `8180`, frontend `5173`, Postgres `5435`, Redis `6379`, coturn `3478`. **HTTPS obrigatório** fora de localhost (câmara/mic/WebRTC exigem contexto seguro). Emails de teste `@teste.local`, limpar no fim.

**K8s**: `deploy/build-images.sh [--push] [REGISTRY] [TAG]` constrói `delonix-{server,web}`. Em kind: `kind load docker-image <img> --name <cluster>` + `kubectl rollout restart`. Ingress precisa da afinidade por sala (ver §4).

## 6. Padrões de código

**Rust**: `AppError` para todos os erros de handler — nunca `unwrap()` em produção. `sqlx::query!`/`query_as!` (verificação compile-time). Migrações `server/migrations/NNNN_*.sql` sequenciais. Novo módulo → declarar em `main.rs` + registar rotas.

**TS/React**: componentes funcionais + hooks; estado global via Context; mensagens WS tipadas (discriminant union); tokens CSS via custom properties (nunca hardcode de cor); i18n `useTranslation()`. **Nunca `var()` para dimensões de tiles** (transições congelam em background) — dimensões inline por tile. **Controlos novos = kit `web/src/components/ui.tsx`** (`Btn`/`IconBtn`/`Card`/`Field`/`SelectCtl`/`Switch`); zero `border-radius`/`height` hardcoded (tokens 4/6/8px + `--ctl-h` 30px); temas = mapas em `styles/tokens.scss` sob `[data-theme=…]` — ver `docs/reference/design-system.md`. **Camada CONSOLA (27/07)** no fim de `styles.scss`: densidade via `html { font-size: 15px }`, rail de navegação escuro nos DOIS temas (tokens `--sb-*`), `.app-bar` no topo do conteúdo (a Home já não duplica «Nova reunião»/código), páginas de altura total usam `height: 100%` e não `100vh`.

## 7. Painel de revisores

**Subagentes autónomos** em `agents/` (invocar via Agent/`@`) — especialistas Delonix, cada um com o catálogo de regressões no radar:

| Agente | Domínio | Invocar para |
|---|---|---|
| **delonix-code** | Rust supremo (nível criador): safety, ownership, async Tokio, perf hot-path | `server/src/*.rs`, sobretudo `sfu.rs`/`recorder.rs`/`signaling.rs` |
| **delonix-devops** | Platform eng.: K8s, Docker, Ansible, Terraform, coturn/rede, afinidade, media | `deploy/`, `deploy/k8s/`, Dockerfiles, ingress, TURN |
| **delonix-frontend** | Frontend supremo: React/TS/CSS4/HTML5/JS + UX Meet/Teams/Zoom | `web/src/**`, `Room.tsx`, `webrtc.ts`, `styles/` |
| **delonix-security-compliance** | Segurança (cripto/E2EE/auth/SSRF) + compliance (eDiscovery/DLP/SCIM/BNA/LGPD) | `auth.rs`, `e2ee.ts`, `webhooks.rs`, `config.rs`, endpoints novos |
| **webrtc-sfu-reviewer** | WebRTC/SFU (Justin Uberti): ICE, simulcast, codecs, media num-só-sentido | `sfu.rs`, `webrtc.ts`, `e2ee.ts`, `recorder.rs` |
| **competitive-strategist** | Posicionamento vs Zoom/Teams/Meet, priorização de roadmap | features novas, decisões de produto |

Personas adicionais (invocar em prompt quando útil): **Lars Bak** (WASM/Worker — `whisperWorker.ts`), **Zoom Reliability Architect** (redes degradadas, fallback TURN). Perfis completos em [`docs/ai-reviewers.md`](docs/ai-reviewers.md).

## 8. Gotchas conhecidos

- **Vite proxy** `/ws` e `/rtc` precisam de `ws: true` — reiniciar Vite após mudanças. `vite.config.ts` só lê certos de dev no `serve` (nunca no `build`, senão a imagem Docker falha).
- **Rebuild release** após migração; **não** atualizar reqwest para 0.13.
- **Web Speech** só Chrome/Edge e envia áudio à Google — fallback automático para Whisper WASM local.
- **K8s multi-réplica** exige afinidade por sala (ver §4) — foi a causa de media num só sentido. `/ws` precisa de Service DEDICADO (`delonix-server-ws`), senão o ingress descarta o `upstream-hash-by`.
- **Oferta SFU no construtor** da `SfuCall` (não gateada por `joined`) e **convidado em espera não monta a `SfuCall`** — senão media morta / reload em loop após admitir.
- **Media K8s = relay-only** (`FORCE_TURN_RELAY=1` + coturn alcançável), senão o ICE liga mas fica preto. Em local não ligar.
- **`.dockerignore` nunca exclui `web/dist`** (o `Dockerfile.web.stage` copia-o).
- **mkcert em Firefox**: o Firefox não confia na store do SO — `security.enterprise_roots.enabled=true` ou importar a rootCA.
- **Glare = duas metades:** servidor adia a oferta E cliente re-oferta após rollback (senão o ecrã desaparece na mesma). Ver R13.
- **Negociação SFU:** ofertas do cliente E do servidor passam pelo canal único `NegoMsg`/`negotiation_loop` — o webrtc-rs não tem rollback, uma oferta do cliente em glare é ADIADA, nunca aplicada fora de estado (R13).
- **Sem PLI periódico** — keyframes só a pedido/reencaminhados do subscritor (R14). **`touch_subs()`** a seguir a qualquer alteração de subscritores (R16).
- **Áudio remoto no `AudioSink`, nunca dentro de um tile** — esconder um tile não pode silenciar ninguém (R19).
- **Top-N de oradores:** renumerar o áudio, decair por tempo (DTX!), nunca suprimir sem extensão RFC 6464; gravação/PSTN recebem tudo (R22). **`video-interest`** enviado sempre, com "todos" quando não pagina (R23).
- **Regressões completas (não reintroduzir):** [`docs/reference/regressions.md`](docs/reference/regressions.md) — R1–R24 com sintoma/causa/regra/ficheiros.

## 9. Próximas prioridades

SSO OIDC genérico · App mobile Flutter (CallKit/ConnectionService) · PSTN dial-in (FreeSWITCH+Kamailio) · i18n completo · SCIM · Whisper server-side (diarização por track) · MLS para E2EE em grupo · SDK público.
