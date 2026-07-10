# Delonix Meet — Dial-in PSTN · Documento de Decisão de Fase 0

> Deliverable #1 do prompt *32 — Base de Comunicações e Dial-in PSTN*.
> **Estado: PENDENTE DE APROVAÇÃO FINAL** (5.1 e 5.5 propostos; restantes decididos).
> Nenhum código/infra de Fase 1 avança antes da luz verde escrita a este documento.

## 0. Âmbito e contexto (resolvido)

- A implementação **vive no projeto Rust atual** (`delonix-meet`, backend axum). É um
  item já do Roadmap ("Dial-in PSTN"). **Não** se cria um serviço/control-plane Go novo,
  **não** se adota a stack PaaS/microVM/Cilium do prompt. Anula-se essa parte do #32.
- O **control plane** (salas de voz, PIN, DID, CDR, billing, participantes) é implementado
  como **novos módulos no backend Rust existente**, reutilizando Postgres, auth, modelo de
  org/multi-tenant e o SFU já presentes.
- Sem prefixo `nk_` (Restrição #1) — o repo já usa nomenclatura Delonix/genérica.

## 1. As 6 decisões (secção 5 do prompt)

| # | Tema | Decisão |
|---|------|---------|
| 5.1 | Fornecedor SIP/DID | **PROPOSTO** — RFI a **1 operadora local angolana + DIDWW**. *Aguarda confirmação.* |
| 5.2 | Modelo de DID | **Ambos, selecionável na consola**: partilhado+PIN por omissão, DID-por-tenant como add-on premium. |
| 5.3 | Escala no arranque | **300+ canais SIP concorrentes** → exige planeamento de capacidade dedicado (ver §3). |
| 5.4 | Gravação de chamadas | **Fora de âmbito por agora** (o FreeSWITCH suporta depois sem re-arquitetar). |
| 5.5 | Origem no produto | **PROPOSTO** — dial-in criado ao agendar reunião + toggle "adicionar dial-in" numa sala. *Aguarda confirmação.* |
| 5.6 | Billing | **CDR integrado desde o dia 1** no ciclo de faturação Delonix (Restrição #6). |

**Decisão adicional (camada de media):** abstrair o media atrás de uma interface no Rust
com **duas implementações selecionáveis pelo admin** — `freeswitch` (self-hosted, residência)
e `provider` (media pelo fornecedor). Satisfaz a Restrição #3 e estende-a ao media.

## 2. Arquitetura resultante

```
Telefone (PSTN) → Operadora → SIP Trunk (externo, aprovado em 5.1)
                                     │
        ┌────────────────────────────┴─────────────────────────────┐
        │  Backend Rust (delonix-meet) — CONTROL PLANE (único)      │
        │   módulos: voice_rooms · pin/ivr · did · cdr · billing    │
        │   trait MediaBackend  ──┬── impl "freeswitch"             │
        │   (escolhido na consola)└── impl "provider"               │
        └───────────┬───────────────────────────┬──────────────────┘
                    │ (residência: media em AO)  │ (media pelo provider)
     ┌──────────────▼───────────────┐           ▼
     │ Kamailio (LB SIP) → N×        │     Bridge do fornecedor → SFU Rust
     │ FreeSWITCH (mixing/IVR/DTMF)  │
     │  — INFRA, não serviço de app  │
     └──────────────┬───────────────┘
                    ▼  SRTP → SFU (webrtc-rs) existente une PSTN ↔ WebRTC
```

- **`MediaBackend` (trait Rust)** — `create_session`, `join_conference(room, pin)`,
  `collect_pin (DTMF)`, `hangup`, eventos → o control plane fica agnóstico ao backend.
- **`freeswitch`**: control plane fala com o FreeSWITCH (ESL/mod_event_socket). O
  FreeSWITCH faz IVR do PIN, mixing e ponte SRTP para o SFU. Kamailio à frente para
  registo/roteamento e **balanceamento (dispatcher)** a 300+ canais.
- **`provider`**: o fornecedor termina o SIP e entrega o áudio ao SFU; menos infra
  nossa, mas o admin assume o trade-off de residência.

## 3. Implicações da escala (300+ canais)

- 1 FreeSWITCH não chega a 300+ com mixing → **Kamailio dispatcher + N nós FreeSWITCH**
  (regra grosseira ~50–100 chamadas G.711 com mixing por vCPU; margem para picos).
- Isto é **infraestrutura** (à imagem do coturn que já corre), não um serviço de aplicação
  novo — respeita a diretiva "sem serviço novo".
- Contrato de SIP trunk tem de suportar o volume de canais (dado da RFI 5.1).

## 4. Modelo de dados (Postgres, migrações na Fase 1)

- `voice_room` (org_id, room_id ↔ sala existente, pin, did_id, media_backend, estado, criada_em)
- `voice_participant` (room_id, canal `pstn|webrtc`, sip_call_id, entrada/saída)
- `voice_cdr` (org_id, duração, direção, número, custo_estimado, tarifa_ref) — alimenta billing
- `voice_did` (org_id?, número, mercado, modelo `shared|dedicated`, fornecedor, estado)

## 5. Restrições honradas

Residência (§2, media em AO no backend `freeswitch`) · abstração de fornecedor (trait +
`provider` interno) · SRTP+TLS obrigatórios sem fallback (config Fase 1) · reutilização de
infra sem novo cluster · billing first-class (CDR dia 1) · Fase 0 bloqueante (este doc).

## 6. Pendências antes de fechar o gate

1. **Confirmar 5.1** — avanço com RFI a *operadora local + DIDWW*?
2. **Confirmar 5.5** — gatilho de criação (agendar + toggle na sala)?
3. **Aprovação escrita** deste documento → só então arranca a Fase 1.

## 7. Fase 1 (proposta, NÃO iniciada)

Sub-fases com gate próprio: (1) módulos de control plane + esquema/migrações no Rust →
(2) `MediaBackend` trait + impl `provider` (mais rápida de validar E2E) → (3) impl
`freeswitch` + Kamailio/FreeSWITCH as-code + SRTP/TLS → (4) IVR de PIN → (5) CDR/billing →
(6) observabilidade + anti-fraude. Cada uma validada antes da seguinte.

> **Nota de calendário:** esta base PSTN é um esforço próprio, com procurement externo
> (SIP trunk) e infra de media — **independente e posterior** ao go-live de sexta da app
> de vídeo (que já está pronto). Não colide com essa data.
