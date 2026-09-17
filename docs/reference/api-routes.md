# Mapa de rotas — reorganização de 2026-09-16

> **Decisão do dono do produto (2026-09-16):** o produto ainda não está em produção, por
> isso as rotas vão já para o sítio certo, **sem aliases** para os caminhos antigos. Este
> ficheiro é a tabela antigo → novo usada para actualizar TODOS os consumidores do repo
> (`web/src`, `web/e2e`, `sms-gateway`, `voice/freeswitch`, `deploy/`, `docs/`) no mesmo PR.
> Os consumidores fora do repo (módulo Odoo `nk_delonix_meet`) seguem a secção
> «Consumidores externos».
>
> Autoridade: [ADR-0004 §4](../adr/0004-organizacao-alvo-do-backend.md) (uma superfície por
> público) e [ADR-0006 §3](../adr/0006-backend-enterprise-contextos-edicoes-e-entrega.md).
> Depois de aplicado, a fonte de verdade é o OpenAPI gerado (`docs/reference/openapi/`);
> este ficheiro fica como registo da migração.

## Regras que decidiram cada linha

1. **Uma superfície por público:**
   - `/api/…` é a BFF (sessão);
   - `/api/v1/…` é o inquilino (`dlx_`);
   - `/api/operator/v1/…` é o operador;
   - `/api/integrations/odoo/v1/…` é o Odoo (`dlxo_`);
   - `/api/integrations/sms-agent/v1/…` é o agente USB (`dlxg_`);
   - `/internal/v1/…` é máquina-a-máquina, só no listener interno.
2. **Substantivos no plural e hierarquia.** Um sub-recurso vive debaixo do pai.
3. **Parâmetros com nome:** `{meeting_id}`, `{recording_id}`, … e `{room_code}` para salas.
4. **Métodos:**
   - `PUT` para singletons (acta, plano, link público, SSO);
   - `PATCH` para alteração parcial;
   - uma acção que não é CRUD é um método personalizado com verbo (`/start`, `/rotate-key`).
5. **O que se mantém de propósito:**
   - `/ws` e `/rtc` são endpoints de protocolo com afinidade por sala no ingress (ADR-0001, R3); renomeá-los mexia em quatro camadas sem ganho de contrato.
   - `/health`, `/ready` e `/metrics` são sondas.

## BFF (`/api`, sessão)

### Autenticação
| Antes | Depois |
|---|---|
| `POST /api/auth/register` | `POST /api/auth/register` |
| `POST /api/auth/login` | `POST /api/auth/login` |
| `POST /api/auth/mfa` | `POST /api/auth/login/mfa` |
| `POST /api/auth/refresh` | `POST /api/auth/refresh` |
| `POST /api/auth/logout` | `POST /api/auth/logout` |
| `GET /api/auth/sso/check` | `GET /api/auth/sso/discovery` |
| `GET /api/auth/sso/login` | `GET /api/auth/sso/authorize` |
| `GET /api/auth/sso/callback` | `GET /api/auth/sso/callback` |

### Utilizador
| Antes | Depois |
|---|---|
| `GET/PATCH /api/users/me` | igual |
| `GET /api/users/search?q=` | `GET /api/users?q=` |
| `GET /api/users/me/mfa` | igual |
| `POST /api/users/me/mfa/enrol` | `POST /api/users/me/mfa/enroll` |
| `POST /api/users/me/mfa/activate` | igual |
| `POST /api/users/me/mfa/disable` | igual |
| `/api/users/me/notifications…` | igual |
| `POST /api/missed-calls/ack` | `POST /api/users/me/missed-calls/acknowledge` |

### Conta pessoal (ADR-0011 — rotas novas, sem «antes»)
| Rota | Notas |
|---|---|
| `GET/PATCH /api/users/me/profile` | `409 profile.field_managed_by_odoo` nos campos do Odoo |
| `GET/PUT/DELETE /api/users/me/avatar`, `GET /api/users/{user_id}/avatar` | imagem crua; a de outra pessoa só com org em comum |
| `GET/PUT /api/users/me/join-preferences` | singleton (`PUT` completo) |
| `GET/PUT /api/users/me/notification-preferences` | singleton (`PUT` completo) |
| `GET/PATCH /api/users/me/tour`, `PUT /api/users/me/tour/steps/{step_id}`, `POST /api/users/me/tour/skip`, `POST /api/users/me/tour/restart` | passos versionados |
| `POST /api/users/me/sessions/revoke-others` | termina todas menos a do pedido (`GET /api/users/me/sessions` e `DELETE …/{session_id}` já existiam) |
| `POST /api/users/me/reauthentication` | abre a janela de 5 min para alterar factores |
| `GET /api/users/me/security` | resumo de segurança |
| `GET/POST /api/users/me/passkeys`, `GET/DELETE /api/users/me/passkeys/{passkey_id}`, `POST /api/users/me/passkeys/begin-registration` | `201` + `Location` ao registar |
| `POST /api/users/me/mfa/backup-codes/regenerate` | exige reautenticação |
| `POST /api/auth/login/mfa/passkey-options`, `POST /api/auth/login/mfa/passkey` | segundo factor com chave |
| `POST /api/users/me/room/rotate-pin` | «Novo PIN» |
| `GET/POST /api/users/me/data-exports`, `GET /api/users/me/data-exports/{export_id}`, `POST …/{export_id}/download-link`, `GET …/{export_id}/content` | `202` + `Location`; `content` sem sessão (assinatura). A síncrona `GET /api/users/me/export` fica |

### Salas
| Antes | Depois |
|---|---|
| `POST /api/rooms` | igual (`201` + `Location`) |
| `GET /api/rooms/{code}` | `GET /api/rooms/{room_code}` |
| `POST /api/rooms/{code}/join` | `POST /api/rooms/{room_code}/join` |
| `GET /api/rooms/{code}/chat` | `GET /api/rooms/{room_code}/messages` |
| `POST /api/rooms/{code}/invite` | `POST /api/rooms/{room_code}/invitations` |
| `POST /api/rooms/{code}/qos` | `POST /api/rooms/{room_code}/quality-samples` |
| `POST /api/rooms/{code}/timings` | `POST /api/rooms/{room_code}/join-timings` |
| `GET/POST /api/rooms/{code}/recordings` | `GET/POST /api/rooms/{room_code}/recordings` |
| `POST /api/rooms/{code}/minutes` | `PUT /api/rooms/{room_code}/minutes` |
| `GET /api/rooms/{code}/notes` | `GET /api/rooms/{room_code}/minutes` |
| `GET /api/rooms/{code}/broadcast` (WS) | `GET /api/rooms/{room_code}/live` (WS) |
| `GET /api/ice` | `GET /api/ice-servers` |
| `POST /api/translate` | `POST /api/ai/translations` |

### Reuniões
| Antes | Depois |
|---|---|
| `GET/POST /api/meetings` | igual |
| — | `GET /api/meetings/{meeting_id}` (**novo**: recurso completo) |
| `DELETE /api/meetings/{id}` | `DELETE /api/meetings/{meeting_id}` (`204`) |
| `POST /api/meetings/conflicts` | `POST /api/meetings/check-conflicts` |
| `POST /api/meetings/{id}/start` | `POST /api/meetings/{meeting_id}/start` |
| `GET /api/meetings/{id}/ics` | `GET /api/meetings/{meeting_id}/calendar.ics` |
| `POST /api/meetings/{id}/minutes` | `PUT /api/meetings/{meeting_id}/minutes` |
| `GET /api/meetings/{id}/invitees` | `GET /api/meetings/{meeting_id}/invitees` |
| `POST /api/meetings/{id}/respond` | `PUT /api/meetings/{meeting_id}/invitees/me` |
| `GET/POST /api/meetings/{id}/agenda` | `GET/POST /api/meetings/{meeting_id}/agenda-items` |
| `PATCH/DELETE /api/meetings/{id}/agenda/{item_id}` | `PATCH/DELETE /api/meetings/{meeting_id}/agenda-items/{item_id}` |
| `GET/PUT /api/meetings/{id}/action-plan` | `GET/PUT /api/meetings/{meeting_id}/action-plan` |
| `POST /api/meetings/{id}/action-plan/items` | `POST /api/meetings/{meeting_id}/action-plan/items` |
| `PATCH/DELETE /api/action-items/{item_id}` | `PATCH/DELETE /api/meetings/{meeting_id}/action-plan/items/{item_id}` |
| `GET /api/quarantine/analytics?org_id=` | `GET /api/orgs/{org_id}/analytics/quarantine` |

### Organizações
| Antes | Depois |
|---|---|
| `GET/POST /api/orgs` | igual |
| — | `GET /api/orgs/{org_id}` (**novo**) |
| `POST /api/orgs/{org_id}/settings` | `PATCH /api/orgs/{org_id}` |
| `…/employees[/{user_id}]` | `…/members[/{user_id}]` |
| `GET /api/orgs/{org_id}/audit` | `GET /api/orgs/{org_id}/audit-events` |
| `GET /api/orgs/{org_id}/audit/verify` | `GET /api/orgs/{org_id}/audit-events/verification` |
| `…/integration/odoo` | `…/integrations/odoo` |
| `POST …/integration/odoo/token` | `POST …/integrations/odoo/rotate-token` |
| `GET …/voice/cdr` | `GET …/voice/call-records` |
| `POST /api/voice/rooms` | `POST /api/orgs/{org_id}/voice/rooms` |
| `GET /api/voice/rooms/{id}` | `GET /api/orgs/{org_id}/voice/rooms/{voice_room_id}` |
| `GET /api/voice/rooms/{id}/participants` | `GET /api/orgs/{org_id}/voice/rooms/{voice_room_id}/participants` |
| `POST /api/voice/rooms/{id}/close` | `POST /api/orgs/{org_id}/voice/rooms/{voice_room_id}/close` |
| `branches`, `groups`, `meeting-rooms`, `stats`, `sso`, `api-keys`, `webhooks[/…/deliveries]`, `stream-destinations`, `sms/*` | iguais |

### Gravações
| Antes | Depois |
|---|---|
| `GET /api/recordings` | igual |
| `GET /api/recordings/{id}/metadata` | `GET /api/recordings/{recording_id}` |
| `GET /api/recordings/{id}` (ficheiro) | `GET /api/recordings/{recording_id}/content` |
| `PATCH /api/recordings/{id}` | `PATCH /api/recordings/{recording_id}` |
| `GET/POST /api/recordings/{id}/share` | `GET/POST /api/recordings/{recording_id}/shares` |
| `DELETE /api/recordings/{id}/share/{user_id}` | `DELETE /api/recordings/{recording_id}/shares/{user_id}` |
| `GET/POST/DELETE /api/recordings/{id}/link` | `GET/PUT/DELETE /api/recordings/{recording_id}/public-link` |
| `…/chapters`, `…/comments` | iguais (com `{recording_id}`) |
| `GET /api/share/{token}` | `GET /api/public/recordings/{token}` |
| `GET /api/share/{token}/download` | `GET /api/public/recordings/{token}/content` |

#### Contrato de dados das gravações (2026-09-29, R234–R237)

O item da biblioteca é o `RecordingLibraryItem` que a consola lê
(`web/src/api.ts`). Fechou a metade por reconciliar do R183.

| Antes | Agora | Porquê |
|---|---|---|
| `duration_secs` (só a do gravador) | `duration_ms` | O upload media e escrevia `duration_ms`; a listagem servia a outra coluna, e a duração nunca aparecia (R236) |
| `category` (`meeting`/`lecture`/`broadcast`/`other`) | `kind` (`meeting`/`training`/`broadcast`/`hybrid`) | É o formato da SALA, não uma etiqueta à escolha |
| `title` | `filename` + `description` + `tags` | O nome é o que se mostra e com que se descarrega; a descrição e as etiquetas são campos próprios |
| `processing_state` (5 valores num eixo) | `status` + `state` + `transcript_status` | O ficheiro e a transcrição são dois eixos; `state` acrescenta `published` |
| — | `visibility`, `published_at` | Publicação para a organização |
| — | `width`/`height`/`fps`/`video_codec`/`audio_codec`/`has_thumbnail` | Medidos com `ffprobe` |
| — | `chapter_count`, `comment_count`, `view_count`, `participant_count`, `caption_languages` | Contagens que a biblioteca mostra |
| — | `uploader_org_id`, `uploader_org_name`, `can_manage` | Organização do autor e o que quem pede pode fazer |
| capítulos em `at_secs`, sem origem | `t_ms` + `source` (`auto`/`manual`) | Milissegundos em todo o contrato; voltar a gerar não apaga os manuais |
| comentários em `at_secs`, `author_id`/`author_name` | `t_ms`, `user_id`/`username` | Idem |

Rotas afectadas:

| Rota | Mudança |
|---|---|
| `GET /api/recordings` | `?scope=mine\|published` (**novo**). `published` lista as publicadas para a organização, incluindo as de salas onde quem pede nunca esteve (R235). `?q=` sozinho devolve a LISTA; a página pede-se com `page_size`/`page_token` |
| `PATCH /api/recordings/{recording_id}` | Aceita `filename`, `description`, `tags`, `kind`; **recusa** campos desconhecidos (`422`) — `title`/`category` deixam de ser aceites em silêncio |
| `GET /api/recordings/{recording_id}/content` | Honra `Range`: `206` + `Content-Range`, `416` fora do ficheiro, `Accept-Ranges: bytes` sempre (R237) |
| `GET /api/recordings/{recording_id}/transcript` | `403 recording.transcript_forbidden` a quem só lá chega por publicação |
| `GET /api/recordings/{recording_id}/participants` | `403 recording.participants_forbidden`, mesma razão |

### Quadros
| Antes | Depois |
|---|---|
| `GET/POST /api/whiteboards` | igual |
| `DELETE /api/whiteboards/{id}` | `DELETE /api/whiteboards/{whiteboard_id}` |
| `GET /api/whiteboards/{id}/png` | `GET /api/whiteboards/{whiteboard_id}/image` |
| `POST /api/whiteboards/{id}/share` | `PUT /api/whiteboards/{whiteboard_id}/public-link` |
| `GET /api/whiteboards/shared/{token}` | `GET /api/public/whiteboards/{token}/image` |

### Plataforma
| Antes | Depois |
|---|---|
| `GET /api/status`, `GET /api/public/settings`, `GET /api/openapi.json` | iguais |

## Pública do inquilino (`/api/v1`, chave `dlx_`)
| Antes | Depois |
|---|---|
| `GET /api/v1/org` | `GET /api/v1/organization` |
| `POST /api/v1/rooms`, `GET /api/v1/rooms/{code}` | `POST /api/v1/rooms`, `GET /api/v1/rooms/{room_code}` |
| `POST /api/v1/rooms/{code}/join-bot` | `POST /api/v1/rooms/{room_code}/bots` |
| `GET /api/v1/recordings` | igual |
| `GET/POST /api/v1/meetings`, `PATCH/DELETE /api/v1/meetings/{id}` | iguais (`{meeting_id}`) + `GET /api/v1/meetings/{meeting_id}` (**novo**) |
| `POST /api/v1/meetings/{id}/ring` | `POST /api/v1/meetings/{meeting_id}/ring` |
| `GET /api/v1/meetings/{id}/notes` | `GET /api/v1/meetings/{meeting_id}/minutes` |
| `POST /api/v1/admin/orgs` | **sai** → operador |
| `/api/v1/integration/odoo/*` | **sai** → integração Odoo |
| `/api/v1/platform/storage*` | **sai** → operador |

## Operador (`/api/operator/v1`)
| Antes | Depois |
|---|---|
| `POST /api/v1/admin/orgs` | `POST /api/operator/v1/organizations` (segredo de plataforma) |
| `GET/PUT /api/v1/platform/storage` | `GET/PUT /api/operator/v1/storage` |
| `POST /api/v1/platform/storage/test` | `POST /api/operator/v1/storage/test` |
| `GET /api/v1/platform/storage/pvc-manifest` | `GET /api/operator/v1/storage/pvc-manifest` |
| `GET /api/operator/v1/nodes` | igual |

## Integrações
| Antes | Depois |
|---|---|
| `POST /api/v1/integration/odoo/provision` | `POST /api/integrations/odoo/v1/provision` |
| `GET /api/v1/integration/odoo/users` | `GET /api/integrations/odoo/v1/users` |
| `PUT /api/sms/agent/devices` | `PUT /api/integrations/sms-agent/v1/devices` |
| `POST /api/sms/agent/claim` | `POST /api/integrations/sms-agent/v1/claim` |
| `POST /api/sms/agent/messages/{id}/result` | `POST /api/integrations/sms-agent/v1/messages/{message_id}/result` |

## Interna (`/internal/v1`, listener interno ou público sem `INTERNAL_BIND_ADDR`)
| Antes | Depois |
|---|---|
| `POST /api/voice/ivr/validate` | `POST /internal/v1/voice/ivr/validate` |
| `POST /api/voice/ivr/cdr` | `POST /internal/v1/voice/ivr/cdr` |

## Consumidores externos
- **Módulo Odoo `nk_delonix_meet`** (repositório `kaeso-18`): passa a chamar:
  - `POST /api/operator/v1/organizations`, em vez de `/api/v1/admin/orgs`;
  - `/api/integrations/odoo/v1/*`;
  - `GET /api/v1/meetings/{meeting_id}/minutes`, em vez de `/notes`.

  O pedido de alteração a esse módulo fica registado no PR.
