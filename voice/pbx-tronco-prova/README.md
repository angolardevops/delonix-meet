# Prova do tronco de uma central FreePBX até ao bordo do Meet

`scripts/pbx-tronco-prova.sh` ergue uma **réplica** do bordo de voz (este `compose.yaml`:
Kamailio, FreeSWITCH, servidor, Postgres e Redis, com projecto, rede e nomes próprios),
arranca a appliance FreePBX 17 real em QEMU com a configuração que o `kind: PbxService` do
`delonix-paas` gera (`spec.meet_trunk`, ADR 0063 I4), e mede a chamada nos dois lados.

Não toca no laboratório do `compose.yaml` da raiz, e pode correr ao lado dele.

```bash
bash scripts/pbx-tronco-prova.sh up          # réplica + sala de prova; imprime a CA do bordo

# No delonix-paas, a configuração da central, tal como o Kind a gera:
PBX_MEET_HOST=172.30.50.14 PBX_MEET_CA_FILE=<ca.pem> PBX_SEED_OUT=<ficheiro> \
  cargo test -p delonix-orchestrator --lib dump_the_generated_user_data_when_asked

bash scripts/pbx-tronco-prova.sh freepbx --seed <ficheiro>   # a central real, duas chamadas
bash scripts/pbx-tronco-prova.sh negativos   # sem SRTP → 488; fora da allowlist e sem conta → não entra
bash scripts/pbx-tronco-prova.sh longa       # 40 s: a chamada passa dos 32 s (R277)
bash scripts/pbx-tronco-prova.sh central     # a central autenticada, fora da allowlist (ADR-0016)
bash scripts/pbx-tronco-prova.sh browser     # um browser na sala e a central: áudio nos dois sentidos
bash scripts/pbx-tronco-prova.sh down
```

O modo `central` só precisa de docker: semeia duas organizações pela API e usa dois
softphones como central. Para a mesma prova com a appliance real, gera-se o seed com a
conta da organização A (os `A_*` de `.pbx-tronco-prova/central.env`) e corre-se com a
allowlist do bordo vazia:

```bash
PBX_MEET_HOST=172.30.50.14 PBX_MEET_CA_FILE=<ca.pem> PBX_SEED_OUT=<ficheiro> \
PBX_MEET_DOMAIN=$A_DOMINIO PBX_MEET_USER=$A_UTIL PBX_MEET_PASSWORD=$A_PASS \
  cargo test -p delonix-orchestrator --lib dump_the_generated_user_data_when_asked
bash scripts/pbx-tronco-prova.sh freepbx --seed <ficheiro> --central
```

`SERVER_IMAGE=<imagem>` corre a réplica com o servidor de outra árvore, sem tocar na
`delonix-server:latest`.

Precisa de docker, de QEMU com KVM, das ferramentas libguestfs, e da imagem da appliance
(`FREEPBX_IMAGE`, por omissão a do store do `delonix`). `PBX_PROVA_TIMEOUT` alarga o
limite da VM (25 min por omissão; com o host carregado, 15 não chegaram).

## O que está medido (2026-10-04)

A réplica: Kamailio 5.8.6, o FreeSWITCH 1.11.3 da imagem do repo com o arranque do
`voice/cluster/`, e o servidor da `develop`. A central: FreePBX 17.0.33 com Asterisk 22.11.0.

| Prova | Resultado |
|---|---|
| O guião do Kind na central | `exit=0`, com o tronco carregado |
| Tronco central → bordo | `Avail` na 5061, transporte `meet-tls`, certificado do bordo verificado |
| Ligação TLS vista do bordo | aberta enquanto a central correu (`kamcmd tls.info`) |
| Transacções no bordo para duas chamadas | 4 (um `INVITE` e um `BYE` por chamada) |
| IVR do Meet | correu nas duas chamadas |
| SRTP | activo nas duas, visto do FreeSWITCH e da central |
| PIN certo (`123456`) por DTMF | aceite — entrou na conferência da sala |
| PIN errado (`999999`) | recusado — o IVR voltou a falar |
| Voz do IVR ouvida pela central | 7 de 10 janelas de 0,5 s com voz nos primeiros 5 s |
| Fim das chamadas | as duas `NORMAL_CLEARING` |
| Oferta sem SRTP, da origem permitida | `488` |
| Origem fora da allowlist, sem conta SIP | não entra: `407` (ou `403`, com as centrais desligadas — era o que se media antes do ADR-0016) |
| Chamada de 40 s | `Record-Route` com o endereço real, `ACK` enviado, sem corte |

## A central autenticada (ADR-0016, 2026-10-04)

De endereços **fora da allowlist**, com a conta SIP da organização («Registo SIP»):

| Prova | Resultado |
|---|---|
| Dois telefones como central de A, PIN de uma sala de A | entram **pela ponte do SFU**, ouvem-se um ao outro (0,2499 / 0,2500) e nenhum se ouve a si |
| UDP, com a conta certa | `403` |
| TLS, sem credenciais | `407`, não entra |
| Password errada · a conta de B no domínio de A | não entram; falha contada no bordo |
| A central de A com o PIN de uma sala de B | o IVR não a deixa entrar; o mesmo PIN, pela central de B, abre a sala |
| `X-Delonix-Central` forjado, pelo bordo | chega ao FreeSWITCH sem o cabeçalho |
| `X-Delonix-Central` forjado, direito ao FreeSWITCH | o IVR rejeita (`603`) |
| Dez falhas de uma origem | `403` mesmo com a password certa; outra origem entra |
| A FreePBX real, com a allowlist vazia | as duas chamadas autenticadas; a do PIN certo na sala da organização, pela ponte |

## Um browser na sala (2026-10-05)

O modo `browser` põe um Chromium na sala com a **pilha real do cliente** (`web/e2e/harness.html`:
`SfuCall` e `Signaling`), a tocar 440 Hz pelo microfone falso, e faz entrar a central — um
softphone autenticado como no modo `central` — a tocar 1000 Hz. Cada lado mede o tom do outro
(`web/e2e/telefone-na-sala.mjs` do lado do browser, o `softphone-prova.sh` do lado da central).
Precisa de `npm ci` em `web/`.

| Prova | Resultado (cinco corridas seguidas) |
|---|---|
| A central entra na sala pela ponte do SFU | sim, sem recuo para a conferência local |
| Browser → central: a central ouve os 440 Hz | amplitude 0,503, e não ouve o seu próprio tom (0,001) |
| Central → browser: o browser recebe o áudio dela | ≈ 602 pacotes em 12 s, nenhum perdido |
| O que o browser descodifica é o tom da central | pico do espectro nos 1002 Hz, a −26 dB, em 12 de 12 leituras |
| E não o seu próprio tom | os 1000 Hz ficam 82 dB ou mais acima dos 440 Hz |
| Controlo negativo (`PBX_PROVA_TOM_CENTRAL=700`) | o browser deixa de encontrar os 1000 Hz e a prova falha; o outro sentido continua |

**Não medido:** a qualidade do áudio (só a presença do tom); mais do que um browser; vídeo;
a mesma coisa no `compose.yaml` ou no cluster, e com um Chrome a sério em vez do Chromium
de testes.

## O que esta prova encontrou

1. **O bordo anunciava-se como `0.0.0.0`** e todas as chamadas por ele caíam aos 32 s —
   R277, corrigido em `voice/kamailio/kamailio.cfg`.
2. **Uma prova curta não prova que a chamada se aguenta.** As chamadas de prova dos
   laboratórios duram 4 s; o defeito só aparece depois dos 32.
3. **Os contadores valem mais do que as linhas de log.** O `xlog` do bordo não sai no
   nível com que o Kamailio corre, e o `Secure Type` do FreeSWITCH é DEBUG, que a R227
   desligou. Mede-se por `kamcmd tm.stats`/`tls.info` e por `Activating audio Secure RTP`.
4. **Atrás de NAT, o FreeSWITCH só sabe para onde mandar a voz depois do primeiro pacote
   de quem liga.** Uma central calada nos primeiros segundos não ouve o IVR — numa chamada
   real o telefone fala desde o início.
6. **Medir o que um browser ouve tem duas armadilhas.** O `audioLevel` e o
   `totalAudioEnergy` do `inbound-rtp` vêm a **zero** neste Chromium, sem saída de áudio
   real, com o tom lá — mede-se nas amostras (Web Audio). E uma fonte de Web Audio feita do
   fluxo inteiro só lê **uma** das faixas de áudio: o cliente tem sempre uma faixa remota
   muda, e quando calhava ser essa lia-se silêncio com os pacotes a chegar (uma corrida em
   três). Uma fonte por faixa resolve.
7. **O compose desta réplica deixou de se ler** quando duas entregas lhe acrescentaram a
   mesma chave (`DATA_ENCRYPTION_KEYS`): o git fundiu sem conflito e o `docker compose`
   passou a recusar o ficheiro. Ninguém soube, porque nada o lia. Há agora um portão
   (`scripts/check-replicas-compose.sh`).
5. **O Kamailio corre sem `L_INFO` nem `L_NOTICE`.** Uma central que entrava autenticada
   não deixava rasto no bordo; passou a contar-se (`kamcmd cnt.get script
   centrais_autenticadas` e `centrais_recusadas`).

## O que NÃO está provado

- **Um browser na sala, fora desta réplica.** Aqui mediu-se (modo `browser`); no
  `compose.yaml`, no cluster e com um Chrome a sério, não.
- **Uma central pela allowlist continua sem organização.** Quem entra por IP é um dial-in
  por `(número, PIN)`; só a conta SIP diz de que organização é a central (ADR-0016).
- **Chamadas do Meet para a central.** O contexto `from-meet` desliga tudo, de propósito.
- **Um bordo atrás de NAT** (`DELONIX_SIP_ADVERTISE`): só validado com `kamailio -c`.
- **O laboratório do `compose.yaml` e o cluster local com a correcção da R277**: correm o
  mesmo `kamailio.cfg`, mas não foram reiniciados e medidos nesta entrega.
- Um Issabel, e uma operadora.
