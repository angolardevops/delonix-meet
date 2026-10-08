# Ambiente de teste local do DelonixPhone (sem aparelho)

Medido em 2026-10-08 nesta máquina (Linux, KVM, 32 núcleos): Android Studio, SDK 36, emulador,
duas imagens e Flutter 3.47.6. Instalado em `~/Android` e `~/development`, sem `sudo`.

```bash
. mobile/ambiente/env.sh            # JAVA_HOME, ANDROID_HOME, PATH (Flutter, adb, emulador)
mobile/ambiente/emulador.sh pixel   # Android 15 com Play (FCM) · `lowend` = API 26, 2 GB
mobile/ambiente/gsm-convivencia.sh  # RF-25: chamada GSM emulada a tocar, atender, terminar
mobile/ambiente/cgnat.sh 6 0        # CGNAT com UDP a 6 s e SEM keep-alive  -> INVITE perdido
mobile/ambiente/cgnat.sh 6 3        # o mesmo com keep-alive de 3 s          -> INVITE chega
mobile/ambiente/emulador.sh pixel stop
```

O que cada coisa prova, e o que **não** prova:

| Peça | Prova | Não prova |
|---|---|---|
| `gsm-convivencia.sh` | o emulador gera tocar/atender/terminar de uma chamada celular (`adb emu gsm …`) | que a app SIP ponha a chamada em espera: falta a app |
| `cgnat.sh` | um NAT com temporizador UDP curto mata o mapeamento de um cliente calado, e o keep-alive salva-o; a origem é reescrita (`198.51.100.1`) | o comportamento de uma operadora real; os temporizadores reais medem-se em Angola (RNF-45) |
| emulador | UI, permissões, Telecom, rede lenta (`adb emu network delay/speed`) | PushKit/APNs (iPhone), áudio real de rede móvel, bateria, Doze de fabricantes |

O `cgnat.sh` corre num user namespace: não mexe na rede do host nem precisa de root.
O iPhone não corre em Linux: simulador só em macOS (runner na nuvem), e mesmo aí sem PushKit.
