# ADR-0011 — Channel Engine, ingestão e distribuição de TV

**Estado:** Proposto · **Data:** 2026-10-04 · **Contexto:** [RFC-0001](../tv/rfc-0001-estudio-e-estacao-de-tv.md) §6–7, decisões D1 e D2.
**Não é Aceite:** a D3 (licença do ffmpeg/x264) está por responder e a escolha do componente ainda não foi medida.

## Decisão proposta

1. **D1 — composição no servidor.** Um *Channel Engine* (processo próprio, imagem própria, fora do pod do SFU) compõe e codifica o sinal do canal. O browser do realizador envia *comandos*, não media; o estado do Program vive no servidor. Isto é o que torna possível CA-04 (fechar o browser não pára a emissão) e CA-05.
2. **D2 — ingestão e origem HLS por um componente externo maduro**, em vez de os implementarmos: candidato **MediaMTX**, atrás do plano de controlo. O Channel Engine publica nele o sinal final; ele serve HLS/LL-HLS ao player e recebe a contribuição externa (WHIP, RTMP(S), SRT).
3. **A autorização da ingestão fica no nosso plano de controlo**: o MediaMTX pergunta-nos (autenticação HTTP) se uma publicação é permitida, com a credencial por fonte. Revogar uma credencial na nossa base nega a publicação seguinte (CA-09). Não se guardam credenciais de fonte no ficheiro de configuração do MediaMTX.

## O que foi verificado (2026-10-04, em fonte primária)

| Facto | Fonte | Nota |
|---|---|---|
| Licença **MIT** | API do GitHub (`bluenviron/mediamtx`) | |
| Última versão **v1.21.1, 2026-09-20**; push mais recente 2026-10-03 | API do GitHub (releases) | projecto activo |
| Ingestão **WHIP** (`http://…:8889/<nome>/whip`), H264/VP8/VP9/AV1/H265 e Opus/G722/G711 | mediamtx.org, «Publish → WebRTC clients» | a página não especifica a autenticação do WHIP |
| Ingestão **RTMP** (porta 1935) e **RTMPS** (1936); H264/H265/VP9/AV1, AAC/Opus/… | mediamtx.org, «Publish → RTMP clients» | a configuração do certificado do RTMPS **não foi lida** |
| Ingestão **SRT** (porta 8890, `streamid=publish:<nome>`), H264/H265, AAC/Opus | mediamtx.org, «Publish → SRT clients» | passphrase/encriptação **não lida** em detalhe |
| Saída **HLS** (`/<nome>/index.m3u8`), com variante de baixa latência, e escalável por CDN | mediamtx.org, «Read → HLS» | a latência real **não foi medida** |
| Autenticação «interna, HTTP ou JWT»; API de controlo; métricas compatíveis com Prometheus | README do repositório | o contrato exacto da chamada HTTP **não foi lido** |
| **RFC 9725** = *WebRTC-HTTP Ingestion Protocol (WHIP)*, Proposed Standard, 2025 | rfc-editor.org | |
| **RFC 8216** = *HTTP Live Streaming*, Informational, 2017; não menciona LL-HLS | rfc-editor.org | LL-HLS **não** é coberto por esta RFC: a norma a consultar é a especificação HLS da Apple |
| **RFC 8825** = *Overview: Real-Time Protocols for Browser-Based Applications* | rfc-editor.org | |
| ffmpeg: build padrão **LGPL v2.1+**; `--enable-gpl` (p. ex. libx264) torna a build GPL; `--enable-nonfree` (p. ex. libfdk-aac) torna-a não redistribuível. Uma imagem de contentor com ffmpeg tem as mesmas obrigações de distribuição de código-fonte e atribuição | ffmpeg.org/legal.html | base da D3 |

Um erro apanhado nesta verificação: a ferramenta de leitura de páginas apresentou as versões do MediaMTX com datas de 2024; a API do GitHub dá 2026. As datas acima vêm da API.

## O que NÃO foi verificado (e por isso não se decide por ele)

- Latência p95 do LL-HLS no MediaMTX, nem o custo de CPU por canal, nem o número de espectadores por origem.
- Compatibilidade com encoders reais (OBS, vMix, câmaras SRT) e com os browsers/telemóveis-alvo.
- O contrato da autenticação HTTP e se permite revogação com efeito imediato em sessões já abertas (CA-09 fala de *nova* publicação; uma sessão a decorrer é outro caso).
- Como o Channel Engine entrega o sinal composto ao MediaMTX e como se recupera se qualquer um deles reinicia.
- Alternativas não avaliadas ao mesmo nível: GStreamer/ffmpeg directos como origem HLS, nginx-rtmp, SRS. A escolha só passa a «Aceite» depois do *spike* abaixo.

## Consequências

- Mais um serviço a operar (imagem, saúde, métricas, actualizações de segurança), em troca de não implementar WHIP/RTMP/SRT/HLS.
- O Channel Engine ainda precisa de ffmpeg (ou GStreamer) para compor: **a D3 continua a bloquear a imagem**. Enquanto a build for LGPL e sem libx264, a codificação H.264 teria de vir de outro encoder (por exemplo, por hardware); isso tem de ser medido, não suposto.
- O SFU actual não muda. O Meet continua a ser a sala interactiva; o canal é outro plano.

## Spike que fecha esta decisão (antes de passar a Aceite)

1. MediaMTX numa máquina de teste, com autenticação HTTP apontada a uma rota nossa.
2. Publicar por WHIP (browser), RTMPS e SRT (OBS), e ler em HLS e LL-HLS num telemóvel e num browser de secretária.
3. Medir: latência de ponta a ponta (p50/p95, amostra ≥ 30 min), CPU/RAM por canal, comportamento ao revogar uma credencial a meio.
4. Registar cenário, ferramenta, amostra, percentil, resultado e limitações em `docs/tv/` — sem estes números a escolha fica «Proposta».
