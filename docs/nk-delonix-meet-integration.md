# nk_delonix_meet ↔ Delonix Meet — desenho da integração

> Fase 4 do épico de IA (13/07/2026). Módulo Odoo 16 `nk_delonix_meet` em
> `kaeso/prod/Kaeso-Singlecompany/nokubiko/`, a par do `nk_strategic_meetings`
> (modelo `strategic.meeting`, com delegação em `calendar.event`).

## Objetivo

1. **Sincronizar o calendário**: reuniões agendadas no Delonix Meet aparecem em
   `strategic.meeting` (e vice-versa, opcional na v2).
2. **Entregar o MoM**: quando a ata AI fica pronta no Delonix (ai.rs → Ollama),
   o resumo entra na reunião estratégica correspondente no Odoo.
3. **Resiliência**: se um dos lados estiver em baixo, nada se perde — os dados
   seguem quando ambos voltarem a estar disponíveis.

## Arquitetura: pull-first com webhook de aceleração

O Odoo é o **cliente** (pull), o Delonix é a **fonte**. Pull com cron dá retry
de graça e evita estado pendente no Delonix; o webhook existente do Delonix
serve apenas para *acelerar* o próximo pull (não é o mecanismo de entrega).

```
Odoo (nk_delonix_meet)                     Delonix Meet
┌──────────────────────┐                   ┌───────────────────────┐
│ delonix.meet.config  │── API key ───────▶│ /api/v1 (apikeys.rs)  │
│ cron: sync_meetings  │── GET /meetings ─▶│ meetings.rs           │
│ cron: sync_moms      │── GET /notes ────▶│ notes_by_room         │
│ delonix.meet.outbox  │◀─ webhook ping ───│ webhooks.rs (já há)   │
└──────────────────────┘                   └───────────────────────┘
```

## Lado Delonix (pequeno, já quase tudo existe)

| Peça | Estado |
|---|---|
| API keys por org com scopes (`apikeys.rs`) | ✅ existe |
| Provisão de org via segredo de plataforma (`POST /api/v1/admin/orgs`) | ✅ existe (PR #1) |
| `GET /api/v1/meetings?since=<iso>` com API key (lista da org, incremental) | ⬜ adicionar |
| `GET /api/v1/meetings/{id}/notes` (title/minutes/transcript + `minutes_ai_at`) | ⬜ adicionar |
| Evento webhook `meeting.mom_ready` (dispara após spawn_mom_summary concluir) | ⬜ adicionar a `webhooks.rs` |

`minutes_ai_at` (timestamp em `meetings`) permite ao Odoo saber se o MoM já é a
versão AI final ou ainda a versão por regras (busca de novo mais tarde).

## Lado Odoo (`nk_delonix_meet`)

### Modelos
- **`delonix.meet.config`** (singleton por company): `base_url`, `api_key`
  (encriptada), `enabled`, `last_meeting_sync`, `last_mom_sync`.
- **`delonix.meet.map`**: `delonix_meeting_id` (uuid, unique) ↔
  `strategic_meeting_id` (m2o) + `room_code`, `mom_state`
  (`pending|raw|final`), `last_error`, `retry_count`.
- **`delonix.meet.outbox`** (para o sentido Odoo→Delonix, v2): payload JSON +
  estado `pending|sent|failed`, cron de reenvio com backoff exponencial —
  garante o requisito "envia quando os dois estiverem disponíveis".

### Crons (ir.cron)
1. **sync_meetings** (5 min): `GET /api/v1/meetings?since=last_sync` → cria/
   atualiza `strategic.meeting` via mapa (título, início, duração,
   participantes por email → `res.partner`). Falha de rede: NÃO avança
   `last_meeting_sync` → o próximo tick repete (idempotente pela `map`).
2. **sync_moms** (5 min): para mapas com `mom_state != final`, `GET notes`;
   grava a ata no chatter/campo minutes da `strategic.meeting`; marca `final`
   quando `minutes_ai_at` presente.
3. **outbox_flush** (10 min, v2): reenvia pendentes com backoff.

### Webhook de aceleração
Controller `@http.route('/delonix/webhook', auth='public', csrf=False)` —
valida HMAC (webhooks genéricos do Delonix já assinam) e apenas agenda os crons
`sync_*` para "agora" (`ir.cron._trigger`). Se o webhook nunca chegar, o cron
apanha na mesma — é por isso que a entrega é resiliente.

### Regras Kaeso
Odoo 16 puro, OCA style, sem dependências novas de Python (usar `requests` da
stdlib do Odoo), narrativa PT-EU com identificadores EN, ACLs: config só
`base.group_system`; mapas read-only para utilizadores de meetings.

## Fluxo ponta-a-ponta

1. Reunião agendada no Delonix (calendário) → cron `sync_meetings` cria a
   `strategic.meeting` espelho no Odoo.
2. Reunião acontece; transcrição ao vivo (distribuída) acumula; ao terminar,
   `save_minutes` persiste **ata bruta** (transcript) + ata por regras.
3. `ai.rs` gera o **resumo elegante** via Ollama e substitui `minutes`
   (+ `minutes_ai_at`); webhook `meeting.mom_ready` dispara.
4. Odoo recebe o ping (ou espera o cron) → `sync_moms` puxa a ata e anexa-a à
   reunião estratégica. Se o Odoo estava em baixo, o cron seguinte entrega.
