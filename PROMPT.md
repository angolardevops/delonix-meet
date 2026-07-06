# Prompt — Delonix Meet

Copia o texto abaixo e cola numa nova sessão do agentes de IA, na raiz do projeto.

---

Constrói do zero a plataforma **Delonix Meet** — uma alternativa completa ao Google Meet, com as melhores funcionalidades do Zoom e do Microsoft Teams, focada em **segurança e performance**. Trabalha de forma autónoma: planeia a arquitetura, cria a estrutura do monorepo, implementa, testa e documenta cada módulo antes de passar ao seguinte.

## Produto

- **WebApp** (browser, responsivo) e **app mobile nativa** (Android e iOS) com UX inspirada no WhatsApp: lista de conversas, contactos, chamadas de voz e videochamadas 1-para-1 e em grupo, tudo 100% funcional.
- **Multi-room e multi-user**: salas ilimitadas, cada sala suporta dezenas de participantes em simultâneo, com criação de sala por link/código (como o Meet).
- Melhorias herdadas do Zoom/Teams: breakout rooms, levantar a mão, reações, partilha de ecrã, gravação, chat na sala com anexos, fundos desfocados/virtuais, lobby/sala de espera, mute do anfitrião, agendamento de reuniões.

## Transcrição + AI (MoM automático)

- **Transcrição em bruto de cada reunião**: captura o áudio da sala no SFU e transcreve em tempo real (ou pós-reunião) com **Whisper self-hosted** (`whisper.cpp`/`faster-whisper` num serviço próprio), com identificação de quem falou (diarização por track/participante — o SFU já sabe de que participante vem cada stream de áudio) e timestamps. A transcrição bruta fica associada à reunião e é consultável na plataforma.
- **Resumo por AI**: no fim da reunião, um pipeline de AI processa a transcrição bruta e gera um **MoM (Minutes of Meeting)** completo e estruturado: participantes, tópicos discutidos, decisões tomadas, action items com responsável e prazo. O motor de AI deve ser configurável — Claude API (`claude-fable-5` por default) ou LLM local via Ollama — através de variáveis de ambiente.
- **O MoM é guardado automaticamente no evento do calendário** da reunião correspondente e enviado aos participantes (notificação in-app + email opcional). Transcrições e MoM seguem a mesma regra de encriptação em repouso que o resto dos dados.

## Calendário inteligente

- Calendário nativo da plataforma (não usar Google Calendar), com vista dia/semana/mês na web e no mobile, onde se agendam reuniões e onde os MoM ficam anexados ao evento.
- **Agendas isoladas e inteligentes — nunca misturar agendas**: cada utilizador tem a sua agenda pessoal; cada organização/equipa (multi-tenant) tem agendas separadas com permissões próprias. Um utilizador que pertença a várias organizações vê as agendas em camadas/cores distintas, mas os dados nunca se cruzam entre tenants (isolamento a nível de base de dados e de API).
- Funcionalidades inteligentes: deteção de conflitos de horário ao agendar, sugestão automática de slots livres comuns aos convidados (tipo "Scheduling Assistant" do Teams), fusos horários por participante, convites com aceitar/recusar/talvez, e lembretes push antes da reunião com botão de entrar direto na sala.

## Stack técnica (obrigatória)

- **Backend 100% em Rust**: `axum` ou `actix-web` para API/REST + WebSocket de sinalização; **SFU WebRTC** com `webrtc-rs` (ou integração com mediasoup/LiveKit self-hosted se justificares tecnicamente) para escalar multi-user; `tokio` para async.
- **WebRTC 100% funcional de ponta a ponta**: sinalização própria via WebSocket, ICE/STUN/TURN (inclui `coturn` no docker-compose), renegociação, simulcast/SVC para adaptar qualidade por participante.
- **Frontend Web**: React + TypeScript + Vite.
- **Mobile nativo**: Flutter (um só código para Android/iOS) com `flutter_webrtc`, notificações push para chamadas recebidas (ecrã de chamada tipo WhatsApp, com toque, atender/rejeitar), CallKit/ConnectionService.
- **Base de dados**: PostgreSQL + Redis (presença, sessões, pub/sub de sinalização).
- **Infra**: Docker Compose para dev (backend, frontend, Postgres, Redis, coturn, serviço de transcrição Whisper), tudo a arrancar com um só comando.

## Segurança (inegociável)

- **Encriptação de todos os dados**: TLS em todo o transporte, SRTP/DTLS no media, **E2EE nas chamadas** via WebRTC Insertable Streams/frame encryption com troca de chaves tipo MLS/Signal onde possível.
- Dados em repouso encriptados (mensagens, gravações, ficheiros).
- Autenticação com Argon2 para passwords, JWT de curta duração + refresh tokens, 2FA opcional.
- Zero-trust: valida tudo no servidor, rate limiting, proteção contra room-hijacking (tokens de sala assinados e com expiração).

## Método de trabalho

1. Começa por escrever `ARCHITECTURE.md` com o desenho do sistema e o plano de fases.
2. Implementa por fases, cada uma a funcionar e testada antes da seguinte:
   - **Fase 1** — backend Rust: auth, users, rooms, sinalização WebSocket.
   - **Fase 2** — WebRTC funcional na web: chamada 1-para-1, depois SFU multi-user/multi-room.
   - **Fase 3** — webapp completa (UI tipo Meet + chat + partilha de ecrã).
   - **Fase 4** — app mobile Flutter tipo WhatsApp (chamadas normais e vídeo, push, ecrã de chamada).
   - **Fase 5** — E2EE, gravação, breakout rooms, hardening de segurança e performance.
   - **Fase 6** — transcrição (Whisper self-hosted + diarização), pipeline de AI para MoM e calendário inteligente multi-tenant com anexação automática do MoM ao evento.
3. Escreve testes (unitários no Rust, integração na sinalização) e um `README.md` com instruções para correr tudo localmente.
4. No fim de cada fase, mostra como testar manualmente (ex.: abrir duas abas do browser e fazer uma chamada).

Não uses serviços cloud proprietários — tudo self-hosted e open-source. Prioriza sempre: funcionar de verdade > segurança > performance > estética.
