# Contrato de API — fronteira pública vs interna

> Avaliação de arquitetura (Martin Fowler, ponto #7). O servidor expõe **dois
> regimes de compatibilidade** no mesmo processo. Este documento nomeia a
> fronteira **antes** de o SDK público e o mobile a cristalizarem por acidente.

## Os dois regimes

### 1. BFF interna — `/api/...` (sem versão)
- **O que é:** o *Backend-for-Frontend* do próprio web Delonix (`web/src/api.ts`).
- **Contrato:** **NÃO estável.** Pode mudar a qualquer momento, desde que o frontend
  mude em conjunto (mesmo repositório, deploy acoplado). Não há promessa a terceiros.
- **Auth:** sessão (JWT access no header + refresh em cookie HttpOnly).
- **Exemplos:** `/api/auth/*`, `/api/rooms`, `/api/orgs/{id}/...`, `/api/meetings/*`,
  `/api/recordings/*`, `/api/ice`, `/api/status`, `/health`.

### 2. Superfície pública versionada — `/api/v1/...`
- **O que é:** o contrato estável para consumidores **externos** — SDK público
  (roadmap), app mobile Flutter (roadmap), integrações headless, bots.
- **Contrato:** **estável dentro de `v1`.** Mudanças incompatíveis exigem `v2`.
- **Auth:** **API key** por org (hash + scopes, `apikeys.rs`), com rate-limit
  (`rate_limit::v1_rate_limit`, por IP).
- **Endpoints atuais:** `GET /api/v1/org`, `POST /api/v1/rooms`,
  `GET /api/v1/rooms/{code}`, `POST /api/v1/rooms/{code}/join-bot`,
  `GET /api/v1/recordings`, e o recurso **`meetings`** (`server/src/meetings_v1.rs`):
  `GET/POST /api/v1/meetings`, `PATCH/DELETE /api/v1/meetings/{id}`,
  `GET /api/v1/meetings/{id}/notes`.
- **Salas vs reuniões (não confundir):** `POST /api/v1/rooms` cria uma sala
  solta — sem horário, sem convidados, e cujo dono é quem emitiu a chave (no
  provisionamento, um utilizador de serviço que nunca faz login). Um link assim
  põe toda a gente na sala de espera **sem anfitrião que possa admitir**. Uma
  integração de calendário usa `POST /api/v1/meetings`, que cria reunião + sala
  com `host_email` humano e convidados por email. `/rooms` fica para bots e
  chamadas ad-hoc.
- **Marcador no código:** a fronteira está anotada em `server/src/main.rs` (bloco
  "FRONTEIRA DE CONTRATO DE API"), imediatamente antes do `.nest("/api/v1", ...)`.

## Regras

1. **Um endpoint novo é interno por omissão.** Só entra em `/api/v1` por promoção
   consciente — quando há um consumidor externo real e o contrato foi estabilizado.
2. **Nunca reutilizar um path interno como se fosse público.** Se o SDK precisa de algo
   que já existe internamente, cria-se o equivalente v1 (mesmo que chame o mesmo
   handler por baixo) — a fronteira mantém-se nítida.
3. **`v1` só quebra com `v2`.** Adições retrocompatíveis (campos novos opcionais) são
   permitidas; remoções/renomeações não.
4. **Testes de contrato** vivem com o v1 (a caminho — ponto #2 da avaliação): garantem
   que a forma dos payloads v1 não muda sem intenção.

## Porquê isto importa

O roadmap tem **SDK público** e **mobile Flutter** — ambos consumidores externos que
precisam de um contrato estável. Sem esta fronteira nomeada, cada decisão sobre "isto é
público?" seria *ad-hoc*, e o SDK acabaria a depender de endpoints internos que mudam
sem aviso. Nomear a fronteira agora é barato; desfazer o acoplamento acidental depois
não é.
