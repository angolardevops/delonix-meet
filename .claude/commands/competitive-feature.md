---
description: Analisa uma feature proposta versus o que Zoom, Teams e Meet têm. Sugere como fazer melhor. Uso: /competitive-feature <nome da feature>
---

Analisa a feature: **$ARGUMENTS**

Usando `docs/competitive-positioning.md` como referência, responde:

## 1. O que cada plataforma faz hoje

**Zoom:** [como implementa, limitações, plano necessário]

**Microsoft Teams:** [como implementa, limitações, integração M365]

**Google Meet:** [como implementa, limitações, integração Workspace]

## 2. O que faltam a todos

[Oportunidades que nenhum dos três cobre bem — especialmente para self-host, soberania de dados, mercados lusófonos/africanos]

## 3. Como o Delonix devia implementar

Considerando:
- Self-hosting first (funciona offline, sem cloud dependency)
- Multi-tenant com isolamento de org
- E2EE preservado
- Backend Rust (sfu.rs, signaling.rs)
- Frontend React (Room.tsx, ou nova página)

[Proposta concreta com: modelo de dados, mensagens WS necessárias, endpoints REST, componentes UI]

## 4. O que NÃO implementar

[Partes do que a concorrência tem que não fazem sentido para o Delonix ou ficam para roadmap]

## 5. Estimativa de esforço

[Pequena (<1 dia) / Média (2-3 dias) / Grande (1 semana+)]
