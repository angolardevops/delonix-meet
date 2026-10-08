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
cd <worktree do laboratório> && make compose-up LAN_IP=<ip desta máquina>
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
