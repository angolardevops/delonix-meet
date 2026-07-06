# Delonix Meet — Arquitetura

Plataforma de videoconferência self-hosted (alternativa ao Google Meet, com melhorias do Zoom/MS Teams), focada em **segurança** e **performance**. Backend 100% Rust, WebRTC de ponta a ponta, WebApp + app mobile nativa.

## Visão geral do sistema

```
┌─────────────┐   ┌──────────────┐   ┌───────────────┐
│  WebApp     │   │ Mobile       │   │  (futuros     │
│  React+TS   │   │ Flutter      │   │   clientes)   │
└──────┬──────┘   └──────┬───────┘   └───────┬───────┘
       │  HTTPS (REST) + WSS (sinalização) + SRTP/DTLS (media)
       ▼                 ▼                   ▼
┌──────────────────────────────────────────────────────┐
│                 delonix-server (Rust)                │
│  ┌──────────┐ ┌─────────┐ ┌───────────┐ ┌─────────┐  │
│  │ API REST │ │ Auth    │ │ Signaling │ │ SFU     │  │
│  │ (axum)   │ │ JWT/2FA │ │ WebSocket │ │ webrtc  │  │
│  └────┬─────┘ └────┬────┘ └─────┬─────┘ └────┬────┘  │
└───────┼────────────┼────────────┼────────────┼───────┘
        ▼            ▼            ▼            ▼
   ┌─────────┐  ┌────────┐  ┌─────────┐  ┌─────────┐
   │Postgres │  │ Redis  │  │ coturn  │  │ Whisper │
   │(dados)  │  │(pres./ │  │(STUN/   │  │(transcr.│
   │         │  │ pubsub)│  │ TURN)   │  │ Fase 6) │
   └─────────┘  └────────┘  └─────────┘  └─────────┘
```

## Estrutura do monorepo

```
delonix-meet/
├── ARCHITECTURE.md          # este documento
├── README.md                # como correr tudo
├── docker-compose.yml       # postgres, redis, coturn (dev)
├── server/                  # backend Rust (workspace de 1 crate por agora)
│   ├── Cargo.toml
│   ├── migrations/          # migrações sqlx (Postgres)
│   ├── src/
│   │   ├── main.rs          # bootstrap, router, estado
│   │   ├── config.rs        # variáveis de ambiente
│   │   ├── error.rs         # erro unificado da API
│   │   ├── db.rs            # pool Postgres + repositórios
│   │   ├── auth.rs          # registo/login, Argon2, JWT access+refresh
│   │   ├── users.rs         # perfil, contactos
│   │   ├── rooms.rs         # criar/entrar em salas, room tokens assinados
│   │   ├── signaling.rs     # WebSocket: join/offer/answer/ice/leave, chat
│   │   └── rate_limit.rs    # rate limiting por IP/utilizador
│   └── tests/               # testes de integração (sinalização)
├── web/                     # React + TypeScript + Vite
│   └── src/
│       ├── api.ts           # cliente REST (auth, rooms)
│       ├── signaling.ts     # cliente WS tipado
│       ├── webrtc.ts        # gestão de RTCPeerConnection (mesh)
│       └── pages/           # Login, Lobby, Room
└── mobile/                  # Flutter (Fase 4)
```

## Decisões de arquitetura

### Topologia de media: mesh → SFU
- **Fase 2a (agora)**: *mesh* — cada participante liga-se por RTCPeerConnection a cada outro. Simples, E2E encriptado por natureza (SRTP direto entre peers), perfeito para 1:1 e salas até ~6 pessoas.
- **Fase 2b (feito)**: **SFU em Rust** (`webrtc-rs`, [sfu.rs](server/src/sfu.rs)): o servidor termina DTLS/SRTP e encaminha RTP sem descodificar. Um `RTCPeerConnection` por participante; o cliente faz a oferta inicial (publicação) e a partir daí o servidor é o único ofertante (subscrições), com renegociações serializadas por peer — sem glare por construção. `stream_id` = peer_id do publisher para o cliente mapear tracks a participantes. PLI periódico (3s) + PLI imediato para novos subscritores. A topologia é escolhida por sala na criação (`mesh` | `sfu`); simulcast fica para uma iteração seguinte.
- **Fase 5**: E2EE sobre o SFU via Insertable Streams (frame encryption com chaves negociadas entre clientes — o SFU nunca vê media em claro).

### Sinalização
WebSocket em `/ws` autenticado por **room token** (JWT de curta duração assinado pelo servidor, obtido via REST ao entrar na sala — impede room-hijacking). Mensagens JSON tipadas:

| Cliente → Servidor | Servidor → Cliente |
|---|---|
| `join` | `joined` (lista de peers) |
| `offer` / `answer` / `ice` (dirigidas a um peer) | `offer` / `answer` / `ice` (com remetente) |
| `chat` | `chat` |
| `leave` | `peer-joined` / `peer-left` |

O estado das salas ativas vive em memória (`DashMap`) no processo; Redis pub/sub entra quando houver múltiplas instâncias (escala horizontal).

### Autenticação e segurança
- Passwords com **Argon2id**; JWT **access** (15 min) + **refresh** (30 dias, rotativo, revogável — guardado hasheado em Postgres).
- **Room tokens**: JWT separado, âmbito = 1 sala, expiração curta (5 min para ligar o WS).
- Rate limiting por IP nos endpoints de auth; validação estrita de input em todos os handlers.
- TLS terminado por reverse proxy em produção (caddy/nginx); em dev, HTTP local.
- Dados em repouso: volume encriptado + colunas sensíveis cifradas (Fase 5); media E2EE (Fase 5).

### Base de dados (Postgres, migrações sqlx)
- `users` (id, email, username, password_hash Argon2, created_at)
- `refresh_tokens` (hash, user_id, expires_at, revoked)
- `rooms` (id, code curto para convites, name, owner_id, topology, created_at)
- Fases seguintes: `contacts`, `messages`, `meetings` (calendário), `transcripts`, `moms`, `orgs`/`org_members` (multi-tenant).

## Plano de fases

| Fase | Entrega | Estado |
|---|---|---|
| 1 | Backend Rust: auth (Argon2+JWT), users, rooms, sinalização WS, testes | ✅ |
| 2 | WebRTC na web: chamadas 1:1 e mesh multi-user/multi-room, coturn | ✅ (mesh) |
| 2b | SFU `webrtc-rs`: PC por participante, fan-out RTP, renegociação server-driven serializada, PLI periódico, topologia selecionável por sala, **simulcast** (3 camadas q/h/f, seleção por tamanho da sala, troca de camada e fallback) | ✅ |
| 3 | WebApp completa: UI tipo Meet, chat, partilha de ecrã, reações, mão levantada, sala de espera com admissão, mute/kick pelo anfitrião, **breakout rooms** (criar/distribuir/visitar/encerrar, herdam topologia+E2EE) | ✅ |
| 4 | App mobile Flutter tipo WhatsApp: contactos, chamadas voz/vídeo, push, CallKit/ConnectionService | ⬜ |
| 5 | E2EE (Insertable Streams, AES-GCM por frame, chave por frase-chave PBKDF2 que nunca vai ao servidor), gravação ✅, lobby ✅, breakout ✅ | ✅ (MLS ⬜) |
| 6 | Transcrição Whisper self-hosted + diarização, pipeline AI → MoM, calendário inteligente multi-tenant | ⬜ |

Cada fase só avança quando a anterior compila, passa nos testes e é demonstrável manualmente.
