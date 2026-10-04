# RFC-0001 — Estúdio de produção e estação de TV digital no Delonix Meet

**Estado:** Rascunho para decisão · **Data:** 2026-10-03 · **Base:** `origin/main` `d1c01f7d`
**Evidência:** [auditoria-2026-10-03.md](auditoria-2026-10-03.md). Sem estes factos esta RFC não vale.
Marca **[a verificar]** tudo o que depende de versão, licença ou desempenho de componente externo que
ainda não foi medido; nenhum desses valores deve ser copiado como facto.

## 1. Problema e objectivos

O Meet serve reuniões. O pedido é um segundo produto sobre a mesma fundação: realizar programas com
convidados remotos, e emitir **canais** de TV pela Internet com programação contínua, gravação e
arquivo. Hoje o estúdio existe só no browser e o directo só vive enquanto o browser do anfitrião está
aberto. Objectivo: separar **emissão** (servidor, persistente) de **realização** (cliente, efémera) e
de **reunião** (sala interactiva), sem quebrar nada do que funciona.

Fora de âmbito: terrestre/cabo/satélite; inserção dinâmica de anúncios por espectador (RF-26, pós-F3);
DRM.

## 2. Estado actual (resumo; detalhe na auditoria)

Browser compõe+codifica H.264 (`studio/compositor.ts`, `directo.ts`) → WebSocket → ffmpeg `-c:v copy`
por destino RTMP(S) (`broadcast.rs`). SFU próprio em webrtc-rs vendorizado, só reencaminha RTP. Sem
ingest externo, sem HLS, sem player, sem entidades de canal/programa no servidor, sem imagem com ffmpeg,
quatro falhas de segurança/correcção bloqueantes (B1–B4).

## 3. Âmbito e exclusões

Dentro: canais, programas, estúdio com estado no servidor, ingest, composição server-side,
HLS/LL-HLS, player, gravação do Program, grelha, playout, quotas, auditoria. Fora: ver §1.

## 4. Terminologia

Reunião (`rooms`/`meetings`, existente) · Estúdio (sessão de realização ligada a uma sala) · Programa ·
**TvChannel** (nome interno; `Channel` já existe com outro sentido em `domain/conferencing/channels.rs`) ·
Emissão (`BroadcastSession`, execução do sinal) · Destino (`stream_destinations`, existente) · Espectador.

## 5. Casos de uso

1. Entrevista: realizador prepara convidado nos bastidores, põe-no no ar, grava o Program.
2. Programa em directo para RTMP de terceiros + player próprio.
3. Canal 24/7: grelha de programas gravados e directos; continuidade quando falta conteúdo.
4. Encoder externo (OBS, vMix, câmara SRT) como fonte de contribuição.

## 6. Arquitectura proposta

Quatro planos, sem obrigar a microserviços:

- **Controlo** (no `delonix-server`): canais, programas, grelha, comandos, quotas, permissões,
  estado desejado vs observado. Postgres é a fonte de verdade.
- **Media**: um **Channel Engine** (processo/worker próprio, imagem própria com ffmpeg) por emissão
  activa. Recebe o estado desejado (Program, cena, grafismo) e produz o sinal final. Não corre no pod
  do SFU: hoje ffmpeg partilha o cgroup 1000m/1Gi do SFU, o que a auditoria marca como risco.
- **Distribuição**: origem HLS/LL-HLS + player; CDN à frente; destinos RTMP externos.
- **Operação**: métricas, auditoria, alertas, runbooks.

Princípio: **o Program vive no servidor**. O browser do realizador envia *comandos* (cortar, misturar,
mostrar título) e vê o estado confirmado; o Channel Engine executa. Isto resolve CA-04 e CA-05 e
permite multi-operador.

## 7. Fluxos de media e controlo

```
convidado ─WebRTC─► SFU ──(retorno RTP)──► Channel Engine ──► HLS origem ─► CDN ─► Player
 encoder ──WHIP/RTMPS/SRT──► ingest ───────►     │  └────────► RTMP(S) destinos (1 saída por destino)
 realizador ─comandos (WS/HTTP, versionados)─► Controlo ─estado desejado─► Channel Engine
                                                    ◄─estado observado/eventos──┘
```

Duas opções para o Channel Engine, a decidir em §18 (D1): **(A)** compositor server-side com
ffmpeg/filter_complex alimentado pelo SFU e ingest; **(B)** manter composição no browser mas passá-la
a um «browser headless» server-side. B reaproveita `compositor.ts` mas traz um Chromium por canal
**[custo a medir]**; A é mais barato por canal e determinístico, mas reescreve layouts e grafismo.
Recomendação provisória: **A** para a emissão de canal (24/7) e **manter o compositor de browser** como
modo «estúdio rápido» já existente, até a paridade existir.

Ingest e HLS: avaliar um componente maduro (por exemplo MediaMTX para WHIP/RTMP/SRT/HLS; ffmpeg ou
GStreamer para composição) em vez de implementar protocolos **[versões, licenças e LL-HLS a verificar
em fonte oficial antes de escolher]**. Protocolos de referência a confirmar: RFC 9725 (WHIP), RFC 8216
(HLS), RFC 8825/8834/8835/8827 (WebRTC); LL-HLS **não** é coberto por RFC 8216 (extensão da Apple,
a consultar à parte); SRT e RTMPS segundo documentação do fornecedor.

## 8. Modelo de domínio (novas tabelas, todas com `org_id` e RLS desde o início)

`tv_channels`, `tv_programmes`, `tv_rundowns`/`tv_segments`, `tv_scenes`, `tv_sources`,
`tv_broadcast_sessions`, `tv_schedule_entries`, `tv_playlists`, `tv_caption_tracks`,
`tv_usage_records`. Reutiliza: `stream_destinations` (liga a canal), `recordings` (+capítulos/legendas),
`audit_logs` (cadeia hash existente), `org_roles`/`Capability` (ADR-0008).
Separar **spec** (estado desejado, versionado com coluna `version` e `If-Match`), **status** (observado,
escrito só pelo executor) e **histórico** (`tv_broadcast_sessions`/eventos). Sem booleano `live`.

## 9. Máquinas de estado (a detalhar no ADR de estados)

- **Canal:** `draft → idle → starting → on_air → degraded → stopping → idle`; `suspended` por quota/admin.
- **Emissão:** `scheduled → preparing → live → ending → ended | failed`.
- **Fonte:** `declared → connecting → healthy → degraded → lost`.
- **Destino:** `idle → connecting → live → backoff → failed` (hoje: `Connecting|Live|Error|Stopped`, `broadcast.rs:384`).
- **Gravação:** `recording → finalising → validating → available | failed` (hoje `ready` antes de validar; B5).

Cada transição documenta responsável, pré-condição, timeout, compensação e recuperação.

## 10. Contratos de API e eventos

`/api/v1/tv/...` com OpenAPI gerado (convenção existente, `check-openapi.sh`). Todas as mutações:
`Idempotency-Key`, `If-Match` (versão), erro `{error, code, details, request_id}`, cursor de paginação.
Eventos (webhook assinado **com timestamp** + dedupe por `X-Delonix-Delivery`, retries limitados com
backoff — hoje inexistentes): `tv.broadcast.started`, `tv.source.lost`, `tv.program.changed`,
`tv.recording.available`, `tv.quota.near_limit`.

## 11. Segurança e isolamento

Corrigir **antes** de qualquer canal: B1 (exigir capacidade `tv.broadcast.go_live`, distinta de
`tv.prepare`, e impor `owner/adm` no `ws_directo` actual), B2 (passar URLs de destino por `net_guard`
com resolução e bloqueio de IP privado), B8 (chaves fora da query string). Credenciais de fonte por
token com rotação/revogação; ingest só aceita sessões autenticadas. Testes cross-org para cada
recurso novo. Bastidores nunca entram no Program sem comando autorizado e auditado.

## 12. Capacidade e custos

Cada canal activo = 1 Channel Engine (CPU de composição + encode por perfil de saída) + saída HLS +
N saídas RTMP. Nada disto está medido. Plano: benchmark por perfil (720p30/1080p30) num worker
dedicado, registar CPU/RAM/rede por canal, e só então fixar `MAX_CANAIS_POR_WORKER` e admissão por
capacidade medida (substitui o tecto por contagem de `config.rs:523`). Custos de CDN e armazenamento
reportados por `tv_usage_records`. **Sem números nesta RFC até haver medição.**

## 13. Falhas e recuperação

Executor com **lease + fencing token** em Postgres (padrão já usado na transcrição, `transcription.rs`):
dois executores não podem correr o mesmo canal. Reinício → o novo executor reclama o lease e retoma
pela `tv_broadcast_sessions`. Perda de fonte → política do canal (reserva, slate, regresso). Destino a
falhar isola-se (já verdade em `broadcast.rs`). Gravação: segmentos válidos recuperáveis; varrer
`tmp-*` órfãos no arranque (hoje só marca `failed`).

## 14. Migração e compatibilidade

Nada do existente é removido. O directo RTMP actual fica como «modo rápido» atrás da sua rota;
passa a exigir capacidade. Feature flag `TV_STUDIO_ENABLED` por org. Migrações aditivas.
Imagem do servidor inalterada; **nova imagem** a imagem do Channel Engine (nome a fixar) com ffmpeg (D3: GPL, ver ADR-0015).

## 15. Alternativas e trade-offs

Compor no browser (barato, sem servidor, mas ponto único e sem 24/7) · LiveKit Egress/mediasoup
(substituir o SFU próprio: fora de âmbito, o HARNESS fixa «não LiveKit/mediasoup») · serviço gerido
de TV online (rápido, mas contraria soberania e custo) · GStreamer vs ffmpeg para composição
**[comparar em protótipo]**.

## 16. Plano de implementação (por incrementos verificáveis)

- **F0.5 — Correcções de segurança e honestidade** (pequenas, independentes, valor imediato):
  B1, B2, B5, B6, B8; ffmpeg na imagem ou recusa clara; quota na gravação server-side.
- **F1 — Estúdio em directo:** `tv_channels`/`tv_programmes`/`tv_broadcast_sessions` + API; capacidades
  `tv.*`; Channel Engine com 1 perfil; ingest WHIP+RTMPS; HLS+player; gravação do Program validada;
  auditoria e eventos; continuidade básica.
- **F2 — Estação:** grelha, playlists, playout com lease, arquivo/VOD, legendas, direitos.
- **F3 — Escala:** multi-canal, ABR, CDN, custos, publicidade agendada.

## 17. Critérios de aceitação

CA-01…CA-18 do prompt, cada um com teste comportamental nomeado na matriz. Metas de latência/capacidade
só passam a «cumpridas» com relatório reproduzível (cenário, ferramenta, amostra, percentil).

## 18. Decisões pendentes (precisam do dono do produto)

- **D1** Channel Engine: composição server-side (A) ou browser headless (B)? *(recomendo A; confirmada em 2026-10-04 — ver ADR-0015, Proposto)*
- **D2** Origem HLS/ingest: componente externo (MediaMTX ou equivalente) ou módulo próprio? *(recomendo externo; confirmada em 2026-10-04; versão e licença verificadas, desempenho **não** — ver ADR-0015)*
- **D3** Licenciamento do ffmpeg/x264 a empacotar (LGPL vs GPL) — **decidida em 2026-10-04: GPL com libx264 no Channel Engine, sem `nonfree`; o servidor fica LGPL** (ver ADR-0015). A validação jurídica (distribuição, patentes) continua por fazer.
- **D4** Nome `TvChannel` e prefixo `tv_` aceites?
- **D5** Object storage (S3-compatível) como pré-requisito de F2, ou continuar PVC (RWO limita multi-nó)?
- **D6** Abrir F0.5 já, em PRs separadas, antes de F1?
