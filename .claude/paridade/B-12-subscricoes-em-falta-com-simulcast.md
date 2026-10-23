---
gap: B-12
grupo: B
title: Com simulcast, uma sala de 40 perde 8 a 15 % das subscrições de vídeo
severidade: ALTO — é o primeiro limite real da sala, antes da capacidade
estado: MEDIDO, causa por provar
medido: 2026-10-21
base: develop @ 19263ba1
---

Objectivo: numa sala de 40 a 48 pessoas com simulcast, cada pessoa vê **todas** as outras que publicam vídeo, durante a reunião inteira.

Done when:

* A rampa do loadgen **com simulcast** dá 100 % dos fluxos esperados aos 40 e aos 48 (critério normal, ≥ 98 %), em **duas corridas seguidas**, com `video_ativos_min` igual ao esperado. A observação é a contagem de fluxos do gerador e o `delonix_sfu_layer_switch_failures_total` do `/metrics` a zero ou explicado. Não serve uma linha de log nem um teste do SFU por chamadas directas
* A causa fica **nomeada e provada com controlo negativo**: um teste (em `sfu_e2e` ou em `server/tests/`) que reproduz a perda **sem** a correcção e passa com ela. Se a causa for o recurso do `switch_layer` (hipótese abaixo), o teste força a recusa do `replace_track` e a falha da re-subscrição, e mostra que o subscritor recupera o vídeo
* Uma subscrição que falha deixa de ficar perdida em silêncio. Ou se tenta outra vez, ou o peer é reavaliado, ou o cliente é avisado; e o que se escolher fica escrito no comentário do sítio

Fora de âmbito: a capacidade da sala (`A-04`, que volta a medir-se depois disto); a entrada em série (`B-06`, fechado: não é a causa medida); o simulcast do ecrã partilhado (`webrtc.ts`, declarado em falta).

Evidência já lida (2026-10-21, `19263ba1`; relatório completo em [`docs/ops/teste-de-carga-2026-10-21-sala-grande.md`](../../docs/ops/teste-de-carga-2026-10-21-sala-grande.md)):
- **Seis corridas seguidas com simulcast, todas a reprovar por fluxos em falta:** 1324/1560 e 1441/1560 aos 40; 2020/2256 e 2015/2256 aos 48; 1927/2256 e 1644/2256 aos 48 com 300 ms entre entradas. Perda de media nula, máquina entre 22 e 46 %: **não é capacidade**
- **Sem simulcast, a mesma sala aos 40 tem 1560/1560**: o defeito só existe com simulcast
- **Entradas mais lentas pioram** (73–85 % com 300 ms, contra 85–92 % com 40 ms): não é uma corrida na entrada
- **No cliente:** a última oferta do servidor já traz menos vídeos do que os publicadores, e `erros_nego` está vazio. Quem falta subscrever é o SFU
- **No servidor**, só nas corridas com simulcast: `sfu layer switch failed error=connection closed` (10 a 25) e `sfu subscribe failed` (2 a 28). As de 300 ms caem dentro da corrida, com a sala cheia
- **A hipótese, por provar:** no `switch_layer` (`server/src/sfu.rs`, perto do `tracing::warn!(… "sfu layer switch failed")`), quando o `replace_track` recusa, a pista é retirada (`remove_track`), sai do `subscribed` e subscreve-se de novo. Se o `subscribe_layer` falha, só se conta (`sfu_layer_switch_failures_total`, cuja ajuda já diz «subscritor sem vídeo») e regista. Nada reavalia aquele peer até ao próximo evento da sala, e numa sala estável esse evento não chega
- É o «Defeito 2» do [relatório de 2026-09-17](../../docs/ops/teste-de-carga-2026-09-17.md), que lá «não reproduz». Reproduz-se com simulcast

Portões em risco: a catraca do clippy (`sfu.rs`); o `sfu_e2e` (a correcção não pode partir a troca f→h→f→q→f da R156); e o portão de arquitectura, se a correcção mover código do SFU. Revisor: `delonix-meet-webrtc`, com a pergunta explícita de **porque é que o `replace_track` recusa** com esta frequência.
