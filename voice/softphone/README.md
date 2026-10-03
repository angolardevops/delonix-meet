# Prova de softphone

`scripts/softphone-prova.sh` guia um softphone de linha de comandos (baresip 1.0, o do
Debian 12, numa imagem local) e **mede** o resultado: chamada estabelecida com SDES-SRTP,
o PIN a chegar por DTMF, e o tom de cada lado ouvido pelo outro.

Para testar à mão, com ouvidos, usa-se um softphone com interface (por exemplo o
Linphone). Este script é para a prova repetível.

```bash
# 1. O próprio script, contra o FreeSWITCH da imagem, numa rede docker sem saída.
bash scripts/softphone-prova.sh selftest

# 2. O controlo negativo com a configuração REAL do repo (R226): os ficheiros que o
#    voice/docker-compose.voice.yml monta. Uma chamada sem SRTP ao perfil dos ramais
#    tem de levar 488.
bash scripts/softphone-prova.sh srtp-real

#    O mesmo contra a configuração que CORRE — a que o cluster local monta
#    (voice/cluster/freeswitch-entrypoint.sh): ramais e dial-in, com e sem SRTP.
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
| `srtp-real`: a global `rtp_secure_media`, com os ficheiros que o compose monta | `mandatory` |
| `srtp-real`: ramal com a password errada | recusado, `403 Forbidden` |
| `srtp-real`: ramal autenticado, **com** SRTP | passa a autenticação e a negociação; o plano de marcação fecha-a com `404` (ver abaixo) |
| `srtp-real`: a mesma chamada **sem** SRTP | recusada, `488 Not Acceptable Here`; no log, «Crypto not negotiated but required» |
| `srtp-real`: o `vars.xml.inc` incluído pelo `vars.xml` | o FreeSWITCH arranca, o perfil escuta em `:5070`, URL e segredo vêm do ambiente |
| `srtp-real` com o `voice/` de `origin/main` (`275ced1`) | **falha**: global vazia, sem SRTP `404` em vez de `488`, e o include mata o arranque (`unclosed <!--`) |
| `srtp-cluster`: ramal com a password errada | recusado, `403 Forbidden` |
| `srtp-cluster`: ramal autenticado, **com** SRTP | passa a negociação e chega ao `ramais_dial.lua`, que a fecha com `404` (o andaime não resolve números) |
| `srtp-cluster`: o mesmo ramal **sem** SRTP | recusado, `488 Not Acceptable Here` |
| `srtp-cluster`: dial-in (perfil `external`), **com** SRTP | atendido pelo IVR |
| `srtp-cluster`: dial-in **sem** SRTP | recusado, `488 Not Acceptable Here` |
| `srtp-cluster` sem nenhuma das duas globais (entrypoint e `internal.xml`) | **falha**: o ramal em claro leva `404` em vez de `488` |
| `srtp-cluster`: os pedidos ao servidor (R227) | nenhum leva o segredo no URL; o do `mod_xml_curl` leva-o em `Authorization: Basic`, os dos dois Lua em `X-Voice-Secret`, com o corpo JSON certo |
| `srtp-cluster`: o segredo de voz e o PIN marcado no `freeswitch.log` | 0 ocorrências |
| `srtp-cluster` com os Lua de antes, ou com o DEBUG ligado no log | **falha**: o segredo aparece 2 vezes e o PIN 1 |
| `srtp-cluster` com o `xml_curl.conf.xml` de antes | **falha**: segredo no URL e 5 vezes no log |

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
  do ficheiro) e por `voice/freeswitch/vars.xml.inc`. As linhas `rtp-secure-media` do
  perfil e de `conference.conf.xml` saíram.
- **O `vars.xml.inc` não é incluído por nada no repo**, e como estava não podia sê-lo: o
  cabeçalho trazia a directiva de include dentro de um comentário, o pré-processador
  executa-a lá, e o FreeSWITCH não arrancava; e lia o ambiente com `cmd="set"`, que não
  o lê. As duas coisas estão corrigidas, e o passo 5 do `srtp-real` mede-as.
- **Dois dígitos DTMF iguais seguidos só chegam os dois se houver «tecla solta» entre
  eles.** Sem isso, `4711` chegava como `471`.

## O andaime do `srtp-real`

A configuração vem das linhas de montagem do próprio compose, postas sobre a vanilla da
imagem do FreeSWITCH do repo. O que a prova acrescenta, e só isto: um ramal num directório
estático (quem responde pelo directório no Meet é o control plane, que aqui não corre); a
remoção dos perfis SIP de demonstração da vanilla (o `external` resolve o seu IP por STUN
e, numa rede sem saída, deita abaixo o mod_sofia inteiro); e o ESL em loopback.

## O andaime do `srtp-cluster`

A configuração é a que o `voice/cluster/freeswitch-entrypoint.sh` monta, com os ficheiros
que o `scripts/cluster-voice.sh` põe no ConfigMap `freeswitch-meet` e o ambiente do pod.
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
- **Uma chamada atendida no perfil dos ramais do repo.** O `srtp-real` prova a
  autenticação Digest e a negociação, e pára aí: o contexto `delonix_ramais` não existe
  para o FreeSWITCH tal como o compose o monta (`Context delonix_ramais not found`,
  `404`), a vanilla não carrega `mod_xml_curl` nem `mod_curl`, e nada inclui o
  `vars.xml.inc` — o perfil fica no porto 5060, e o script avisa-o com `!`.
- **A imagem do compose** (`safarov/freeswitch:latest`) não foi medida: a prova corre na
  imagem do repo.
- **O registo** (`REGISTER`): o script marca sem registar.
- **O dial-in** (Kamailio → contexto `public`), um re-INVITE em claro a meio de uma
  chamada cifrada, e a entrada de um tronco declarado `srtp=off` com a global a valer.
- **TLS na sinalização**: o baresip 1.0 não valida o certificado do servidor por omissão.
- **Um PBX à frente** (Issabel, FreePBX): não houve nenhum no caminho.
- A qualidade do áudio — só a presença do tom.
