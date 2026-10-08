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
