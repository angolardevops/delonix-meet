# Prova de softphone

`scripts/softphone-prova.sh` guia um softphone de linha de comandos (baresip 1.0, o do
Debian 12, numa imagem local) e **mede** o resultado: chamada estabelecida com SDES-SRTP,
o PIN a chegar por DTMF, e o tom de cada lado ouvido pelo outro.

Para testar à mão, com ouvidos, usa-se um softphone com interface (por exemplo o
Linphone). Este script é para a prova repetível.

```bash
# 1. O próprio script, contra o FreeSWITCH da imagem, numa rede docker sem saída.
bash scripts/softphone-prova.sh selftest

# 2. Um softphone contra o teu servidor: marca, envia o PIN, mede o tom que ouve.
SOFTPHONE_PASSWORD=… bash scripts/softphone-prova.sh chamada \
    --servidor 192.168.1.10:5070 --utilizador 1001 --destino 9000 --pin 123456 --espera-tom 440

# 3. Dois softphones na mesma sala: A toca 1000 Hz, B toca 440 Hz, e cada um tem de
#    ouvir o do outro e não o seu. Dois sentidos, sem browser.
SOFTPHONE_PASSWORD_A=… SOFTPHONE_PASSWORD_B=… bash scripts/softphone-prova.sh par \
    --servidor 192.168.1.10:5070 --utilizador-a 1001 --utilizador-b 1002 --destino 9000 --pin 123456
```

As passwords vêm do ambiente, nunca da linha de comandos, e não ficam em disco no host.
`--transporte udp|tcp|tls`, `--segundos` e `--espera-pin` afinam a chamada; a gravação
do que o softphone ouviu fica em `.softphone-prova/ultima-chamada-ouvido.wav`.

## O que está medido (2026-10-03, FreeSWITCH 1.11.3 da imagem do repo)

| Prova | Resultado |
|---|---|
| `selftest`: PIN `4711` por RFC 2833 | o FreeSWITCH recebeu `4711` |
| `selftest`: tom do FreeSWITCH (440 Hz) ouvido pelo softphone | amplitude 0,31 |
| `selftest`: tom do softphone (1000 Hz) gravado pelo FreeSWITCH | amplitude 0,25 |
| `selftest`: o softphone não ouve o próprio tom | 0,0002 |
| `selftest`: chamada **sem** SRTP | recusada, `488 Not Acceptable Here` |
| `selftest`: par na mesma conferência | cada um ouve o outro a 0,25 e a si a ≤ 0,003 |
| `chamada` e `par` em rede *host* contra um FreeSWITCH de teste | passam; com os dois softphones sem se ouvirem, o `par` falha |

## Dois factos que esta prova mediu

- **Só a variável GLOBAL `rtp_secure_media=mandatory` recusa uma chamada em claro à
  entrada.** O parâmetro de perfil `rtp-secure-media` não existe no sofia (zero
  ocorrências em `sofia.c` na v1.11.3): só com ele, a chamada em claro foi aceite. Um
  `set rtp_secure_media=mandatory` no dialplan antes do `answer` também não a recusa.
  No Meet, a global está em `voice/freeswitch/vars.xml.inc`; as linhas
  `rtp-secure-media` de `sip_profiles/internal.xml` e `autoload_configs/conference.conf.xml`
  não fazem o que o comentário ao lado diz.
- **Dois dígitos DTMF iguais seguidos só chegam os dois se houver «tecla solta» entre
  eles.** Sem isso, `4711` chegava como `471`.

## O que NÃO está provado

- **Nada contra o Delonix Meet a correr**: nem um ramal real em `internal.xml`, nem o
  IVR do dial-in, nem a ponte para a sala. O `chamada` e o `par` foram exercitados contra
  um FreeSWITCH de teste sem autenticação.
- **A autenticação Digest** (`auth_pass`) e o **registo**: o script marca sem registar.
- **TLS na sinalização**: o baresip 1.0 não valida o certificado do servidor por omissão.
- **Um PBX à frente** (Issabel, FreePBX): não houve nenhum no caminho.
- A qualidade do áudio — só a presença do tom.
