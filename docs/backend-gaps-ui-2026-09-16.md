# Lacunas do backend face à UI nova — medição de 2026-09-16

> **Árvore do backend:** `origin/main` em `6878ff7`.
> **UI medida:** os worktrees `frontend/console-ui-rebuild` e `frontend/ui-*`, lidos a
> 2026-09-16 enquanto outra sessão os alterava — é um retrato, não um contrato.
> **Método:** `grep` de todas as chamadas `/api`, `/ws` e `/rtc` em cada worktree,
> comparadas rota a rota com o router, mais a leitura das notas «fica de fora» nos
> ecrãs novos.
> **O que não foi medido:** o que a UI vai chamar quando os ecrãs ainda em stub
> (`Room`, `Calendar`, `Studio`, …) forem escritos.

## 1. Rotas que a UI chama e o backend não serve

**Nenhuma.** O `web/src/api.ts` é igual byte a byte em todos os worktrees.

A única rota nova é `/api/orgs/{org_id}/stream-destinations`:

- foi criada **no ramo da UI** (`6331292`, migração `0039`, `server/src/stream_destinations.rs`);
- nenhum ecrã a chama;
- guarda a `stream_key` em claro, a mesma classe de falha da S5.

Este trabalho traz a rota para o backend, com a chave cifrada. O ramo da UI deve largar a
sua cópia de `server/`.

## 2. Capacidades que os ecrãs novos deixam de fora por não haver backend

| # | Capacidade | Ecrã que a pede | Recurso de backend |
|---|---|---|---|
| G1 | Destinos de emissão guardados por org + estado por destino (saúde, bitrate) | `home/SideColumn.tsx`, `studio/LivePanel.tsx` | `stream_destinations` (CRUD, chave cifrada) + estado vivo do `broadcast::Registo` |
| G2 | «A minha sala» — link pessoal permanente + dial-in | `home/SideColumn.tsx` | `GET/PUT /api/users/me/room` (sala pessoal, DID opcional) |
| G3 | Armazenamento da org: usado vs quota | `home/SideColumn.tsx`, `admin/CapacityRow.tsx` | `GET /api/orgs/{org_id}/storage-usage` + `max_storage_bytes` na quota |
| G4 | Gravações com metadados: duração, resolução, tamanho, estado de processamento, categoria | `recordings/RecordingTable.tsx`, `Recordings.tsx` | colunas + máquina de estados (a 0036 já tem `status`) |
| G5 | Capítulos e comentários com marca temporal numa gravação | `recordings/RecordingPanel.tsx` | `recording_chapters`, `recording_comments` |
| G6 | Pesquisa na transcrição | `Recordings.tsx` | `GET /api/recordings?q=` sobre `transcript` (FTS do Postgres) |
| G7 | Registo de entregas de webhooks + reenviar — **feito** (migração 0043) | `integrations/WebhooksCard.tsx` | `webhook_deliveries` + `GET /api/orgs/{org_id}/webhooks/{hook_id}/deliveries[/{delivery_id}]` (paginado, `?status=`) + `POST …/deliveries/{delivery_id}/redeliver` (`202` + `Location`, 10/min por webhook) |
| G4 | ✅ Gravações com metadados: duração, resolução, tamanho, estado de processamento, categoria | `recordings/RecordingTable.tsx`, `Recordings.tsx` | **Feito** (migração 0045): `duration_secs`, `width`, `height`, `category`, `title`, e `processing_state` derivado (`ready` · `failed` · `transcribing` · `transcribed` · `transcription_failed` — sem `processing`, que o recorder nunca deixa ver) na biblioteca e em `GET /api/recordings/{id}/metadata`; `PATCH /api/recordings/{id}` (dono ou admin). Duração e resolução só nas gravações do servidor; nas carregadas pelo browser ficam `null` |
| G5 | ✅ Capítulos e comentários com marca temporal numa gravação | `recordings/RecordingPanel.tsx` | **Feito**: `/api/recordings/{id}/chapters[/{chapter_id}]` (dono ou admin escreve, máx. 100) e `/api/recordings/{id}/comments[/{comment_id}]` (quem vê a gravação; só o autor edita e apaga, apagar é lógico; DLP no corpo); paginados |
| G6 | ✅ Pesquisa na transcrição | `Recordings.tsx` | **Feito**: `GET /api/recordings?q=` (título, ficheiro e transcrição; `tsvector` `simple` + GIN; termos em prefixo; `snippet` com `«»`). Com `q`/`page_size`/`page_token` a resposta é `{items, next_page_token}`; sem parâmetros continua a lista inteira |
| G7 | Registo de entregas de webhooks + reenviar | `integrations/WebhooksCard.tsx` | `webhook_deliveries` + `POST …/deliveries/{id}/redeliver` |
| G8 | Centro de notificações | `AppShell.tsx` (o antigo era só cliente) | `GET /api/notifications`, marcar como lida, e envio em tempo real pelo `/rtc` |
| G7 | Registo de entregas de webhooks + reenviar | `integrations/WebhooksCard.tsx` | `webhook_deliveries` + `POST …/deliveries/{id}/redeliver` |
| G8 | Centro de notificações — **feito** (2026-09-16, `notifications.rs`, migração 0044) | `AppShell.tsx` (o antigo era só cliente) | `GET /api/users/me/notifications` (cursor, `unread_only`, `unread_count`), `PATCH …/{id}` `{"read"}`, `POST …/mark-all-read`, `DELETE …/{id}`; push `{"type":"notification"}` pelo `/rtc`. Falta o lado do web (tipo novo em `presence.ts` e o ecrã) e o produtor na v1 (`meetings_v1.rs` cria convidados sem notificar) |
| G9 | Retenção de chat (e de auditoria só como exportação — a cadeia é imutável) | `admin/SettingsCard.tsx` | `chat_retention_days` nas definições + varredor |
| G10 | Inventário de nós de media (capacidade) — **feito** (`nodes.rs`, migração 0048, `/api/operator/v1/nodes`) | `admin/CapacityRow.tsx` | batimento por nó (salas, pares, filas) + `GET /api/operator/v1/nodes` |
| G11 | PNG de quadro por URL assinado | `boards/BoardViewer.tsx` | URL assinado de curta duração |
| G12 | Edição, perfil e capacidades da instalação | todos (esconder o que não existe) | `edition` + `capabilities` em `GET /api/public/settings` (ADR-0006 §2) |

Ficam de fora, por serem só de cliente: a edição multi-faixa do Estúdio, a mistura e as
legendas queimadas. O teclado PSTN, a transferência e o DTMF ficam também de fora, porque
dependem de um tronco SIP com media — ver `docs/voice-rfi-sip-trunk.md`.

## 3. Defeito de protocolo existente (não vem da UI nova) — **fechado** (R124)

O cliente envia `promote-admit` e espera receber `admit-role` e `peer-role`
(`web/src/signaling.ts:77,78,120`, `Room.tsx:3236`). O `ClientMsg`/`ServerMsg` de
`server/src/signaling.rs` não tem nenhum dos três. A mensagem é recusada na
desserialização e a promoção a admissor nunca chega ao servidor.

**Fechado a 2026-09-16 (R124):** `ClientMsg::PromoteAdmit`, `ServerMsg::{AdmitRole, PeerRole}`,
`PeerInfo.can_admit`; o co-anfitrião de admissões recebe a sala de espera e decide sobre
ela, e o papel persiste em `room_admitters` (`rooms::set_room_admitter`, até aqui sem
chamadores).
