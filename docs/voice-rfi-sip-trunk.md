# RFI — SIP Trunk / DID para Delonix Meet (Dial-in PSTN)

> Pedido de Informação (Request for Information) a enviar aos fornecedores
> candidatos do ponto 5.1 da Fase 0. Objetivo: comparar cobertura, preço, SLA e
> conformidade de residência antes de contratar. **Enviar a ≥2 fornecedores**
> (recomendado: 1 operadora local angolana + DIDWW).

## Sobre nós (contexto para o fornecedor)
A Delonix vai lançar uma plataforma de conferência (Delonix Meet) com **dial-in
PSTN**: participantes entram numa sala de reunião por chamada telefónica normal,
introduzindo um PIN. O **processamento de media é self-hosted em Angola** (SBC +
media server próprios) por requisito de **residência de dados (Lei 22/11)** — ao
fornecedor pedimos apenas o **SIP trunk** (a ligação à PSTN) e os **números DID**.

## Requisitos-chave
- **Mercado primário: Angola** (números geográficos de Luanda e, se possível,
  Huambo/Lobito/Benguela). Secundário/opcional: Namíbia e Moçambique.
- **Escala: 300+ canais SIP concorrentes** no arranque (crescimento previsto).
- **Entrega**: SIP sobre **TLS** e **media SRTP** (obrigatório, sem fallback em claro).
- **Residência**: preferência por POP/peering **dentro de Angola** ou o mais
  próximo possível; confirmação de que o fornecedor **não retém** o conteúdo de
  media (só transporte de sinalização/trunk).

## Perguntas (por favor responder ponto a ponto)

### A. Cobertura e números (DID)
1. Fornecem **DIDs geográficos angolanos** (não apenas não-geográficos)? Que cidades?
2. Os DIDs suportam **inbound de conferência** (múltiplas chamadas simultâneas no mesmo número)?
3. Prazo e processo de aprovisionamento de novos DIDs (KYC/documentação exigida em AO)?
4. Cobertura equivalente em **Namíbia** e **Moçambique** (opcional)?

### B. Capacidade e qualidade
5. Suportam **300+ canais concorrentes** por trunk? Como se contrata/escala (canais vs. CPS)?
6. **Latência e jitter** típicos até Angola; onde ficam os POP/peering mais próximos?
7. **SLA de disponibilidade** (%) e compensações; janelas de manutenção.
8. Suporte a **codecs** G.711 (a/µ-law) e Opus?

### C. Segurança e integração
9. Suportam **SIP-TLS (5061)** e **SRTP** no trunk? Autenticação por **IP allowlist**
   e/ou **credenciais SIP** (`use-auth-secret` / registo)?
10. Proteções anti-**toll-fraud** do vosso lado (limites de destino, alertas)?
11. Documentação de interop com **Kamailio/FreeSWITCH**?

### D. Comercial
12. **Custo por minuto** inbound (e outbound, se aplicável) para AO.
13. **Aluguer mensal por DID** (geográfico vs. não-geográfico).
14. Custos de setup/porta, mínimos mensais, moeda e ciclo de faturação.
15. Fornecem **CDRs** detalhados (para reconciliação com o nosso billing)?

### E. Conformidade
16. Como asseguram (ou não impedem) a **residência de dados** angolana, dado que o
    media é processado por nós? Retêm gravações/metadados de chamada?

## Critérios de decisão (uso interno)
Cobertura geográfica AO real · preço/min + aluguer DID · SLA e proximidade de POP ·
suporte TLS/SRTP · capacidade 300+ canais · conformidade de residência · qualidade
da documentação de interop.

## Próximo passo
Com as respostas, preenche-se a decisão do ponto 5.1 em
[docs/pstn-dial-in-fase0.md](pstn-dial-in-fase0.md) e liga-se o trunk escolhido na
**sub-fase 3** (a infra de media da sub-fase 2 já está pronta a recebê-lo).
