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
| `…/chapters`, `…/comments` | iguais (com `{recording_id}`); payloads em `t_ms` (R183) |
| `GET /api/share/{token}` | `GET /api/public/recordings/{token}` |
| `GET /api/share/{token}/download` | `GET /api/public/recordings/{token}/content` |

**R183 — o contrato de dados da UI nova, nos caminhos desta linha.** A coluna «Antes» é
o servidor da branch da UI (`integra/validacao-l2`); o que não tem par era novo.

| Antes (servidor da UI) | Depois |
|---|---|
| `GET /api/recordings/{id}/details` | `GET /api/recordings/{recording_id}` (o mesmo `RecordingLibraryItem`) |
| `GET /api/recordings?q=&scope=mine\|published` | igual (lista); com `page_size`/`page_token`, página |
| `POST /api/recordings/{id}/publish` `{visibility:"org"}` | `PUT /api/recordings/{recording_id}/publication` → `200` item |
| `POST /api/recordings/{id}/unpublish` | `DELETE /api/recordings/{recording_id}/publication` → `204` (`404 recording.not_published`) |
| `GET /api/recordings/{id}/thumbnail` | `GET /api/recordings/{recording_id}/thumbnail` |
| `POST /api/recordings/{id}/views` | `POST /api/recordings/{recording_id}/views` → `204` |
| `GET /api/recordings/{id}/participants` | `GET /api/recordings/{recording_id}/participants` |
| `GET /api/rooms/{code}/participants` | `GET /api/rooms/{room_code}/participants` |
| `GET /api/recordings/{id}/transcript` | `GET /api/recordings/{recording_id}/transcript` |
| `PATCH /api/recordings/{id}/chapters/{chapter_id}` | `PATCH /api/recordings/{recording_id}/chapters/{chapter_id}` |
| `GET /api/recordings/{id}/captions` | `GET /api/recordings/{recording_id}/captions` |
| `GET\|PUT\|PATCH\|DELETE /api/recordings/{id}/captions/{lang}` | `GET\|PUT\|PATCH\|DELETE /api/recordings/{recording_id}/captions/{lang}` (`PUT` → `201`+`Location` ao criar, `200` ao substituir) |
| `GET /api/recordings/{id}/captions/{lang}/vtt` | `GET /api/recordings/{recording_id}/captions/{lang}/vtt` |
| `POST /api/recordings/{id}/captions/generate`, `POST …/chapters/generate` | **por portar** (dependem do cliente Ollama; sem rota nem stub até lá) |

Erros com código: `recording.not_manager` (403), `recording.comment_delete_forbidden` (403),
`recording.no_file` (409), `recording.chapter_timestamp_taken` (409),
`recording.caption_not_ready` (409), `recording.too_many_chapters` / `recording.too_many_captions` (422),
`recording.invalid_{filename,description,tags,kind,visibility,scope,timestamp,chapter_title,comment,caption_lang,caption_status,vtt}` e
`recording.caption_too_large` (400). Quem não chega à gravação recebe sempre `404` antes de qualquer outro.

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
