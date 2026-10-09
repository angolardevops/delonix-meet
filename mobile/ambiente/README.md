# Ambiente de teste local do DelonixPhone (sem aparelho)

Medido em 2026-10-08 nesta máquina (Linux, KVM, 32 núcleos): Android Studio, SDK 36, emulador,
duas imagens e Flutter 3.47.6. Instalado em `~/Android` e `~/development`, sem `sudo`.

```bash
. mobile/ambiente/env.sh            # JAVA_HOME, ANDROID_HOME, PATH (Flutter, adb, emulador)
mobile/ambiente/emulador.sh pixel   # Android 15 com Play (FCM) · `lowend` = API 26, 2 GB
mobile/ambiente/gsm-convivencia.sh  # RF-25: chamada GSM emulada a tocar, atender, terminar
mobile/ambiente/cgnat.sh 6 0        # CGNAT com UDP a 6 s e SEM keep-alive  -> INVITE perdido
mobile/ambiente/cgnat.sh 6 3        # o mesmo com keep-alive de 3 s          -> INVITE chega
mobile/ambiente/patrol.sh           # teste Patrol da app (RF-25, 1.ª metade) com o gatilho de GSM
mobile/ambiente/patrol.sh integration_test/ligar_ao_laboratorio_test.dart   # provisiona e regista no laboratório do Meet
mobile/ambiente/emulador.sh pixel stop
```

O que cada coisa prova, e o que **não** prova:

| Peça | Prova | Não prova |
|---|---|---|
| `gsm-convivencia.sh` | o emulador gera tocar/atender/terminar de uma chamada celular (`adb emu gsm …`) | que a app SIP ponha a chamada em espera: falta a app |
| `cgnat.sh` | um NAT com temporizador UDP curto mata o mapeamento de um cliente calado, e o keep-alive salva-o; a origem é reescrita (`198.51.100.1`) | o comportamento de uma operadora real; os temporizadores reais medem-se em Angola (RNF-45) |
| `patrol.sh` | a app **real** vê uma chamada celular emulada: repouso → a tocar → em curso → repouso, com o diálogo de permissão do sistema tratado pelo Patrol | que a chamada SIP fique em espera (não há motor SIP); iPhone |
| emulador | UI, permissões, Telecom, rede lenta (`adb emu network delay/speed`) | PushKit/APNs (iPhone), áudio real de rede móvel, bateria, Doze de fabricantes |

O `cgnat.sh` corre num user namespace: não mexe na rede do host nem precisa de root.
O iPhone não corre em Linux: simulador só em macOS (runner na nuvem), e mesmo aí sem PushKit.

O teste Patrol corre dentro do aparelho e não gera chamadas celulares; `gatilho-gsm.py` (127.0.0.1:8765, três rotas, número fixo) traduz pedidos HTTP do emulador (`10.0.2.2`) em `adb emu gsm`. O tráfego em claro só está permitido no manifesto de *debug*.

## Ligar ao laboratório do Meet (provisionamento como o Linphone)

O Meet já emite um QR de uso único por ramal (R278): é um URL `https://…/api/public/extension-provisioning/<64 hex>`
que devolve a configuração `lpconfig` do Linphone. A app aceita o mesmo: **ler o QR**, **colar o endereço** ou
**parâmetros manuais**. Ler o QR gasta o bilhete e **troca a palavra-passe SIP do ramal**.

Pré-requisito: laboratório em modo LAN (o QR tem de ser `https` e o IP alcançável do emulador; um `.local` é recusado pelo servidor):

```bash
cd .worktrees/delonix-meet/laboratorio-develop && make compose-up LAN_IP=<ip desta máquina>
# (outro sítio: export DP_LAB_DIR=<worktree do laboratório>)
```

O `patrol.sh` arranca o `gatilho-gsm.py`, que também emite bilhetes (`/lab/bilhete`, `/lab/credenciais`, `/lab/bilhete-usado`)
com a conta de administração do laboratório, que só o anfitrião tem, e cria o ramal da empresa **1900** («DelonixPhone (emulador)»)
para não mexer nos ramais de pessoas. A raiz de confiança do laboratório entra por `--dart-define=LAB_CA_B64` só em debug.

| Teste | Prova |
|---|---|
| colar o endereço do QR | bilhete real → XML real → conta → REGISTER com digest → **registado no FreeSWITCH** |
| QR usado duas vezes | o servidor responde 404 e a app diz que já foi usado |
| URL `http` | recusado sem tocar na rede |
| parâmetros manuais | as mesmas credenciais, escritas à mão, registam |
| palavra-passe errada | 403 do FreeSWITCH; a mensagem não repete a palavra-passe |
| negar a câmara | o ecrã de recurso aparece |

**Registo por TLS.** Com o perfil TLS dos ramais (branch `delonix-meet-telefonia/ramais-tls`: `DELONIX_RAMAIS_TLS_PORT`,
passo 9b do entrypoint do FreeSWITCH) o QR do laboratório já manda registar por **TLS na 5071**, com o certificado
da borda (cobre o IP), e o `RegistoSip` confere-o contra a raiz de laboratório (`LAB_CA_B64`, só debug). Sem esse
perfil o QR continua em UDP/5070 e a app mostra o aviso «sem cifra». O FreeSWITCH lista a conta como `Registered(TLS)`.

**Não provado:** a leitura de um QR com a câmara. A câmara virtual do emulador (cena 3D) saiu em branco no emulador sem janela;
fica por medir num aparelho real ou com a janela do emulador. TCP em claro não existe na app, e o UDP só em debug.
O registo de diagnóstico (`RegistoSip`) não é o motor de chamadas: sem SRTP, sem chamadas, sem CGNAT.

## A app acorda por push de laboratório (ADR-0023)

`prova-acordar.py` prova a cadeia completa com a **app morta** no emulador. O servidor, o FreeSWITCH e a app são
os reais; só o fornecedor de push é «de papel» (o receptor desta máquina abre a app por *intent*, como o FCM
faria).

```bash
# 1. laboratório com o push ligado (a imagem do servidor tem de levar o S-01, PR #279)
cd .worktrees/delonix-meet/laboratorio-develop && make compose-up LAN_IP=<ip> PUSH_LAB_URL=http://<ip>:18890/push
# 2. emulador a correr e o APK de debug instalado (a raiz de laboratório entra por --dart-define)
. mobile/ambiente/env.sh
(cd mobile/delonixphone && flutter build apk --debug --dart-define=LAB_CA_B64=$(base64 -w0 <lab>/deploy/compose/generated/lan-tls/ca.crt))
adb install -r -g mobile/delonixphone/build/app/outputs/flutter-apk/app-debug.apk
# 3. a prova (cria uma pessoa de prova com ramal no laboratório e deixa-a lá)
python3 -I mobile/ambiente/prova-acordar.py --lab <worktree do laboratório>
```

O que mede (8 em 8 em 2026-10-08): a app configura-se por *intent* (provisiona pelo QR, entra no Meet, regista o
aparelho `lab`, arranca o motor e regista-se por TLS); fica morta e o ramal sem registo; uma chamada de um
telefone SIP de papel (TLS, SRTP, PCMU) é segurada pelo FreeSWITCH; o servidor pede o push; a app abre, regista-se e
mostra «Chamada a entrar»; o chamador ouve `180`; atende-se **no ecrã** e o chamador recebe `200 OK`; e, com o
aparelho revogado, o chamador recebe `480` em 0,1 s e a app continua morta.

**O que não prova:** FCM/APNs reais, um telemóvel físico, a app morta pelo sistema (usa-se `am force-stop`),
áudio (o emulador corre sem som), nem o iPhone. Os *intents* `dlx_*` são uma comodidade de laboratório: a
configuração (`dlx_configurar_url`, `dlx_email`, `dlx_senha`) **só corre em debug**, e num release é ignorada.

**Um engano que esta prova desfez:** a primeira versão fazia a chamada por `originate … loopback/…`, cuja perna só
fala `L16/8000`; o FreeSWITCH oferecia à app só `L16` e a app recusava com 488 (`INCOMPATIBLE_DESTINATION`).
Era um artefacto da prova, não do Lua: com um chamador a sério (PCMU) a oferta é normal. Fica o aviso de que
**o codec da perna chamadora condiciona a oferta à app**: uma chamada de um telefone só com G.729 falharia da
mesma forma (por medir).


## A cadeia completa com o delonix-push real

`prova-acordar-delonix.sh` sobe um delonix-push (repo `angolardevops/delonix-push`), liga o laboratório do Meet a ele
(`PUSH_DELONIX_URL` e `PUSH_DELONIX_KEY` no `compose-lan.sh`), constrói o APK e corre `prova-acordar-delonix.py`:
a app configura-se, o Meet cunha o aparelho no delonix-push, o processo da app é morto (o serviço renasce), uma chamada
SIP de papel (TLS, SRTP, PCMU) entra com o **ecrã apagado**, o FreeSWITCH segura-a e pede o *wake*, o delonix-push entrega
à app, a notificação de ecrã inteiro abre a Activity, a app regista-se por SIP e toca, atende-se («200 OK»), e por fim
desligar o aparelho no Meet revoga-o também no delonix-push. Medido a 2026-10-09 no emulador: 11 verificações.

Armadilhas que a prova apanhou: o `uiautomator dump` falha com o ecrã desligado e lê-se um `u.xml` antigo (apaga-se antes);
o `pm clear` repõe as permissões (`POST_NOTIFICATIONS` e `USE_FULL_SCREEN_INTENT` concedem-se depois); o ecrã inteiro só abre a
Activity com o ecrã apagado. **Não prova** FCM/APNs, um telemóvel físico, nem áudio.
