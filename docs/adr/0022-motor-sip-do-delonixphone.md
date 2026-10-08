# ADR-0022 — O motor SIP do DelonixPhone e o seu licenciamento

**Estado:** Proposto · **Data:** 2026-10-08 · **Decisão técnica delegada pelo dono do produto;
a compra e o contrato da licença são dele** (ver «O que o dono tem de fazer»).
**Contexto:** `docs/mobile/delonixphone-requisitos.md` §4 e a pergunta Q2.

## Decisão

1. **Motor v1: liblinphone (Linphone SDK, linha 5.4 ou posterior; a versão exacta fixa-se na Fase 0)**,
   atrás da porta `SipEngine`. Os adaptadores nativos (Kotlin e Swift) escrevem-se na casa.
2. **Licença: comercial, da Belledonne Communications, para qualquer binário que saia da equipa**
   (lojas, TestFlight, pistas de teste da Play). **AGPLv3 só em desenvolvimento, laboratório e CI**, em
   builds que ninguém de fora recebe. Enquanto o contrato não existir, **nenhum binário sai da equipa**.
3. **Plano B, decidido já** para não ficar refém de uma negociação (critérios na secção «Saída»):
   WebRTC com SIP sobre WSS (`flutter_webrtc` e `sip_ua`, ambos MIT). O Siprix (comercial) fica como B2.

## Contexto — o que se mediu e o que se leu

**No repo (`origin/develop`, 2026-10-08):**

- O servidor já emite o QR de provisionamento (R278) como **configuração `lpconfig` do Linphone**
  (`extension_provisioning.rs`): `media_encryption=srtp`, `media_encryption_mandatory=1`, `auth_info_0`,
  `proxy_0`. Medido: o resgate devolve esse XML (6 testes Patrol contra o laboratório).
- A interoperabilidade foi toda medida **com um Linphone**: Opus 48 kHz com o ramal (ADR-0018), o ramal
  na sala, a central do cliente.
- O perfil dos ramais exige **SDES-SRTP** (`rtp_secure_media=mandatory`, ADR-0009/0010) e agora tem **TLS
  na 5071** (#274).

**Nas fontes (consultadas hoje; a licença e o preço não são aconselhamento jurídico):**

| Opção | Licença | O que pesa |
|---|---|---|
| **liblinphone** | [AGPLv3 ou proprietária, paga, para código fechado](https://wiki.linphone.org/xwiki/wiki/public/view/Lib); o preço não é público | A Belledonne diz que, num produto de cliente, valem na prática os termos da GPLv3. Num app de loja com código nosso fechado, ou se abre o código ou se compra. [Suporta provisionamento remoto de `linphonerc` em XML](https://wiki.linphone.org/xwiki/wiki/public/view/Lib/Features/Remote%20Provisioning/) (`set_provisioning_uri`) |
| **PJSIP** | [GPL v2+ ou proprietária (Teluu)](https://pjsip.org/licensing.htm) | Mesma estrutura. Um blog de terceiros fala em 7 500 GBP/ano; **não confirmado**, pedir cotação |
| **baresip** | BSD-3 | [Há apps Android](https://apt.izzysoft.de/fdroid/index/apk/com.tutpro.baresip?repo=main) e um driver de áudio para iOS, mas **nenhuma app iOS documentada** nos resultados; sem push por desenho nessas apps |
| **Siprix** | Comercial; [trial com chamadas de 60 s](https://dev.to/vladyslav_havrylevskyj_dd/introduction-to-the-voip-solution-for-flutter-by-siprix-2ii0), preço desconhecido | Plugin Flutter com TLS, SRTP e [PushKit+CallKit embutidos](https://pub.dev/documentation/siprix_voip_sdk/latest/) (segundo o próprio fornecedor); binários fechados (RNF-49, soberania) |
| **flutter_webrtc + sip_ua** | MIT | [`flutter_webrtc` muito activo](https://pub.dev/packages/flutter_webrtc) (1,36 k *likes*). O [`sip_ua`](https://pub.dev/packages/sip_ua) está na 1.1.0, publicada há ~10 meses, com ~7 mil descargas: **só WebSocket e TCP, exige DTLS-SRTP (sem SDES)**, e falha com «sem fingerprint DTLS» se o servidor não o anunciar |

O [wrapper `linphone_flutter_plugin`](https://pub.dev/documentation/linphone_flutter_plugin/latest/) que
existe no pub.dev cobre só **Android**; para iPhone escreve-se a ponte (o SDK tem [vinculações Swift e
Kotlin/Java, e SPM desde a 5.4](https://linphone.org/en/download/)).

## Porquê o liblinphone

1. **O servidor já fala a língua dele.** O QR do R278 é o formato de provisionamento do Linphone: o SDK
   consome-o sem mapeamento. A app deixa de traduzir XML à mão (o `contaDeLpconfig` em Dart serve a
   validação e os testes, não o runtime).
2. **Zero trabalho de servidor para media.** SDES-SRTP, TLS e Opus são o que o FreeSWITCH já negoceia. A via
   WebRTC obriga a um perfil `wss` com DTLS-SRTP e a provar que o `rtp_secure_media=mandatory` global não
   recusa ofertas DTLS (a [documentação](https://developer.signalwire.com/freeswitch/users-and-endpoints/webrtc-sip/)
   diz que a variável é de SDES; **não medi** o que acontece com o nosso perfil).
3. **Menos risco funcional.** Espera, transferência, DTMF, várias contas, vídeo e conferência são
   comportamento já maduro do SDK; na via WebRTC escreve-se sobre um `sip_ua` pouco usado.
4. **É o que o dono pediu:** «parecido ao Linphone, completo».

## O que esta escolha custa (sem esconder)

- **Licença paga, preço desconhecido.** Não consigo pedi-la nem comprá-la.
- **Dois motores de media na app** quando entrar a sala com vídeo (RF-51): o liblinphone para chamadas SIP e
  o `flutter_webrtc` para o SFU. Disputam a sessão de áudio e o cancelamento de eco. **Não medido**; até
  lá, a sala entra por áudio com a ponte SIP (RF-50). Medir na Fase 3.
- **Duas pontes nativas** (Kotlin e Swift) para manter, sem plugin iOS pronto.
- **O push é do servidor, não do motor.** Nenhum motor acorda uma app morta sem o servidor (S-01, S-02).
  Desenhar com os parâmetros de push no `Contact` do RFC 8599 (`pn-provider`, `pn-param`, `pn-prid`), que
  é, pelo que sei, o que o Linphone usa com o Flexisip; **a confirmar na Fase 0**.

## Licenciamento do resto da app

- O nosso código da app é proprietário; só pode ligar-se ao liblinphone sob a licença comercial.
- `flutter_webrtc` e `sip_ua` são MIT: ficam disponíveis para a sala e para o plano B.
- **Auditar na Fase 0** as licenças de todas as dependências Dart, Gradle e SPM (o repo tem o job SBOM).
- **Codecs:** Opus, G.722, PCMA e PCMU (e VP8 em vídeo) como lista de v1. H.264, AMR e G.729 só depois de
  confirmada a posição de patentes. O wiki fala numa variante sem vídeo
  (`org.linphone.no-video`), útil para a v1 só de voz.

## Saída (quando passamos ao plano B)

Ao fim da Fase 0, **o spike mede** num emulador Android e num aparelho: registo TLS com o XML do QR,
chamada com Opus e SRTP, toque com a app morta. Passa-se ao plano B se **qualquer** uma destas for verdade:

- a Belledonne não responde, ou os termos (preço, redistribuição nas lojas, uso por clientes que alojam o
  Meet) são recusados pelo dono;
- o spike não consegue o toque com a app morta por razões do SDK (e não do servidor);
- a licença comercial não está **assinada** antes da primeira distribuição.

No plano B abre-se o **S-10** (perfil `wss` com DTLS-SRTP no FreeSWITCH, a medir contra o `rtp_secure_media`
global) e a `SipEngine` passa a `flutter_webrtc` com `sip_ua`. A porta existe para isto; trocar o motor não
reescreve a UI.

## O que o dono tem de fazer

Pedir à Belledonne (licensing@ do site oficial) uma cotação com estas perguntas:

1. Preço e modelo (por app, por aparelho ou por ano; se há *royalties*).
2. Distribuição na App Store e na Play Store coberta, incluindo TestFlight e pistas de teste.
3. Uso com adaptadores Flutter/Kotlin/Swift próprios, e **marca branca por organização**.
4. Clientes que **alojam o Meet**: usam o nosso binário ou compilam o deles?
5. Suporte e SLA, política de versões e de correcções de segurança, e *escrow* do código-fonte.
6. Qual variante usar (com ou sem vídeo) e a posição de patentes dos codecs de vídeo.

Em paralelo, confirmar com um jurista a leitura da AGPL num app de loja, caso se queira a via de código
aberto.

## O que este ADR não prova

- Nenhum preço, nenhuma versão exacta do SDK, nenhum teste do SDK neste repo: tudo isto é da Fase 0.
- A leitura das licenças vem das páginas oficiais citadas e de pesquisa de hoje; **não é parecer jurídico**.
- A afirmação sobre o `Contact` do RFC 8599 é conhecimento geral, por confirmar.
- O comportamento do `rtp_secure_media=mandatory` com DTLS não foi medido.
