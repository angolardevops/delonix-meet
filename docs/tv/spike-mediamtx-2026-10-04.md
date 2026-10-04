# Spike local do MediaMTX (2026-10-04)

Fecha **parte** do spike descrito no [ADR-0011](../adr/0011-channel-engine-ingest-e-distribuicao-de-tv.md).
O ADR continua **Proposto**: o que aqui se mediu é um subconjunto, numa máquina partilhada e carregada.

## Cenário

- **Componente:** MediaMTX **v1.21.0** (imagem `bluenviron/mediamtx:latest` que já estava no host; a v1.21.1 de 2026-09-20 não foi testada). Contentor com `--network host`, escutas em `127.0.0.1`. Configuração e scripts em [`spike-mediamtx/`](spike-mediamtx/).
- **Publicadores:** `ffmpeg` 6.1.1 (Ubuntu) com `testsrc2` + `sine`, H.264 (libx264, `veryfast`, `zerolatency`, GOP 30) + AAC. 720p30 a 2,5 Mbps (RTMP) e 720p30 (SRT); 360p30 nos testes de revogação.
- **Autenticação:** `authMethod: http` a apontar para um servidor Python local que decide por uma chave guardada num ficheiro.
- **Host:** 32 CPUs, 30 GiB; **partilhado**, load average entre 4,7 e 10,7 durante o spike (registado). Rede: loopback.

## Resultados

| # | Teste | Resultado |
|---|---|---|
| 1 | Publicar por **SRT** (`streamid=publish:<canal>:<user>:<chave>`) com a chave certa | aceite; pista H264 + AAC; `ready=true` |
| 2 | Publicar por **RTMP** com a chave certa / errada | certa aceite; errada recusada (`allow=false` no log do auth; o ffmpeg falha com «Operation not permitted») |
| 3 | Publicar por **RTMPS** (certificado autoassinado, `rtmpEncryption: optional`, porta 1936) com a chave certa / errada | certa aceite; errada recusada |
| 4 | **Credencial no URL RTMP** | `rtmp://user:pass@host/…` **não funciona** com o ffmpeg: ele não envia o `user:pass@`, e o MediaMTX recebeu credenciais vazias. Funciona por *query*: `rtmp://host/canal?user=…&pass=…` |
| 5 | **Rotação da chave** com sessões abertas | as sessões **já abertas continuam** (RTMP e SRT ficaram `ready` 20 s depois). A autenticação só corre **ao ligar** |
| 6 | **Nova** publicação com a chave revogada / com a nova | revogada recusada; nova aceite |
| 7 | **Expulsar** o publicador pela API de controlo (`POST /v3/rtmpconns/kick/<id>`) | `200`; o ffmpeg do canal caiu (`Broken pipe`) e o caminho deixou de estar `ready` |
| 8 | **LL-HLS** (`hlsVariant: lowLatency`) | playlist mestre versão 10, áudio e vídeo em playlists separadas, `PART-TARGET=0,2` s, `TARGETDURATION=1` s; a primeira resposta é um **302** (`cookieCheck`) e o `session` vai em *query* |
| 9 | **Atraso de empacotamento** da parte mais recente (60 s, 2 amostras/s, n=120) | min 0,00 · **p50 0,11 s** · **p95 0,21 s** · máx 0,23 s |
| 10 | Recursos do contentor com 2 publicadores e 1 leitor a ler playlists | ~38 MiB RAM; CPU 3–8 % de **um** núcleo |

## O que isto significa para o desenho

1. **A revogação (CA-09) não se resolve só no `authHTTP`.** Ele protege a *próxima* ligação. Para cortar uma sessão a decorrer, o nosso plano de controlo tem de chamar a **API de controlo** do MediaMTX para expulsar (teste 7). Isto é requisito do Channel Engine.
2. **A credencial por fonte vai em *query* no RTMP/SRT.** Um encoder real (OBS) deve levá-la no campo «chave de stream» (`canal?user=…&pass=…`) — **não testado com OBS**. O corpo da chamada de autenticação traz a credencial em `password`, `token` **e** `query`: o nosso endpoint de autenticação não pode registar nenhum dos três (RNF-03). O script do spike foi corrigido para isso.
3. **O `cookieCheck`/`session` do HLS** (teste 8) condiciona a cache de CDN e o player: o ficheiro da playlist depende da sessão. A integração com CDN tem de ser desenhada com isto à vista.
4. **A configuração por omissão abre o ICE UDP do WebRTC (`:8189`) e o MoQ (`:8892/8893`) em todas as interfaces**, mesmo com os restantes serviços em `127.0.0.1`. Em produção têm de ser restringidos ou desligados.
5. **O atraso de empacotamento no servidor é pequeno** (p95 0,21 s com partes de 0,2 s). Isto **não é** a latência que o espectador vê: a latência de ponta a ponta soma o encoder, a rede, o buffer do player (tipicamente algumas partes) e a CDN. A meta RNF-05 (LL-HLS p95 ≤ 5 s) **continua sem prova**.

## Limites desta medição (leia antes de citar números)

- **Uma só execução**, 60 s de amostra — não os 30 min do ADR. Máquina **partilhada e carregada** (load 4,7–10,7); sem repetição para estimar variância.
- **Mede só o empacotamento** (relógio de parede do host contra o `PROGRAM-DATE-TIME` da playlist). Não é *glass-to-glass*.
- **Sem WHIP**: o ffmpeg local (6.1.1) não tem o muxer WHIP, e não testei com um browser. A ingestão WHIP continua **por verificar**.
- **Sem leitores reais**: nenhum player, telemóvel ou browser tocou o HLS; o «leitor» só descarregava playlists, pelo que o teste 10 **não representa** a carga de espectadores nem de segmentos.
- **Sem encoder real** (OBS, vMix, câmara SRT); sinal sintético.
- **Versão 1.21.0**, não a 1.21.1; loopback; um servidor; sem CDN.
- O teste de **verificação de certificado** do RTMPS foi **inconclusivo** (o ffmpeg não verifica por omissão) e não conta.
- A **passphrase/encriptação do SRT** não foi testada.

## O que falta para o ADR passar a «Aceite»

WHIP a partir de um browser; leitura de LL-HLS num telemóvel e num browser de secretária (latência visual medida); amostra de ≥ 30 min, repetida, numa máquina quieta; um encoder real; carga de espectadores a descarregar segmentos; e a decisão D3.
