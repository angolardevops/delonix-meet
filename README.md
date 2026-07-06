# Delonix Meet

Plataforma de videoconferência self-hosted — alternativa ao Google Meet com backend 100% Rust, WebRTC de ponta a ponta e foco em segurança e performance. Ver [ARCHITECTURE.md](ARCHITECTURE.md) para o desenho do sistema e o plano de fases.

## Estado atual

- ✅ **Fase 1** — backend Rust: autenticação (Argon2id + JWT com refresh rotativo), utilizadores, salas com códigos tipo Meet, sinalização WebSocket com room tokens assinados, rate limiting.
- ✅ **Fase 2 (mesh)** — webapp React: chamadas WebRTC multi-user/multi-room, chat na sala, partilha de ecrã, links partilháveis `#/r/<código>`, modo espectador sem câmara.
- ✅ **Fase 2b (SFU)** — SFU 100% Rust (`webrtc-rs`): escala para salas grandes com um único uplink por participante; topologia escolhida ao criar a sala (SFU por default, mesh P2P opcional). Vídeo 720p@30 com bitrate controlado, câmara local espelhada.
- ✅ **Fase 3** — funcionalidades tipo Meet/Zoom/Teams: **reações** flutuantes, **mão levantada** (com estado no roster), **sala de espera** opcional com admissão/recusa pelo anfitrião, e controlo do anfitrião (**silenciar** e **remover** participantes). Autorização validada no servidor — nunca no cliente.
- ⬜ Fases seguintes: breakout rooms, simulcast no SFU, app mobile Flutter, E2EE, gravação, transcrição + MoM por AI, calendário inteligente.

## Requisitos

- Rust (1.80+), Node 20+, Docker + Docker Compose

## Arrancar em desenvolvimento

```bash
# 1. Infraestrutura (Postgres na porta 5435, Redis, coturn)
docker compose up -d

# 2. Backend (porta 8180; corre as migrações automaticamente)
cd server && cargo run

# 3. Frontend (porta 5173, com proxy para o backend)
cd web && npm install && npm run dev
```

Abre http://localhost:5173.

### Variáveis de ambiente (backend)

| Variável | Default | Descrição |
|---|---|---|
| `DATABASE_URL` | `postgres://delonix:delonix_dev@localhost:5435/delonix_meet` | Postgres |
| `BIND_ADDR` | `0.0.0.0:8180` | endereço do servidor |
| `JWT_SECRET` | (dev) | **obrigatório mudar em produção** |
| `TURN_HOST` / `TURN_SECRET` | `localhost:3478` / (dev) | coturn |

## Testar manualmente uma chamada

1. Abre http://localhost:5173 em **duas janelas do browser** (uma normal, uma anónima).
2. Regista dois utilizadores (um em cada janela).
3. Na primeira: **Criar sala** → copia o código (ex.: `abc-defg-hij`) ou o link `#/r/abc-defg-hij`.
4. Na segunda: **Entrar com código**.
5. Vídeo e áudio ligam automaticamente (mesh WebRTC); testa o chat 💬, o mute 🎙 e a partilha de ecrã 🖥.

Sem câmara/microfone a entrada funciona na mesma em modo espectador (tile com iniciais).

## Testes

```bash
cd server && cargo test    # 14 testes: auth, JWT, rate limiting, hub de sinalização
cd web && npm run build    # typecheck (tsc) + build de produção
```

## Segurança

- Passwords com **Argon2id**; login com verificação de tempo constante contra enumeração de contas.
- JWT de acesso (15 min) + refresh token rotativo e revogável (30 dias, guardado hasheado).
- O WebSocket de sinalização só aceita **room tokens** — JWT âmbito-de-sala com 5 min de validade.
- Credenciais TURN efémeras (HMAC, 1h), nunca estáticas no cliente.
- Rate limiting nos endpoints de autenticação.
- Media encriptado por SRTP/DTLS (nativo do WebRTC) em todas as salas.
- **E2EE opcional por sala** (🔒 na criação): cada frame de áudio/vídeo é cifrado com AES-GCM via Insertable Streams antes de sair do browser — o servidor/SFU só vê bytes cifrados. A chave é derivada de uma frase-chave (PBKDF2, 250k iterações) partilhada fora da plataforma e **nunca é enviada ao servidor**; frames que não autenticam (frase errada/adulteração) são descartados.
