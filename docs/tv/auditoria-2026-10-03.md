# Auditoria — Estúdio de Produção e Estação de TV Digital (Fase 0)

**Data:** 2026-10-03 · **Base medida:** `origin/main` `d1c01f7d` · **Método:** leitura estática do
código por três auditorias paralelas (media, plano de controlo, frontend/operação). Nada foi
compilado nem executado; os achados marcados **[V]** foram reconfirmados à mão contra o código.
Estados: completo, parcial, ausente, incorrecto, não verificável. «Ausente» só quando há grep
registado; caso contrário «não verificável».

## 1. Resumo

O Meet já tem **um estúdio de TV no browser** (Preview/Program, cenas, mesa de som, atalhos,
legendas) e **uma emissão RTMP multi-destino no servidor** (ADR-0003, opção C: o browser compõe e
codifica H.264, o servidor remultiplexa com `ffmpeg -c:v copy`). Isso é um bom ponto de partida
para o **estúdio**, mas é a metade «ao vivo, com o anfitrião presente» do produto. Para ser
**estação de TV** falta quase tudo o que é servidor: não há canal, programa, grelha nem playout,
não há distribuição ao público (HLS/player), não há ingestão externa, e a emissão vive e morre
com o WebSocket do browser do anfitrião.

### Achados bloqueantes (corrigir antes de abrir canais a clientes)

| # | Achado | Evidência | Gravidade |
|---|---|---|---|
| B1 | **Qualquer participante com room token pode pôr uma sala no ar.** `ws_directo` valida o token e o `sala_id`, mas não lê `claims.owner`/`claims.adm` (campos existentes). **[V]** | `server/src/broadcast.rs:1415-1475`; `auth.rs:32,39`; grep `claims.owner/adm` em `broadcast.rs` = vazio | Alta (CA-01/RF-19) |
| B2 | **SSRF por RTMP.** `rtmp_url_is_valid` só verifica o esquema e o host não vazio; o `ffmpeg` liga a qualquer host (rede interna, metadados cloud). Não usa `net_guard`. **[V]** | `broadcast.rs:194-206`; grep `net_guard` em `broadcast.rs`/`stream_destinations.rs` = vazio | Alta (RNF-02) |
| B3 | **A imagem do servidor não tem ffmpeg.** Distroless `cc-debian12`, só copia `delonix-server`; nenhum ficheiro de deploy instala ffmpeg. Directo e gravação server-side devolvem `SemFfmpeg`/falham. **[V]** em `Dockerfile.server`; não verificável se há overlay externo ao repo | `Dockerfile.server:32`; `broadcast.rs:1626` | Alta (RF-11/16) |
| B4 | **O directo morre com o browser do anfitrião** e não sobrevive a reinício; o registo é `HashMap` em memória por pod. | `broadcast.rs:1275,1763-1783`; ADR-0003 «anfitrião é ponto único» | Alta (CA-04/08, RF-15) |
| B5 | **Gravação `ready` antes de validar.** O `UPDATE … status='ready'` precede o `ffprobe`; falha do probe não reverte. **[V]** | `recorder.rs:995-1015` | Média (CA-11) |
| B6 | Iniciar/parar directo **não deixa trilha de auditoria** (só webhook). | grep `audit::` em `broadcast.rs` = vazio **[V]** | Média (RF-20) |
| B7 | Gravação server-side grava **todos** os publishers (por fonte), nunca «só o Program»; e a quota de armazenamento não se impõe na gravação do servidor. | `sfu.rs:1895-1930`; `recorder.rs:493,683,960` | Média (CA-01/14) |
| B8 | Chaves RTMP ad hoc viajam em **query string** de WebSocket (aparecem em logs de proxy). | `broadcast.rs:1550-1561` | Média (RNF-02) |

## 2. Matriz RF → estado (resumo)

Legenda de acção: **R** reutilizar, **E** estender, **N** novo. Fases como no prompt (F1 estúdio,
F2 estação, F3 escala).

| ID | Requisito | Estado actual | Evidência | Lacuna | Acção | Fase |
|---|---|---|---|---|---|---|
| RF-01 | Canais | ausente (grep) | `Channel` existente é outro conceito: `domain/conferencing/channels.rs:30` | tabela, API, política | N (nome `TvChannel`) | F1 |
| RF-02 | Programas | ausente no servidor; rundown só em localStorage | `CenaCompleta.tsx:33,396` | persistir, versionar | N | F1 |
| RF-03 | Estúdio Preview/Program | parcial (só browser) | `studio/tv/mesa.ts:1-50` | servidor desconhece; sem confirmação server | E | F1 |
| RF-04 | Fontes | parcial | webrtc browser, SIP; sem ingest externo | WHIP/RTMP/SRT-in | N | F1/F2 |
| RF-05 | Bastidores/convidados | parcial: lobby/admissão ok; green room ausente | `signaling.rs:4017-4040` | estado «bastidores» e saída pública controlada | E | F1 |
| RF-06 | Comunicação produção | parcial: chat de sala; sem talkback/cue | `room_chat.rs` | intercom, cue, countdown | N | F1 |
| RF-07 | Áudio | parcial (cliente): mesa, AUX/solo, medidor | `mesaDeSom.ts` | mix-minus de convidados ausente (só telefone `phone_bridge/audio.rs:12`); sync A/V não verificável | N | F1 |
| RF-08 | Cenas/grafismo | parcial (cliente) | `compositor.ts`, `palco.ts` | persistência, partilha, sanitização server | E | F1 |
| RF-09 | Guião/teleponto | parcial (cliente, localStorage) | `CenaCompleta.tsx` | servidor, versão | E | F1 |
| RF-10 | Ingestão externa | ausente (grep whip/srt/rtmp-in = 0) | — | tudo | N (MediaMTX/ffmpeg, a decidir) | F1 |
| RF-11 | Composição/transcodificação | ausente no servidor (composição é no browser) | ADR-0003 | worker server-side, capacidade | N | F1/F2 |
| RF-12 | Player/distribuição | ausente (grep hls = 0) | `docs/adopcao-vs-concorrencia.md:131` | HLS/LL-HLS, player, CDN | N | F1 |
| RF-13 | Multi-destino | completo no mux; parcial no estado | `broadcast.rs:425-463`, `stream_destinations.rs` | `state` na BD não reflecte runtime; SSRF (B2) | E | F1 |
| RF-14 | Grelha | ausente | — | tudo | N | F2 |
| RF-15 | Playout | ausente; sem agendador de «ir ao ar» | `lib.rs:1282-1469` só varredores | executor com lease | N | F2 |
| RF-16 | Gravação | parcial | `recorder.rs` | Program, validação pré-`ready` (B5), recuperação de `tmp-*` | E | F1 |
| RF-17 | Arquivo/VOD | parcial: biblioteca, capítulos, partilha | migr. `0058-0062` | catálogo, excertos in/out, tags | E | F2 |
| RF-18 | Legendas | parcial: VTT em VOD, panel no estúdio; directo não verificável | `recording_captions.rs` | no player de directo | E | F2 |
| RF-19 | Papéis | parcial: catálogo ADR-0008; capacidades de estúdio **não impostas** | `authorization.rs:34-78` | «preparar» vs «emitir» (B1) | E | F1 |
| RF-20 | Auditoria/concorrência | auditoria completa (cadeia hash); directo sem eventos; versão/lease ausentes | `0037`; grep etag = 0 | eventos TV, `version`, posse da realização | E | F1 |
| RF-21 | Continuidade | ausente (greps silence/frozen/failover = 0) | — | reserva, detecção, regresso | N | F1/F2 |
| RF-22 | Métricas | parcial: counters/gauges, sem histogramas, sem métricas de directo | `metrics.rs` | métricas de emissão, audiência | E | F1/F3 |
| RF-23 | Quotas/custos | parcial: contadores simples; `MAX_DIRECTOS` por nó | `config.rs:523`, `usage.rs:168` | medição de minutos/egress, política de excesso | E | F1/F3 |
| RF-24 | API/eventos | parcial: OpenAPI, erros uniformes; sem idempotência, sem retries de webhook, sem timestamp na assinatura | `webhooks.rs:113-146,305` | tudo para TV | E | F1 |
| RF-25 | Direitos | ausente: banner de gravação sem aceitação registada | `signaling.rs:4106-4133` | registo de consentimento/direitos | N | F2 |
| RF-26 | Publicidade | ausente | — | pós-F3 | N | F3 |

## 3. Matriz RNF

| ID | Estado | Nota | Fase |
|---|---|---|---|
| RNF-01 isolamento | parcial: RLS só em `employee_groups`; `rooms/meetings/recordings` sem `org_id` (`0024`, `check-tenant-rls.sh:12`) | tabelas de TV nascem com `org_id` e RLS | F1 |
| RNF-02 segurança | parcial: SSRF de webhooks completo; RTMP incorrecto (B2); segredos de destino cifrados (`secret_box.rs`) | F1 |
| RNF-03 privacidade | parcial: retenção por org; sem *legal hold* nem por canal | F2 |
| RNF-04 disponibilidade | não verificável: sem SLO, alertas ou dashboards (find = vazio) | F1 |
| RNF-05 latência | ausente: sem medição; sem LL-HLS | F1/F3 |
| RNF-06 continuidade | ausente | F2 |
| RNF-07 escalabilidade | ausente: só teste de 20 xstack (`docs/ops/teste-de-carga-2026-09-17.md`) | F3 |
| RNF-08 qualidade A/V | não verificável; sem RTCP SR/NTP no SFU | F1 |
| RNF-09 recuperação | parcial: `fail_stale_processing` marca falhada, não recompõe | F2 |
| RNF-10 concorrência | ausente: sem lease, ETag nem idempotência | F1 |
| RNF-11 observabilidade | parcial: logs JSON + request-id; sem traces (grep otel = 0) | F1 |
| RNF-12 eficiência | parcial: ffmpeg no cgroup do SFU (1000m/1Gi), sem semáforo global | F1/F3 |
| RNF-13 compatibilidade | ausente: sem matriz de encoders/browsers | F1 |
| RNF-14 UX estado real | parcial: relatório WS de 2 s por destino; estado de estúdio é local | F1 |
| RNF-15 WCAG | parcial: ~1006 aria/role, 14 handlers de teclado, sem auditoria | F2 |
| RNF-16 manutenibilidade | em curso (ADR-0004/0006) | — |
| RNF-17 deployment | parcial: PDB, drain, probes ok; PVC RWO; ffmpeg ausente (B3) | F1 |
| RNF-18 testabilidade | parcial: 33+33+21 testes unit de directo/gravação; e2e do RTMP real **fora do CI** (`scripts/e2e-fora-do-ci.txt`) | F1 |
| RNF-19 portabilidade | parcial: sem object storage (grep s3 = 0) | F2/F3 |
| RNF-20 admissão | parcial: tecto por contagem, não por carga; ADR-0003 portão 2 «por medir» | F1 |

## 4. Critérios de aceitação — onde estamos

CA-01 **falha** (B7: ISO grava todos); CA-02 passa só no browser; CA-04 **falha** (B4); CA-05 **falha**
(sem canal/continuidade); CA-06/21 ausentes; CA-07 **passa** (um ffmpeg por destino, testes
`broadcast.rs:2440-2620`); CA-08 **falha**; CA-09 parcial (rotação de chave de destino existe);
CA-10 parcial (sem teste cross-org para directo); CA-11 **falha** (B5); CA-12 **falha**; CA-13/14/15 ausentes;
CA-16 parcial; CA-17 ausente; CA-18 a medir com `make fitness` e a bateria completa.

## 5. Limites desta auditoria

Não corrida de testes nem compilação. Não verificados: imagens publicadas fora do repo, overlays
`deploy/k8s-overlays/`, runtime real, qual de `:8180`/`:8181` serve `/metrics`, ADR-0007 (citado,
inexistente), licenciamento do ffmpeg/x264 a empacotar, rota de upload da gravação do Program feita
no browser.
