# Prova de softphone

`scripts/softphone-prova.sh` guia um softphone de linha de comandos (baresip 1.0, o do
Debian 12, numa imagem local) e **mede** o resultado: chamada estabelecida com SDES-SRTP,
o PIN a chegar por DTMF, e o tom de cada lado ouvido pelo outro.

Para testar à mão, com ouvidos, usa-se um softphone com interface (por exemplo o
Linphone). Este script é para a prova repetível: o `selftest`, o `srtp-real` e o
`srtp-cluster` correm no CI (workflow «Imagem FreeSWITCH») sempre que `voice/freeswitch/`,
`voice/cluster/`, o `compose.yaml`, o `cluster-voice.sh` ou o próprio script mudam.

```bash
# 1. O próprio script, contra o FreeSWITCH da imagem, numa rede docker sem saída.
bash scripts/softphone-prova.sh selftest

# 2. O controlo negativo com a configuração que CORRE (R226, R227): a que o arranque
#    (voice/cluster/freeswitch-entrypoint.sh) monta com os ficheiros que o compose.yaml
#    põe em /meet. Ramais e dial-in, com e sem SRTP: sem SRTP os dois levam 488. E o
#    segredo de voz e o PIN marcado não ficam no log.
bash scripts/softphone-prova.sh srtp-real

#    O mesmo, com a lista de ficheiros que o cluster local põe no ConfigMap.
bash scripts/softphone-prova.sh srtp-cluster

# 3. Um softphone contra o teu servidor: marca, envia o PIN, mede o tom que ouve.
SOFTPHONE_PASSWORD=… bash scripts/softphone-prova.sh chamada \
    --servidor 192.168.1.10:5070 --utilizador 1001 --destino 9000 --pin 123456 --espera-tom 440

# 4. Dois softphones na mesma sala: A toca 1000 Hz, B toca 440 Hz, e cada um tem de
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
| `srtp-real` e `srtp-cluster`: ramal com a password errada | recusado, `403 Forbidden` |
| `srtp-real` e `srtp-cluster`: ramal autenticado, **com** SRTP | passa a negociação e chega ao `ramais_dial.lua`, que a fecha com `404` (o andaime não resolve números) |
| `srtp-real` e `srtp-cluster`: o mesmo ramal **sem** SRTP | recusado, `488 Not Acceptable Here` |
| `srtp-real` e `srtp-cluster`: dial-in (perfil `external`), **com** SRTP | atendido pelo IVR |
| `srtp-real` e `srtp-cluster`: dial-in **sem** SRTP | recusado, `488 Not Acceptable Here` |
| `srtp-real` e `srtp-cluster` sem nenhuma das duas globais (entrypoint e `internal.xml`) | **falha**: o ramal em claro leva `404` em vez de `488` |
| `srtp-real` e `srtp-cluster`: os pedidos ao servidor (R227) | nenhum leva o segredo no URL; o do `mod_xml_curl` leva-o em `Authorization: Basic`, os dos dois Lua em `X-Voice-Secret`, com o corpo JSON certo |
| `srtp-real` e `srtp-cluster`: o segredo de voz e o PIN marcado no `freeswitch.log` | 0 ocorrências |
| `srtp-real` e `srtp-cluster`: ficheiros do directório de logs com o segredo de voz | nenhum (com o arranque anterior: o `freeswitch.xml.fsxml`) |
| `srtp-real` e `srtp-cluster` com os Lua de antes, ou com o DEBUG ligado no log | **falha**: o segredo aparece 2 vezes e o PIN 1 |
| `srtp-real` e `srtp-cluster` com o `xml_curl.conf.xml` de antes | **falha**: segredo no URL e 5 vezes no log |

## O que esta prova mediu (R226 no catálogo de regressões)

- **Em qualquer perfil, o que recusa uma chamada em claro à entrada é a variável GLOBAL
  `rtp_secure_media=mandatory`.** Medido com o `selftest` sem a global: sem nada, a chamada em claro é aceite;
  só com `rtp-secure-media` no perfil (que não existe no sofia: zero ocorrências em
  `sofia.c` na v1.11.3), aceite; só com `require-secure-rtp=true` (que o sofia lê para
  uma flag que mais nada consulta), aceite; com a global posta por uma directiva no
  próprio ficheiro do perfil, `488`. Um `set rtp_secure_media=mandatory` no dialplan
  antes do `answer` não a recusa num perfil que negoceia o SDP à chegada (o dos ramais);
  num perfil com `inbound-late-negotiation=true` (o `external` da vanilla) recusa — medido
  nos dois sentidos, com e sem o `set`.
- **No Meet, a global é posta por `voice/freeswitch/sip_profiles/internal.xml`** (no topo
  do ficheiro) e pelo arranque (`voice/cluster/freeswitch-entrypoint.sh`). As linhas `rtp-secure-media` do
  perfil e de `conference.conf.xml` saíram.
- **O compose de voz antigo nunca correu, e foi retirado** (2026-10-04). Montava ficheiros
  soltos sobre a vanilla e um `vars.xml.inc` que nada incluía: o perfil dos ramais ficava
  no porto 5060 e o contexto `delonix_ramais` nem existia. O `compose.yaml` e o cluster
  sobem o FreeSWITCH pelo mesmo arranque, e é essa configuração que a prova mede.
- **Dois dígitos DTMF iguais seguidos só chegam os dois se houver «tecla solta» entre
  eles.** Sem isso, `4711` chegava como `471`.

## O andaime do `srtp-real` e do `srtp-cluster`

A configuração é a que o `voice/cluster/freeswitch-entrypoint.sh` monta, com os ficheiros
que o `compose.yaml` (`srtp-real`) ou o ConfigMap `freeswitch-meet` do
`scripts/cluster-voice.sh` (`srtp-cluster`) põem em `/meet`, e o ambiente do contentor.
O que a prova acrescenta: os endereços do servidor em loopback; um servidor de andaime nos
dois portos (o público e o interno) que lê cada pedido inteiro, guarda-o, e responde a
tudo com o directório de um só ramal, como o `server/src/ramais.rs` (com o `a1-hash` do
Digest e o `auth-acl=delonix_ramais`); e a lista de acesso dos ramais em loopback, porque
numa rede sem saída o FreeSWITCH fica em `127.0.0.1`. O servidor de andaime **não valida**
o segredo: a prova lê os pedidos guardados. Que o servidor a sério o aceita assim é do
teste `security_voice_odoo`.

## O que NÃO está provado

- **Nada contra o Delonix Meet a correr**: nem um ramal real do control plane, nem o IVR
  do dial-in, nem a ponte para a sala. O `chamada` e o `par` foram exercitados contra um
  FreeSWITCH de teste sem autenticação.
- **O compose e o cluster a correr.** A prova arranca o entrypoint num contentor, com um
  servidor de andaime: um ramal real do control plane, o Kamailio à frente do dial-in e a
  rede de cada ambiente ficam de fora (`make compose-voice-check`, `make cluster`).
- **O registo** (`REGISTER`): o script marca sem registar.
- **O dial-in** (Kamailio → contexto `public`), um re-INVITE em claro a meio de uma
  chamada cifrada, e a entrada de um tronco declarado `srtp=off` com a global a valer.
- **TLS na sinalização**: o baresip 1.0 não valida o certificado do servidor por omissão.
- **Um PBX à frente** (Issabel, FreePBX): não houve nenhum no caminho.
- A qualidade do áudio — só a presença do tom.
