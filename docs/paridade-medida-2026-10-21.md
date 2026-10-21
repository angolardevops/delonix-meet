# Paridade remedida — os 24 ids da fila contra a develop de 2026-10-21

**Medido a 2026-10-21, 08:44 (+01:00)** contra `develop` @ `a16ab417b`, na máquina `kaeso-sys-01`.
A árvore da `main` (`7b9ff9c4`, promovida pelo #379) é **igual byte a byte**: `git diff --shortstat origin/main origin/develop` não devolve nada. Estes números valem para as duas.

A medição anterior é a [`paridade-medida-2026-10-18.md`](paridade-medida-2026-10-18.md), contra `f21e6fc47`. **Este documento não a reescreve.** Diz, para cada id da fila, se a linha se mantém, se mudou (com o antes e o depois) ou se não foi remedida (e porquê). Uma quarta opção não existe. «Presumo que se mantém» seria uma quarta.

**O que entrou desde então, medido:** `git rev-list --count f21e6fc47..a16ab417b` dá **51** commits, e `git diff --shortstat f21e6fc47 a16ab417b` dá **143 ficheiros, +12184/−434**. O pedido desta sessão dizia 81 ficheiros e +7126/−316, um número que **não consegui reproduzir** com nenhum recorte. Só `server web` dá 97/+9566/−357, e sem `docs` nem `.claude` dá 117/+10571/−419. Fica o meu número, com o comando ao lado.

## Como se mediu, e o que isso garante

- **Ferramentas:** `/usr/bin/grep`, e não o `grep` da shell da ferramenta (que é ugrep), com `git show f21e6fc47:<path>` para o antes. O script completo está no [Anexo](#anexo--o-script-que-produziu-as-linhas), e quem o correr na raiz da árvore obtém as mesmas linhas.
- **Toda a ausência tem controlo positivo:** o mesmo comando, no mesmo sítio, encontra uma coisa que se SABE estar lá. Dois controlos falharam à primeira passagem, e nenhum veredicto se escreveu com eles nesse estado:
  - **C-01:** procurar `conference` em `voice/freeswitch/dialplan` deu `exit=1`, porque o sítio estava errado. Em `voice/freeswitch` inteiro deu `exit=0` (`autoload_configs/conference.conf.xml`).
  - **C-04:** `grep -c "MIME_TYPE_H264" server/src/sfu.rs` deu `0`, e a contagem estava certa. O H.264 não é registado pelo nosso código: vem do `media.register_default_codecs()` do crate `webrtc`. Corrigido na linha do C-04.
- **Um «mantém-se» é sustentado de duas maneiras:** o grep de hoje dá o mesmo resultado que o de 10-18, **e** o `git diff --shortstat f21e6fc47 a16ab417b -- <ficheiros que sustentam a linha>` sai vazio. O controlo positivo deste segundo critério é o `web/src/room/useTranscription.ts`, que mudou (+18/−2) e o diff apanhou.
- **O que não se correu:** nenhuma bateria de testes, nenhum e2e, nenhuma carga. Nada foi visto num browser nem num telemóvel. Os testes citados existem e foram **lidos**, não executados. A carga desta máquina era 18,5 (média de 1 min) durante a medição, o que não afecta greps mas invalidaria qualquer número de tempo.

## Resumo

| | |
|---|---|
| Ids da fila | **24** (A-01…A-09, A-11, B-01…B-10, C-01…C-04; o `A-10` e o `B-11` estão fora da fila por decisão do `INDEX.md` e remedem-se no fim, à parte) |
| **Mudou** | **3**: `A-01`, `A-02`, `B-07` |
| **Mantém-se** | **21** |
| **Não remedido** | **0** ids. Há partes de linhas que um grep não alcança e que **não** se remederam, nomeadas em cada linha: o T5 do `C-03` e os números de carga do `A-04`, `B-05` e `B-06` |
| Âncoras dos 9 prompts | **6 prompts com âncoras deslizadas**, **3 com âncoras erradas desde que foram escritos**. Detalhe na [secção 3](#3-âncoras-dos-prompts) |
| Afirmações do documento de 10-18 desmentidas por esta medição | 5. Ver a [secção 4](#4-o-que-o-documento-de-10-18-diz-e-deixou-de-ser-verdade) |

---

## 1. Os quatro suspeitos

### A-02 — gravação ISO: **MUDOU**

| | Antes (`f21e6fc47`) | Depois (`a16ab417b`) |
|---|---|---|
| `recording_tracks` | não existia | existe |
| Rotas | nenhuma | `GET /api/recordings/{recording_id}/tracks` e `…/tracks/{track_id}/content` (`server/src/lib.rs:955-959`) |
| Prova | nenhuma | `server/tests/gravacao_iso.rs`: 6 `#[sqlx::test]`, um deles com `ffprobe` sobre o ficheiro **descarregado** pela rota (`:157-207`) |

Comandos e códigos de saída:

```
$ /usr/bin/grep -rl "recording_tracks" server/src server/migrations                    → exit=0, 7 ficheiros
    server/src/{sfu,recordings,metrics,recorder}.rs
    server/migrations/0124_recording_tracks.sql, 0125_recording_tracks_screen_audio.sql, 0126_recording_artifacts.sql
$ git grep -l recording_tracks f21e6fc47 -- server/src server/migrations                → exit=1  (o «zero» de 10-18 reproduz-se no commit dele)
$ git grep -c iso_recording f21e6fc47 -- server/migrations/0081_studios.sql            → exit=0  (controlo positivo: o sítio é o certo)
$ /usr/bin/grep -nE 'tracks' server/src/lib.rs                                          → exit=0, linhas 955 e 957
$ /usr/bin/grep -rln "/tracks" web/src                                                  → exit=1
$ /usr/bin/grep -rln "/chapters" web/src                                                → exit=0  (controlo positivo do anterior)
```

**Fica igual:** `iso_recording BOOLEAN NOT NULL DEFAULT TRUE` (`server/migrations/0081_studios.sql:12`; nenhuma migração posterior lhe muda o `DEFAULT`). Quem decidiu, decidiu **implementar** e não recuar: o critério 1 do prompt escolhe-se pelo ramo «se for implementar».

**O que a medição NÃO mostra que esteja feito**, por critério do prompt:

1. *ffprobe de um ficheiro descarregado com uma só fonte.* **Escrito em teste, não corrido aqui.** E o teste parte de uma **fixture**: o OGG é feito pelo `ffmpeg` do teste e a linha de `recording_tracks` é inserida à mão (`gravacao_iso.rs:96-150`). Prova a rota que serve. **Não prova** que uma sessão do SFU produza a pista. O caminho de produção (`registar_pistas_iso`, `server/src/recorder.rs:1302`) tem testes unitários contra Postgres (`recorder.rs:5432-5720`), que também só se leram.
2. *Nenhum ecrã apresenta «ISO» enquanto não houver pista servida.* Agora há pista servida pela API, mas **nenhum ecrã a consome**. `grep "/tracks" web/src` dá `exit=1`. O interruptor continua no estúdio (`web/src/pages/studio/tv/Fontes.tsx:179-190`, chave `tv.fontes.iso` = `'ISO'` em `web/src/locales/pt/tv.ts:408`). Quem liga o interruptor só chega às pistas por API.
3. *«Duas fontes → dois ficheiros com durações próprias.»* O teste de integração usa **uma** pista. O de duas fontes não está entre os seis.

**Veredicto do prompt:** velho pelo critério 2 da skill (a medição mudou). A premissa «`recording_tracks` = zero» é falsa desde a 0124. O passo seguinte é um `/paridade fechar A-02` que junte a observação dos três critérios, e não reescrever o prompt de memória.

### Legendas e soberania (§1.1 do documento de 10-18, sem id na fila): **MUDOU**

| | Antes (`f21e6fc47`, texto de `d91f3af7`) | Depois (`a16ab417b`) |
|---|---|---|
| Ordem dos motores | Web Speech «último» por soberania escrita, e a falha do `/asr` recuava para ela em Chrome | `servidor → webspeech (só com três sins) → wasm → nenhum` (`web/src/legendasPref.ts:162-171`) |
| Política da organização | não existia: «não há política do lado do servidor que a proíba» | `organizations.allow_web_speech BOOLEAN NOT NULL DEFAULT TRUE` (`server/migrations/0127_org_allow_web_speech.sql:18-19`), escrita por `PATCH` (`server/src/org.rs:233,241`) |
| Recusa no servidor | nenhuma | `recusa_motor` em `server/src/signaling.rs:1843` recusa uma frase com `engine == "webspeech"` numa organização que o proíbe, e conta-a em `delonix_caption_engine_refused_total` (`server/src/metrics.rs:351-353`) |

```
$ /usr/bin/grep -rn "allow_web_speech" server/src                                       → exit=0 (org.rs, metrics.rs, signaling.rs)
$ /usr/bin/grep -rn "caption_engine_refused" server/src                                 → exit=0 (metrics.rs:152,351-353,401; signaling.rs:133,1848,8031,8078)
$ /usr/bin/grep -rinE 'openai|anthropic|azure|bedrock|vertexai|generativelanguage' server/src server/crates --include=*.rs -l   → exit=1
$ /usr/bin/grep -rinE 'ollama' server/src --include=*.rs -l                             → exit=0  (controlo positivo do anterior)
$ /usr/bin/grep -n "allowRemoteModels" web/src/*.ts                                     → exit=0  (whisperWorker.ts:8)
```

**Uma afirmação do documento de 10-18 continua inexacta depois da correcção.** A secção 1.1 (reescrita em `84061084` pela sessão que fez a correcção) diz que «o recuo de uma falha é sempre outro motor soberano». O código diz outra coisa. Com os **três sins** (browser com o motor, organização que o permite, pessoa que o aceitou), uma falha do `/asr` cai na **Web Speech**: a linha 162 deixa passar o servidor em baixo, e as linhas 167-168 devolvem `'webspeech'` antes do `wasm` da linha 170. É uma escolha de desenho com razão escrita (`legendasPref.ts:124-128`: «uma escolha explícita que nunca se cumpre não é uma escolha»), e `legendasPref.test.ts:149` tranca o caso com tudo a sim. O que está certo é: **sem os três sins**, a falha nunca cai na Google. O «sempre» é que é falso.

### Superfície pública: é o **B-07**, que **MUDOU**

| | Antes (`f21e6fc47`) | Depois (`a16ab417b`) |
|---|---|---|
| `.route(` no router | 335 | **338** |
| `v1.json` | 9 caminhos / 12 operações / 21 esquemas | igual |
| `bff.json` | 282 / 404 / 429 | **285 / 407 / 431** |
| `operator.json` | 11 / 13 / 23 | igual |
| `integrations.json` | 6 / 6 / 20 | igual |
| v1 ÷ BFF (operações) | 2,97 % | 2,95 %: continua «3 %» |

Contado com o módulo `json` do Python (operações = chaves `get/post/put/patch/delete/head/options` de cada caminho), no ficheiro commitado em cada commit. Os três caminhos novos da BFF, por diferença de conjuntos:

```
novos  = ['/api/recordings/{recording_id}/tracks',
          '/api/recordings/{recording_id}/tracks/{track_id}/content',
          '/ws/admission']
saídos = []
$ /usr/bin/grep -cE '^\s*\.route\(' server/src/lib.rs                                   → exit=0, 338
$ git show f21e6fc47:server/src/lib.rs | /usr/bin/grep -cE '^\s*\.route\('               → 335
```

Autorização das rotas novas, por leitura:

- **As duas `tracks`** têm extractor `AuthUser` (`server/src/recordings.rs:2622-2627`, `:2667-2669`) e são exercitadas pelo `web/e2e/isolamento.mjs:826-828`, com controlo positivo («B lista as suas») e duas recusas na porta. Com isso, as chamadas `recusado`/`recusadoNaPorta` passam de **349** para **351**.
- **O `/ws/admission`** não tem extractor. Autentica pelo room token na query, através da mesma `admitir()` que o `/ws` usa (`server/src/signaling.rs:4350`, `ws_admission_handler`). Tem linha **com razão** em `scripts/rotas-publicas.txt`, e a razão nomeia o defeito que a rota fecha: um WebSocket recusado no handshake chega como `1006`, e a pessoa via «verifica a rede».

**O que não muda:** a v1 continua com 9 caminhos e o SDK continua a não existir. O B-07 «mudou» no número da BFF, e a lacuna mantém-se do mesmo tamanho.

### A-09 — modo webinar: **MANTÉM-SE**

```
$ /usr/bin/grep -rliw webinar --include=*.rs --include=*.ts --include=*.tsx server web/src                 → exit=0, 3 ficheiros, todos de teste
    server/tests/content.rs (1), server/tests/meeting_session_options.rs (1), web/src/pages/diagrams/exporters.test.ts (2) = 4 ocorrências, como em 10-18
$ /usr/bin/grep -rnE 'format\s*(==|!=)\s*"' server/src --include=*.rs                                      → exit=0, 1 linha: signaling.rs:4785 `if format != "training"`
$ /usr/bin/grep -rliw hls server/src server/crates                                                          → exit=1
$ /usr/bin/grep -rliw rtmp server/src                                                                       → exit=0  (controlo positivo do anterior: 7 ficheiros)
$ git diff --shortstat f21e6fc47 a16ab417b -- server/src/rooms.rs server/src/meetings.rs server/src/broadcast.rs server/src/room_tools.rs   → vazio
$ git diff f21e6fc47 a16ab417b -- server/src/signaling.rs server/src/sfu.rs | grep -E '^\+.*(format|broadcast|hybrid|Attendee)'         → exit=1
```

O `signaling.rs` cresceu 571 linhas (a admissão e a política de legendas), e nenhuma delas lê o formato. O único formato que decide comportamento continua a ser o `training`, nas salas de grupo.

**Nuance que já era verdade em 10-18 e o documento não disse:** o `broadcast` e o `hybrid` não são completamente inertes. Passam a `recordings.kind` (`server/src/rooms.rs:39`, `server/src/recordings.rs:46-53`) e servem de filtro na biblioteca (`web/src/pages/recordings/libraryData.ts:68-70`). Rotulam, mas não mudam o comportamento da sala, e é isso que o A-09 mede.

---

## 2. Os outros vinte ids

Coluna «sustentação»: o diff dos ficheiros que sustentam a linha, entre `f21e6fc47` e `a16ab417b`. «vazio» = `git diff --shortstat` sem saída.

| id | Veredicto | Medido hoje (comando → saída) | Sustentação |
|---|---|---|---|
| `A-01` | **MUDOU** | `grep -c e2ee server/src/voice.rs` → **9** (era **0**); `server/tests/dial_in_e2ee.rs` com 2 `#[sqlx::test]`. A correcção (`3722c5e0`) estava num ramo a 10-18 e entrou na `develop` pelo #378. Controlo positivo: `grep -n dial_out.room_e2ee server/src/dial_outs.rs` → `:309`. **Continua por provar:** a chamada real. Não há DID de teste | — |
| `A-03` | mantém-se | `grep -rliE "vapid\|pushManager\|web-push\|webpush"` em `server web mobile deploy` (código) → **exit=1**; controlo: `serviceWorker` em `web/src` → `main.tsx`, exit=0. `sw.js` sem `addEventListener('push'\|'notificationclick')` → exit=1; controlo: 3 `addEventListener` no mesmo ficheiro. `meetings_v1.rs:946` continua a documentar `offline: [uuid]`; `voice_push_wake.rs:368` continua «não finge» | `sw.js`, `notifications.rs`, `meetings_v1.rs`, `voice_devices.rs`, `mobile/`: vazio |
| `A-04` | mantém-se | `ls docs/ops/` → só os relatórios de **2026-09-17** e **2026-10-05**, e não há rampa nova. Logo, «40 medidos, onde quebra não se sabe» continua a ser o que se sabe. `grep -n max_participants server/src/sfu.rs` → exit=1. **Mudou ao lado, não na lacuna:** sem `NODE_PEER_CAPACITY`, o servidor vale agora `DEFAULT_NODE_PEER_CAPACITY = 50` (`server/src/config.rs:583,927`); antes nada se recusava (`f21e6fc47:server/src/config.rs:577`). Só `0` explícito desliga a admissão. **Não remedido:** nenhum número de carga, porque nenhuma rampa correu | `docs/ops/`: vazio |
| `A-05` | mantém-se | `grep -rliE "graph\.microsoft\|googleapis\|caldav"` em `server/src server/crates web/src` (`.rs/.ts/.tsx`) → **exit=1**; controlo: `text/calendar` em `mail.rs` → `:412`, exit=0 | `mail.rs`, `meetings.rs`, `notifications.rs`: vazio |
| `A-06` | mantém-se | a única rota de chat continua a ser por sala: `lib.rs:822` → `GET /api/rooms/{room_code}/messages` (era `:801`). As outras «channels» do router são de **TV** (`/api/orgs/{org_id}/tv/channels`), que não são canais de conversa. `grep -rliE "team_channel\|chat_channel\|channel_members"` em migrações e `server/src` → exit=1; controlo: `room_chat` em migrações → 5 ficheiros | `room_chat.rs`: vazio |
| `A-07` | mantém-se | `require_mfa` continua a ser lido só para impedir a remoção do último factor (`check_removal`, `server/crates/delonix-meet-domain/src/identity/factors.rs:39-55`) e para o mostrar (`mfa.rs:418`, `passkeys.rs:317,558`). O travão de tentativas continua em processo, num `DashMap` (`server/src/rate_limit.rs:7,25,33`), e a única menção a Redis é o comentário «trocar por Redis ao escalar» (`:21`) | `identity/`, `rate_limit.rs`, `mfa.rs`, `passkeys.rs`, `auth.rs`: vazio |
| `A-08` | mantém-se | `grep -rliE "legal_hold\|legal hold\|ediscovery\|e-discovery"` em `server/src server/crates server/migrations web/src` → **exit=1**; controlo: `fn retention_sweep` → 3 ficheiros | `audit.rs`, `dlp.rs`, `search/`, `data_exports.rs`: vazio. O `recorder.rs` mudou, mas a varredura dele continua sem *hold* |
| `A-09` | mantém-se | ver a [secção 1](#a-09--modo-webinar-mantém-se) | — |
| `A-11` | mantém-se | `grep -rliE "saml\|AuthnRequest"` em `server web/src` → **exit=1**; controlo: `oidc` em `server/src` → 9 ficheiros | `auth.rs`: vazio |
| `B-01` | mantém-se | `grep -n flutter_webrtc mobile/delonixphone/pubspec.yaml` → exit=1; controlo: 16 dependências lidas no mesmo ficheiro. `MotorFabrica.kt:12` (release e profile): «Este build não inclui o motor SIP (licença por assinar).» | `mobile/`: vazio |
| `B-02` | mantém-se | a acta da sala continua heurística: `const actionRe = /(vamos\|temos de\|…)/i` em `web/src/room/useTranscription.ts:24`, a mesma linha que em 10-18. `grep -rliE "o que perdi\|catch.?up\|whatDidIMiss\|recap"` em `server/src web/src` → exit=1; controlo: o próprio `actionRe` | `useTranscription.ts` mudou (+18/−2), mas só a política de legendas (`caption-policy`). O `buildMoM` não mudou |
| `B-03` | mantém-se | `grep -rliE "pgvector\|embedding"` em `server web/src` → **exit=1**; controlo: `tsvector` em migrações → 3 ficheiros | `search/`: vazio |
| `B-04` | mantém-se | `web/src/webrtc.ts:188` → `frameRate: { ideal: 5, max: 15 }` (10-18 citava `:184-189`) | `webrtc.ts`: vazio |
| `B-05` | mantém-se | `FFMPEG_MAX_CONCURRENT` com omissão **1** (`server/src/config.rs:856`, `bounded_env(…, 1, 1, 64)`). A composição continua no mesmo pod. **Não remedido:** os 64 % de perda, que são do relatório de 2026-09-17 e não se re-executaram | `composition.rs` mudou (+51), mas só o vocabulário de `kind` das pistas ISO |
| `B-06` | mantém-se | `reevaluate_room_except` (`server/src/sfu.rs:1826-1837`) continua a percorrer os pares em série: `self.clone().reevaluate_peer(room_id, id).await` em `:1835`. **O corpo é igual byte a byte** ao de `f21e6fc47:server/src/sfu.rs:1812-1865`, medido com `diff`. **Não remedido:** os 1,7 s, que são do relatório de 2026-10-05 | corpo da função: igual |
| `B-07` | **MUDOU** | ver a [secção 1](#superfície-pública-é-o-b-07-que-mudou) | — |
| `B-08` | mantém-se | `server/src/guests.rs:7` «**Sempre pela sala de espera**», e `:149-152` força a espera mesmo com a sala de espera desligada | `guests.rs`: vazio |
| `B-09` | mantém-se | `grep -rliwE "axe\|axe-core\|@axe-core"` em `web/src web/e2e web/package.json` → **exit=1**; `prefers-contrast\|forced-colors` em `web/src` → **exit=1**; controlo: `prefers-reduced-motion\|prefers-color-scheme` → 6 ficheiros. `aria-*` em `.tsx` (o recorte de 10-18): **849 em 161 ficheiros**, 24 `aria-live`, os mesmos números nos dois commits. Com `.ts` incluído dá 893 em 168; em 10-18 o recorte foi só `.tsx`, e não estava escrito | — |
| `B-10` | mantém-se | `const MAX_RESULTS: i64 = 200` em `server/src/scim.rs:60`, e `"totalResults": recursos.len()` em `:412`. `grep -nE "start_index\|startIndex.*Query\|pub count"` → exit=1. `grep -rn Groups server/src/lib.rs` → exit=1 | `scim.rs`: vazio |
| `C-01` | mantém-se | `grep -rliE "call_queue\|voicemail\|message.waiting\|\bmwi\b\|call_park\|ring_group\|\bblf\b\|barge\|eavesdrop"` em `server/src server/crates server/migrations` → **exit=1**; `disable-transfer=true` em `voice/freeswitch/sip_profiles/internal.xml:39`; controlo: `conference` em `voice/freeswitch` → exit=0. **Nuance que já era verdade em 10-18:** a imagem carrega `mod_valet_parking` (`voice/freeswitch/image/modules.conf:10`), mas nenhum dialplan nem script o usa (`grep -rn valet voice/freeswitch/dialplan voice/freeswitch/scripts` → exit=1). Módulo carregado não é estacionamento | `voice/`, `phone_bridge/`, `telephony_*.rs`: vazio |
| `C-02` | mantém-se | `grep -rliE "sip\.js\|jssip\|webphone\|sipjs"` em `web` (`.ts/.tsx/package.json`) → **exit=1**; controlo: 12 `RTCPeerConnection` em `web/src/webrtc.ts` | `mobile/`: vazio |
| `C-03` | mantém-se | `grep -rliE "P-Asserted-Identity\|sip_cid_type=pid"` em `voice/freeswitch voice/kamailio` → exit=1; `session-timeout\|session-expires\|rtp-timeout-sec\|media_timeout` nos perfis → exit=1 (controlo: `internal.xml` existe e foi lido pelo `disable-transfer` acima); tecto de gasto e tranca de internacional (`spend_cap\|spending_limit\|max_spend\|international_block\|block_international`) → exit=1. **Não remedido:** o T5 («nenhuma operadora real alguma vez entregou uma chamada»), que é um facto de operação e não se lê no código | `voice/`, `telephony_*.rs`: vazio |
| `C-04` | mantém-se | `grep -rliwE "h323\|h\.323\|polycom\|webex"` → exit=1. O gravador continua só VP8 (`server/src/recorder.rs:2`; `sfu.rs:2174`: «Se o browser negociar VP9/H264/AV1, o depacketizer VP8 devolve…»). **Correcção de 10-18:** o H.264 vem do `media.register_default_codecs()` (`server/src/sfu.rs:900`, crate `webrtc` 0.17.2 nos dois commits), e a função regista **10** entradas `MIME_TYPE_H264`/`profile-level-id`, não «seis» | `sfu.rs:895-905`: igual |

### Fora da fila, remedidos à parte

| id | Veredicto | Medido |
|---|---|---|
| `A-10` | mantém-se | `find . -iname "*soc2*" -o -iname "*iso27001*" -o -iname "*hipaa*" -o -iname "*fedramp*" …` → 0 resultados; controlo: o mesmo `find` encontra `docs/adr/0001-room-shard-affinity.md` |
| `B-11` | mantém-se | `grep -rliwE "electron\|tauri"` em `package.json`/`Cargo.toml`/`.ts` → exit=1; controlo: `"vite"` em `web/package.json` → 2 |

---

## 3. Âncoras dos prompts

Cada `ficheiro:linha` dos 9 prompts foi relido em `f21e6fc47` e em `a16ab417b`, por script (o segundo bloco do [Anexo](#anexo--o-script-que-produziu-as-linhas)). Foram apanhadas as citações com caminho completo entre crases. As relativas (`:599`, `(:725)`) verificaram-se à mão onde importam.

| Prompt | Âncoras | O que está errado |
|---|---|---|
| `A-01` | **deslizadas** (é o esperado num fechado) | `voice.rs:1208` deixou de existir (a consulta foi reescrita pela correcção); `signaling.rs:2346` → **2420**; `recorder.rs:1750` → **2179** |
| `A-02` | intactas | as âncoras estão de pé, mas **a medição mudou**: `recording_tracks` = 0 é falso. É velho pelo critério 2 da skill |
| `A-03` | intactas | — |
| `A-04` | **deslizadas** | `deploy/k8s/01-config.yaml:23,27` → **25,29**; `signaling.rs:4130` → **4269**; `:4225` → **4490** |
| `A-05` | intactas | — |
| `A-08` | **deslizadas** | `recorder.rs:2486` → **2929**; `lib.rs:2179,2248,2337` → **2239, 2315, 2404** |
| `A-09` | **deslizadas** | `signaling.rs:342` → **361**; `:417` → **436**; `:1473` → **1512**; `:1738` → **1785** |
| `B-06` | **errada desde o nascimento** | `sfu.rs:1689-1703` já era outra coisa em `f21e6fc47` (um `warn!` de gravação degradada). O `reevaluate_room` estava em `:1812` e está hoje em **1822-1837** |
| `B-10` | **deslizada + duas erradas desde o nascimento** | `lib.rs:456` → **477**, e `.nest("/scim/v2")` `:599` → **620** (deslizadas); `scim.rs:64` nunca foi o `MAX_RESULTS`, que está em **`:60`**; `scim.rs:344` era uma linha vazia, e o `ListParams` está em **`:346`**; `:148` não tem o prefixo `dlxs_`, que está em **`:54`**. O `scim.rs` não mudou entre os dois commits: os números estavam errados à nascença |

Resumo: **seis prompts precisam de âncoras relidas antes de se correrem** (`A-01`, `A-04`, `A-08`, `A-09`, `B-06`, `B-10`), e **um precisa de ser fechado ou reescrito** (`A-02`). Três estão de pé: `A-03`, `A-05`, e o `A-01` como registo histórico.

---

## 4. O que o documento de 10-18 diz e deixou de ser verdade

Ninguém lhe toca: é a fotografia de `f21e6fc47`, e a guarda da skill proíbe «actualizá-lo de passagem». Quem o citar precisa de saber isto:

1. **§1.4, «Gravação ISO — NÃO existe», e §5.1:** falso desde a 0124. Ver A-02.
2. **§1.2, «Buraco conhecido: o dial-IN não verifica `rooms.e2ee`», e §5.3:** falso desde o #378. Ver A-01.
3. **§1.1, «o recuo de uma falha é sempre outro motor soberano»:** inexacto. Com três sins, recua para a Web Speech. Ver a secção das legendas.
4. **§3, linha `A-04`, «O chart envia `ORG_MAX_PARTICIPANTS: "40"`»:** quem envia 40 é o **manifesto** `deploy/k8s/01-config.yaml:29`. O **chart** Helm envia `orgMaxParticipants: 20` (`deploy/helm/delonix-meet/values.yaml:186`), e já enviava em `f21e6fc47` (`:175`). Errado à nascença.
5. **§4, linha `C-04`, «seis perfis de H.264»:** são 10, no mesmo crate. Errado à nascença.

E uma nota de processo: a secção 1.1 desse documento **foi editada depois da medição** (`84061084`, 2026-10-18 21:20), pela sessão que corrigiu as legendas. O cabeçalho continua a dizer «medido contra `f21e6fc47`», e esse texto já descreve código que só entrou depois. É precisamente o que a guarda da skill quer evitar.

---

## Anexo — o script que produziu as linhas

Correr na raiz da árvore: `bash <ficheiro>`. Esta é a versão condensada do script que correu: os comandos são os mesmos para cada linha citada. Há duas excepções, corridas à mão depois de o controlo falhar à primeira: o controlo do `C-01` (`voice/freeswitch` em vez de `voice/freeswitch/dialplan`) e o do `B-02` (`const actionRe`). Os comandos dos quatro suspeitos estão na secção 1, e não aqui.

```bash
#!/usr/bin/env bash
set -u
G=/usr/bin/grep
EXC=(--exclude-dir=node_modules --exclude-dir=dist --exclude-dir=target --exclude-dir=.git)
INC=(--include=*.rs --include=*.ts --include=*.tsx --include=*.js --include=*.mjs --include=*.sql --include=*.toml --include=*.xml --include=*.yaml --include=*.yml --include=*.dart --include=*.kt --include=*.conf --include=*.lua --include=*.json)
m() { local id="$1" lab="$2"; shift 2
  local out; out="$("$@" 2>/dev/null)"; local rc=$?
  printf '%s\t%s\texit=%s\tlinhas=%s\t$ %s\n' "$id" "$lab" "$rc" "$(printf '%s' "$out" | $G -c .)" "$*"
  [ -n "$out" ] && printf '%s\n' "$out" | head -8 | sed 's/^/      /'; }
echo "# HEAD $(git rev-parse --short=9 HEAD)  $(date -Iseconds)  $(hostname)"
m A-01 "e2ee no voice.rs"                 $G -c e2ee server/src/voice.rs
m A-03 "AUSÊNCIA push"                    $G -rliE "vapid|pushManager|web-push|webpush" "${EXC[@]}" "${INC[@]}" server web mobile deploy
m A-03 "controlo +"                       $G -rlE "serviceWorker" "${EXC[@]}" "${INC[@]}" web/src
m A-03 "sw.js push"                       $G -nE "addEventListener\(['\"](push|notificationclick)" web/public/sw.js
m A-03 "controlo +"                       $G -c "addEventListener" web/public/sw.js
m A-04 "relatórios de carga"              ls docs/ops/
m A-04 "max_participants no SFU"          $G -n "max_participants" server/src/sfu.rs
m A-05 "AUSÊNCIA calendário externo"      $G -rliE "graph\.microsoft|googleapis|caldav" "${EXC[@]}" --include=*.rs --include=*.ts --include=*.tsx server/src server/crates web/src
m A-05 "controlo +"                       $G -n "text/calendar" server/src/mail.rs
m A-06 "rotas de mensagens"               $G -nE '"/api/[^"]*(messages|channels|chat)[^"]*"' server/src/lib.rs
m A-06 "AUSÊNCIA canais de equipa"        $G -rliE "team_channel|chat_channel|channel_members" server/migrations server/src
m A-06 "controlo +"                       $G -rlE "room_chat|room_messages|chat_messages" server/migrations
m A-07 "require_mfa"                      $G -rn "require_mfa" server/src --include=*.rs
m A-07 "rate_limit em processo"           $G -nE "DashMap|HashMap|Mutex" server/src/rate_limit.rs
m A-08 "AUSÊNCIA legal hold"              $G -rliE "legal_hold|legal hold|ediscovery|e-discovery" "${EXC[@]}" --include=*.rs --include=*.sql --include=*.ts --include=*.tsx server/src server/crates server/migrations web/src
m A-08 "controlo +"                       $G -rln "fn retention_sweep" server/src
m A-09 "webinar"                          $G -rliw webinar "${EXC[@]}" --include=*.rs --include=*.ts --include=*.tsx server web/src
m A-09 "formato decide"                   $G -rnE 'format\s*(==|!=)\s*"' server/src --include=*.rs
m A-09 "AUSÊNCIA hls"                     $G -rliw hls server/src server/crates
m A-09 "controlo +"                       $G -rliw rtmp server/src
m A-11 "AUSÊNCIA saml"                    $G -rliE "saml|AuthnRequest" "${EXC[@]}" --include=*.rs --include=*.ts --include=*.tsx --include=*.toml --include=*.sql server web/src
m A-11 "controlo +"                       $G -rliw oidc server/src
m B-01 "flutter_webrtc"                   $G -n "flutter_webrtc" mobile/delonixphone/pubspec.yaml
m B-01 "controlo +"                       $G -cE "^\s+[a-z_]+:" mobile/delonixphone/pubspec.yaml
m B-02 "AUSÊNCIA catch-up"                $G -rliE "o que perdi|catch.?up|whatDidIMiss|recap" "${EXC[@]}" --include=*.rs --include=*.ts --include=*.tsx server/src web/src
m B-02 "controlo +"                       $G -n "const actionRe" web/src/room/useTranscription.ts
m B-03 "AUSÊNCIA semântica"               $G -rliE "pgvector|embedding" "${EXC[@]}" --include=*.rs --include=*.sql --include=*.ts --include=*.tsx server web/src
m B-03 "controlo +"                       $G -rlE "tsvector" server/migrations
m B-04 "frameRate"                        $G -rnE "frameRate" web/src/webrtc.ts web/src/media.ts
m B-05 "FFMPEG_MAX_CONCURRENT"            $G -rnE "FFMPEG_MAX_CONCURRENT|ffmpeg_max_concurrent" server/src/config.rs
m B-06 "laço em série"                    $G -nE "fn reevaluate_room|reevaluate_peer\(room_id, id\)\.await" server/src/sfu.rs
m B-07 ".route("                          $G -cE '^\s*\.route\(' server/src/lib.rs
m B-08 "espera do convidado"              $G -nE "waiting|espera" server/src/guests.rs
m B-09 "AUSÊNCIA axe"                     $G -rliwE "axe|axe-core|@axe-core" "${EXC[@]}" --include=*.ts --include=*.tsx --include=*.mjs --include=*.json web/src web/e2e web/package.json
m B-09 "AUSÊNCIA contraste"               $G -rlE "prefers-contrast|forced-colors" "${EXC[@]}" --include=*.css --include=*.ts --include=*.tsx web/src
m B-09 "controlo +"                       $G -rlE "prefers-reduced-motion|prefers-color-scheme" "${EXC[@]}" --include=*.css --include=*.ts --include=*.tsx web/src
m B-10 "MAX_RESULTS/totalResults"         $G -nE "const MAX_RESULTS|\"totalResults\": recursos" server/src/scim.rs
m B-10 "AUSÊNCIA startIndex"              $G -nE "start_index|startIndex.*Query|pub count" server/src/scim.rs
m B-10 "AUSÊNCIA /Groups"                 $G -rn "Groups" server/src/lib.rs
m B-11 "AUSÊNCIA electron/tauri"          $G -rliwE "electron|tauri" "${EXC[@]}" --include=package.json --include=Cargo.toml --include=*.ts .
m C-01 "AUSÊNCIA PBX"                     $G -rliE "call_queue|voicemail|message.waiting|\bmwi\b|call_park|ring_group|\bblf\b|barge|eavesdrop" "${EXC[@]}" --include=*.rs --include=*.sql server/src server/crates server/migrations
m C-01 "disable-transfer"                 $G -rn "disable-transfer" voice/freeswitch
m C-01 "controlo +"                       $G -rl "conference" voice/freeswitch
m C-02 "AUSÊNCIA softphone web"           $G -rliE "sip\.js|jssip|webphone|sipjs" "${EXC[@]}" --include=*.ts --include=*.tsx --include=package.json web
m C-03 "AUSÊNCIA PAI"                     $G -rliE "P-Asserted-Identity|sip_cid_type=pid" voice/freeswitch voice/kamailio
m C-03 "AUSÊNCIA timeouts"                $G -rlE "session-timeout|session-expires|rtp-timeout-sec|media_timeout" voice/freeswitch/sip_profiles
m C-03 "AUSÊNCIA tecto de gasto"          $G -rliE "spend_cap|spending_limit|max_spend|international_block|block_international" "${EXC[@]}" --include=*.rs --include=*.sql server
m C-04 "AUSÊNCIA h323"                    $G -rliwE "h323|h\.323|polycom|webex" "${EXC[@]}" --include=*.rs --include=*.ts --include=*.tsx server/src server/crates web/src
```

As contagens de OpenAPI: para cada um de `docs/reference/openapi/{v1,bff,operator,integrations}.json`, `json.loads(git show <rev>:<ficheiro>)`, e conta-se `len(paths)`, as chaves `get/post/put/patch/delete/head/options` de cada caminho e `len(components.schemas)`.

A releitura das âncoras: para cada `` `ficheiro.ext:linha` `` de cada `.claude/paridade/*.md`, compara-se `git show <base>:ficheiro` linha N com a árvore linha N. Se diferem, procura-se a linha antiga, por texto exacto, na árvore.
