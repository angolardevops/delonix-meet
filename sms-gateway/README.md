# delonix-sms-gateway

Agente do gateway de SMS do Delonix Meet ([ADR-0005](../docs/adr/0005-gateway-sms.md)).
Corre **na máquina onde o telefone ou o modem está ligado por USB** — o servidor do
Meet corre num pod sem acesso a USB e nunca vê o cabo.

O que faz, em ciclo:

1. lê `/sys/bus/usb/devices` (sem libusb nem libudev) e classifica cada dispositivo;
2. sonda os candidatos a modem — pelo `mmcli` se o ModemManager os gere, por comandos
   AT na porta série se não;
3. reporta o inventário ao servidor (`PUT /api/sms/agent/devices`, a cada 5 s ou ao
   ritmo que o servidor pedir);
4. pede mensagens (`POST /api/sms/agent/claim`, a cada 3 s), envia-as pelo dispositivo
   escolhido e reporta o resultado.

Liga-se **sempre para fora** (HTTPS): não abre portas e funciona atrás de NAT.

## A realidade dos telefones Android

A maior parte dos telefones Android **não expõe um modem AT por USB**. Ligados, mostram
só MTP (ficheiros) e, com a depuração USB activa, ADB. **O ADB não envia SMS sem root**,
e não há forma suportada de o fazer. O agente detecta estes casos e diz porquê:

| `kind` | O que é | Envia? |
|---|---|---|
| `modem` | tem porta série (`/dev/ttyUSB*`, `/dev/ttyACM*`) ou é gerido pelo ModemManager | decide a sonda |
| `android_adb` | telefone com depuração USB | não — explicado |
| `android_mtp` | telefone em modo de ficheiros | não — explicado |
| `mass_storage_modem` | pen Huawei/ZTE ainda em modo «CD virtual» | não — precisa de `usb_modeswitch` |
| `unknown` | outra coisa (teclado, webcam, …) | não |

Hubs e root hubs não aparecem.

**Hardware recomendado para a demonstração:** uma pen/modem GSM USB em modo modem —
por exemplo uma Huawei E173 ou E3531 (depois do `usb_modeswitch`, que na maioria das
distribuições corre sozinho) — ou uma placa SIM800 / SIM7600 com USB. Com um cartão SIM
activo e sem PIN (ou com o PIN já introduzido).

## Permissões

- **Transporte `at_serial`** (sem ModemManager): o utilizador que corre o agente tem de
  poder abrir a porta série — normalmente, estar no grupo `dialout`:
  `sudo usermod -aG dialout "$USER"` e voltar a entrar na sessão.
- **Transporte `modemmanager`**: o `mmcli` fala com o ModemManager por D-Bus; criar e
  enviar SMS pode precisar de autorização do polkit para utilizadores sem sessão local
  activa.
- Se o ModemManager está activo, **é ele que gere o modem** e o agente não lhe fala AT
  por cima (dois diálogos na mesma porta corrompem-se). Um dispositivo acabado de ligar
  fica 20 s «a aguardar o ModemManager» antes de o agente o sondar por AT.

Nada disto precisa de root.

## Diagnóstico: `--once`

Ligou um telefone ou uma pen e quer saber se serve? Não precisa de servidor nem de token:

```bash
cargo run --release -- --once
```

Imprime uma tabela legível (com a razão por baixo de cada dispositivo que não envia) e
a seguir o mesmo inventário em JSON — exactamente o que seria reportado ao servidor.

## Correr contra o servidor

1. Na consola do Delonix Meet, como administrador da organização, crie um gateway na área de
   SMS (`POST /api/orgs/{org_id}/sms/gateways`). O token (`dlxg_…`) **só aparece nesse momento** — guarde-o.
2. Arranque o agente na máquina do telefone:

   ```bash
   export DELONIX_SMS_SERVER=https://meet.exemplo.ao
   export DELONIX_SMS_TOKEN=dlxg_...        # nunca na linha de comandos partilhada
   cargo run --release
   ```

3. Na consola, o dispositivo aparece na lista de dispositivos da organização
   (`GET /api/orgs/{org_id}/sms/devices`). Seleccione-o como ponto
   de envio (só é possível se estiver `capable`).

O token nunca é escrito em log. `SIGINT`/`SIGTERM` param o agente; um envio em curso tem
até 70 s para terminar.

Variáveis e opções:

| Opção | Variável | Omissão |
|---|---|---|
| `--server` | `DELONIX_SMS_SERVER` | — |
| `--token` | `DELONIX_SMS_TOKEN` | — |
| `--sysfs-root` | — | `/sys/bus/usb/devices` |
| `--once` | — | desligado |
| — | `RUST_LOG` (filtro do `tracing`, ex.: `debug`) | `info` |

## Como se envia

- **`at_serial`**: o servidor entrega os PDUs já codificados (`server/src/sms_codec.rs`
  é o único sítio que codifica SMS). Para cada segmento: `AT+CMGF=0`,
  `AT+CMGS=<tpdu_len>`, espera pelo `> `, escreve o PDU e `Ctrl-Z`, e espera até 60 s
  pelo `+CMGS: <mr>`. O `provider_ref` são as referências, separadas por vírgula.
- **`modemmanager`**: `mmcli -m <n> --messaging-create-sms=number='…'` com o texto
  passado por ficheiro (`--messaging-create-sms-with-text`, escrito `0600` em
  `$XDG_RUNTIME_DIR` e apagado a seguir), depois `mmcli -s <n> --send`. Sem
  `$XDG_RUNTIME_DIR`, o texto vai em linha e **mensagens com plica (`'`) são recusadas**,
  porque o formato chave=valor do `mmcli` não tem escape documentado para ela. O
  ModemManager faz a sua própria codificação e segmentação; os PDUs do servidor não são
  usados neste transporte.

**No máximo uma vez:** o agente nunca repete um envio. Se falhar, reporta `ok:false` com
o erro; quem quiser repetir cria uma mensagem nova.

## O que não está coberto

- Recibos de entrega e SMS recebidos.
- Telefones Android sem porta AT (detectados e explicados, não enviados).
- Empacotamento (`.deb`, unit `systemd` de utilizador) — tarefa seguinte do ADR.
