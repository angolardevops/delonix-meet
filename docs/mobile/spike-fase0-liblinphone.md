# Spike da Fase 0 — o liblinphone contra o FreeSWITCH do laboratório

**Data:** 2026-10-08 · **Decisão que testa:** ADR-0022 · **Licença deste código:** o liblinphone é AGPLv3
ou comercial; o spike corre **só em builds de debug** e nenhum binário saiu da equipa.

## O que se mediu

Emulador Android 15 (x86_64, sem janela, sem áudio) contra o laboratório do Meet em modo LAN, com o perfil
TLS dos ramais (#274). SDK: `org.linphone.no-video:linphone-sdk-android:5.4.127` (o núcleo reporta `5.4.126`).
A configuração é o XML `lpconfig` que o servidor entrega por QR, carregado **tal e qual** com
`Config.loadFromXmlString`; nada do mapeamento é nosso.

| Medida | Resultado |
|---|---|
| Registo com o XML do QR | **Ok por TLS**, em 354 a 758 ms (corridas diferentes; mediana ≈ 590 ms) |
| Controlo negativo: sem a raiz de laboratório | **Falha** («io error»): o SDK confere o certificado e recusa |
| Chamada a sair para o número de acesso (8000) | `StreamsRunning` em 75 a 83 ms depois de marcar; **SRTP**, **Opus/48 kHz** |
| Áudio da chamada a sair (IVR → app) | 28,8 kbit/s a chegar, 30 a 36 kbit/s a sair, perda 0 %, *jitter buffer* ≈ 60 ms |
| Chamada a entrar no ramal (originada pelo FreeSWITCH) | tocou em 63 a 196 ms, atendida, `StreamsRunning` com **SRTP** e **Opus/48 kHz**, 60 kbit/s a chegar |
| Sequência de estados a entrar | `IncomingReceived` → `Connected` → `StreamsRunning` |

Quatro testes Patrol (`integration_test/spike_liblinphone_test.dart`), 4 em 4 na corrida final. A
garantia de licença (um build de release não leva o SDK) está medida no fim.

## O que isto confirma do ADR-0022

- **O servidor já fala a língua do SDK.** O XML do QR (R278) regista tal e qual, por TLS, com SRTP
  obrigatório e sem nenhum mapeamento nosso.
- **A interoperabilidade medida** (SDES-SRTP + Opus 48 kHz + TLS) é a que o servidor já negoceia: zero
  trabalho de servidor para media.
- **O SDK tem o estado `PushIncomingReceived`** no seu modelo de chamada (visto na API do AAR): o fluxo de
  push é parte do desenho dele. **Não foi exercitado.**

## O que o spike NÃO mede

- **Toque com a app morta (a pergunta principal da Fase 0).** Continua por provar: precisa da pasarela de
  push do servidor (S-01, S-02) e de FCM. Aqui a app esteve sempre em primeiro plano.
- **RTT e perda reais.** O emulador fala com o laboratório no mesmo anfitrião: RTT ≈ 0 e perda 0 não dizem
  nada sobre uma rede móvel. Sem CGNAT (o `cgnat.sh` não foi combinado com o SDK).
- **Qualidade de áudio:** o emulador corre sem áudio (`-no-audio`); mediu-se o fluxo RTP, não o som.
- **Gama baixa** (API 26, 2 GB, RNF-41): não corrido.
- **iPhone:** nada. O SDK iOS (SPM desde a 5.4) e o CallKit/PushKit exigem macOS e conta Apple.
- **Foreground service** (`CoreService`, tipo `phoneCall`), Doze e restrições de fabricantes: fora do spike.
- **Carga do anfitrião:** a máquina esteve com carga média de 25 a 46 (outras sessões); os tempos de
  arranque do emulador e dos testes não são representativos, os de rede dentro do emulador menos ainda.

## Achados de engenharia

- **O `playback` de um tom acaba sozinho:** o meu gatilho fazia o servidor desligar a chamada ao fim de 1 s.
  Foi defeito do gatilho, não do SDK (`endless_playback` mantém a chamada).
- **A automação do diálogo do microfone é frágil com a máquina carregada:** numa corrida o diálogo foi
  tocado e a permissão ficou concedida sem o pedido voltar ao Dart; noutra não ficou concedida. Os testes de
  chamada passaram a dar o microfone por `adb pm grant` (comando fixo, no anfitrião).
- **`Core.setRootCaData` não tem *getter*:** em Kotlin chama-se o método, não uma propriedade.
- **Depois de um reinício da máquina**, `make compose-up` falha se houver contentores parados de antes:
  `make compose-down` primeiro (o Postgres recuperou sozinho de um arranque sujo).

## A garantia de licença

O liblinphone entra por `debugImplementation`; `src/release` e `src/profile` levam um stub que responde
`motor_indisponivel`. Prova no fim deste documento.

## Próximo passo

1. Pasarela de push do servidor (S-01, S-02) e FCM: **a pergunta que decide a Fase 0**.
2. O dono pedir a cotação à Belledonne (ADR-0022): sem contrato nenhum binário sai.
3. Repetir o spike em gama baixa e com o `cgnat.sh` à frente do servidor.
4. Um portão de CI que falhe se o APK de release tiver `org/linphone` (hoje só está medido à mão).

## Prova: o APK de release não leva o liblinphone (medido)

`flutter build apk --release` compila (o stub de `src/release` responde `motor_indisponivel`).

| APK | Entradas `linphone`/`mediastreamer`/`bctoolbox`/`belle-sip`/`ortp` | `org/linphone` nos `classes*.dex` |
|---|---|---|
| **release** (67,6 MB) | **0** | **0** |
| debug (controlo positivo) | 26 | presente |

Nenhum binário distribuível pode, portanto, conter código AGPL antes de existir contrato. Isto mede o
build **local**; a CI ainda não tem um portão que o prove em cada PR (ficou por fazer: ver o próximo passo).
