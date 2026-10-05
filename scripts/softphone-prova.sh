#!/usr/bin/env bash
# ============================================================
#  Prova de softphone: um UA SIP de linha de comandos (baresip) que marca,
#  envia o PIN por DTMF, toca um tom e grava o que ouve — e MEDE os tons, em
#  vez de alguém dizer «ouvi».
#
#  Serve para provar uma interligação antes de se dizer «a funcionar»
#  (.claude/skills/delonix-meet-voip): chamada estabelecida com SDES-SRTP, o
#  DTMF a chegar ao IVR, e áudio NOS DOIS SENTIDOS.
#
#  Modos:
#    selftest   prova o PRÓPRIO script contra o FreeSWITCH da imagem, numa
#               rede docker interna (sem saída): SRTP negociado, o PIN a
#               chegar, os tons medidos nos dois sentidos, o par de softphones
#               numa conferência, e o controlo negativo (sem SRTP → recusada).
#               Não toca no Meet nem em nada da tua rede.
#    srtp-real  o controlo negativo com a configuração que CORRE (R226): a
#               que o voice/cluster/freeswitch-entrypoint.sh monta com os
#               ficheiros que o compose.yaml põe em /meet, numa rede docker sem
#               saída. Mede o perfil dos ramais (um ramal autentica-se, com um
#               servidor de directório de andaime a responder) e o do dial-in:
#               sem SRTP, os dois têm de dar 488. E o segredo de voz: chega ao
#               servidor em cabeçalhos, nunca no URL, e não fica — nem ele nem
#               o PIN marcado — no log (R227).
#    srtp-cluster  o mesmo, com os ficheiros que o scripts/cluster-voice.sh
#               põe no ConfigMap do cluster local.
#    chamada    um softphone contra um servidor teu (ramal no FreeSWITCH, ou
#               um ramal do PBX): marca --destino, envia --pin, toca --tom e
#               mede --espera-tom no que ouviu.
#    medir F HZ a amplitude do tom de HZ no ficheiro F (WAV), e mais nada: para
#               outra prova medir uma gravação sua.
#    par        dois softphones (contas A e B) no MESMO destino — a mesma sala.
#               A toca 1000 Hz e B 440 Hz; cada um tem de ouvir o tom do outro
#               e NÃO o próprio (mix-minus). É a prova de dois sentidos sem
#               precisar de um browser na sala.
#
#               Nos dois: --dominio D põe D no endereço da conta (o `From`),
#               para um servidor que decide por domínio — o bordo desafia a
#               central com o realm do `From` (ADR-0016); --rede R corre os
#               softphones numa rede docker em vez de na do anfitrião.
#
#  As passwords vêm do AMBIENTE, nunca da linha de comandos:
#    SOFTPHONE_PASSWORD (chamada) · SOFTPHONE_PASSWORD_A / _B (par)
#
#  Exemplos:
#    bash scripts/softphone-prova.sh selftest
#    bash scripts/softphone-prova.sh srtp-real
#    bash scripts/softphone-prova.sh srtp-cluster
#    SOFTPHONE_PASSWORD=… bash scripts/softphone-prova.sh chamada \
#        --servidor 192.168.1.10:5070 --utilizador 1001 --destino 9000 --pin 123456
#    SOFTPHONE_PASSWORD_A=… SOFTPHONE_PASSWORD_B=… bash scripts/softphone-prova.sh par \
#        --servidor 192.168.1.10:5070 --utilizador-a 1001 --utilizador-b 1002 \
#        --destino 9000 --pin 123456
#
#  O que NÃO prova: a qualidade do áudio (só a presença do tom), o TLS da
#  sinalização (o baresip 1.0 do Debian não valida o certificado do servidor
#  por omissão), nem nada do lado do browser.
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

IMG_BS=${SOFTPHONE_IMAGE:-delonix-meet/baresip:1.0.0}
IMG_FS=${FS_IMAGE:-delonix-meet/freeswitch:1.11.3}
WORK=$PWD/.softphone-prova
TAG=sp$$                       # prefixo dos contentores desta corrida
PRESENTE=${SOFTPHONE_LIMIAR_PRESENTE:-0.03}   # amplitude a partir da qual um tom «está lá»
AUSENTE=${SOFTPHONE_LIMIAR_AUSENTE:-0.01}     # e abaixo da qual «não está»
SELFTEST_SUBNET=${SOFTPHONE_SELFTEST_SUBNET:-172.31.250.0/29}
PORTO_RAMAIS=5070              # o DELONIX_RAMAIS_SIP_PORT do compose.yaml e do cluster
DESTINO_REAL=101               # um número curto: o que o contexto dos ramais aceita
DESTINO_DIALIN=244923000000    # um número qualquer: o contexto `public` aceita 6 a 15 dígitos
PIN_DIALIN=482913              # um PIN qualquer, de 6 dígitos: só interessa vê-lo chegar ao servidor
fail=0
ok()  { printf '  ✓ %s\n' "$*"; }
bad() { printf '  ✗ %s\n' "$*"; fail=1; }
aviso() { printf '  ! %s\n' "$*"; }   # medido e fora do que esta prova julga

uso() { sed -n '2,56p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-2}"; }

limpar() {
  local c
  for c in $(docker ps -aq --filter "name=^${TAG}-" 2>/dev/null); do docker rm -f "$c" >/dev/null 2>&1; done
  docker network rm "${TAG}-net" >/dev/null 2>&1 || true
  rm -rf "$WORK/$TAG"
  rmdir "$WORK" 2>/dev/null || true
}
trap limpar EXIT

imagem_baresip() {
  docker image inspect "$IMG_BS" >/dev/null 2>&1 && return 0
  echo "▶ a construir $IMG_BS (voice/softphone/Containerfile)"
  docker build -q -t "$IMG_BS" -f voice/softphone/Containerfile voice/softphone >/dev/null \
    || { echo "✗ não consegui construir $IMG_BS"; exit 1; }
}

# tom <ficheiro.wav> <Hz> <segundos> — 8 kHz mono, a amplitude 0,25 (o G.711 é a 8 kHz;
# o gerador de tom do baresip 1.0 só trabalha a 48 kHz, daí o ficheiro).
tom() {
  python3 - "$1" "$2" "$3" <<'PY'
import sys, wave, math, struct
p, f, secs = sys.argv[1], float(sys.argv[2]), int(sys.argv[3]); sr = 8000
w = wave.open(p, 'wb'); w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
w.writeframes(b''.join(struct.pack('<h', int(0.25 * 32767 * math.sin(2 * math.pi * f * n / sr)))
                       for n in range(sr * secs)))
w.close()
PY
}

# medir <ficheiro.wav> <Hz> — a MAIOR amplitude do tom numa janela de 0,5 s, por canal
# (Goertzel, normalizada a 1,0 = fundo de escala). Uma janela e não a chamada inteira:
# a chamada tem silêncio antes e depois do tom. Devolve o maior dos canais.
medir() {
  python3 - "$1" "$2" <<'PY'
import sys, wave, math, struct
p, f = sys.argv[1], float(sys.argv[2])
try:
    w = wave.open(p, 'rb')
except Exception as e:
    print("0.0000"); sys.exit(0)
ch, sr, n = w.getnchannels(), w.getframerate(), w.getnframes()
raw = w.readframes(n); w.close()
s = struct.unpack('<%dh' % (len(raw) // 2), raw[: len(raw) // 2 * 2])
win = sr // 2; best = 0.0
for c in range(ch):
    x = s[c::ch]
    for i in range(0, max(len(x) - win, 0) + 1, win // 2):
        blk = x[i:i + win]
        if len(blk) < win: break
        k = 2 * math.cos(2 * math.pi * f / sr); a = b = 0.0
        for v in blk:
            a, b = v + k * a - b, a
        amp = math.sqrt(max(a * a + b * b - k * a * b, 0.0)) * 2 / win / 32768
        best = max(best, amp)
print("%.4f" % best)
PY
}
maior() { python3 -c "import sys; sys.exit(0 if float(sys.argv[1]) >= float(sys.argv[2]) else 1)" "$1" "$2"; }
menor() { python3 -c "import sys; sys.exit(0 if float(sys.argv[1]) <= float(sys.argv[2]) else 1)" "$1" "$2"; }

logs() { docker logs "$1" 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | tr -d '\r'; }
# esperar <contentor> <padrão> <segundos> — 0 se apareceu
esperar() {
  local i
  for i in $(seq 1 $(( $3 * 2 ))); do
    logs "$1" | grep -Eq "$2" && return 0
    sleep 0.5
  done
  return 1
}

# perna <nome> <rede docker> <servidor host:porto> <utilizador> <var da password|-> <destino>
#       <tom Hz> <transporte> <mediaenc> <porto SIP local> <porto da consola> <base RTP> <ip de escuta> <segundos máx>
perna() {
  local nome=$1 rede=$2 srv=$3 user=$4 pwvar=$5 dest=$6 hz=$7 transp=$8 menc=$9
  local sipp=${10} consp=${11} rtpb=${12} lip=${13} tmax=${14}
  local d="$WORK/$TAG/$nome" host=${srv%%:*} acc
  rm -rf "$d"; mkdir -p "$d/conf"; chmod 700 "$WORK/$TAG"
  tom "$d/conf/tom.wav" "$hz" 60
  cat > "$d/conf/config" <<EOF
poll_method		epoll
sip_listen		$lip:$sipp
audio_player		aubridge,nulo
audio_source		aufile,/conf/tom.wav
audio_alert		aubridge,nulo
ausrc_srate		8000
auplay_srate		8000
ausrc_channels		1
auplay_channels		1
rtp_ports		$rtpb-$(( rtpb + 100 ))
module_path		/usr/lib/baresip/modules
module			cons.so
module			g711.so
module			aubridge.so
module			aufile.so
module			sndfile.so
module			srtp.so
module_tmp		account.so
module_app		menu.so
cons_listen		127.0.0.1:$consp
snd_path		/rec
EOF
  # O endereço da conta é o `From`. Com --dominio é esse domínio, e o servidor
  # só entra no URI que se marca.
  acc="<sip:$user@${DOMINIO:-$srv};transport=$transp>;regint=0;audio_codecs=PCMA/8000/1,PCMU/8000/1"
  [ "$menc" != nenhum ] && acc="$acc;mediaenc=$menc"
  if [ "$pwvar" != - ]; then
    [ -n "${!pwvar:-}" ] || { echo "✗ falta a password no ambiente: $pwvar"; exit 2; }
    acc="$acc;auth_user=$user;auth_pass=${!pwvar}"
  fi
  ( umask 077; printf '%s\n' "$acc" > "$d/conf/accounts" )
  # O transporte não vai no URI marcado: o baresip acrescenta o da conta.
  docker create --name "${TAG}-$nome" --network "$rede" --user 0 --entrypoint sh "$IMG_BS" \
    -c "mkdir -p /rec; exec baresip -4 -f /conf -t $tmax -e '/dial sip:$dest@$host${srv#"$host"}'" >/dev/null \
    || { echo "✗ não consegui criar o contentor do softphone $nome"; exit 1; }
  docker cp "$d/conf" "${TAG}-$nome:/conf" >/dev/null
  rm -f "$d/conf/accounts"                # a password não fica no disco do host
  docker start "${TAG}-$nome" >/dev/null
}

# dtmf <nome> <porto da consola> <dígitos> — um a um, cada um seguido de «tecla
# solta» (0x04, o KEYCODE_REL do baresip). Sem isso o dígito fica «premido» e um
# igual a seguir é ignorado: medido, «4711» chegava ao FreeSWITCH como «471»,
# com qualquer intervalo entre dígitos.
dtmf() {
  local i c
  for (( i = 0; i < ${#3}; i++ )); do
    c=${3:i:1}
    docker exec "${TAG}-$1" sh -c "printf '%s' '$c' | nc -u -w1 127.0.0.1 $2; sleep 0.25; printf '\\004' | nc -u -w1 127.0.0.1 $2" >/dev/null 2>&1
    sleep 0.35
  done
}

# recolher <nome> — espera o fim da chamada, traz a gravação do que a perna OUVIU.
# Escreve o caminho do wav em $WORK/$TAG/<nome>/ouvido (vazio se não houver).
recolher() {
  local nome=$1 tmax=$2 d="$WORK/$TAG/$1"
  esperar "${TAG}-$nome" 'terminated \(duration|session closed|ua: stop all' "$tmax" || true
  docker exec "${TAG}-$nome" sh -c "printf '/quit\n' | nc -u -w1 127.0.0.1 \$(sed -n 's/^cons_listen.*://p' /conf/config)" >/dev/null 2>&1
  sleep 1
  docker cp "${TAG}-$nome:/rec" "$d/rec" >/dev/null 2>&1
  ls "$d"/rec/*-dec.wav 2>/dev/null | head -1 > "$d/ouvido"
  logs "${TAG}-$nome" > "$d/baresip.log"
}
ouvido() { cat "$WORK/$TAG/$1/ouvido" 2>/dev/null; }

# verificar_tons <nome> <tom esperado|-> <tom próprio> — mede e julga
verificar_tons() {
  local nome=$1 esperado=$2 proprio=$3 w a
  w=$(ouvido "$nome")
  [ -n "$w" ] || { bad "$nome: sem gravação do que ouviu (o áudio nunca arrancou)"; return; }
  if [ "$esperado" != - ]; then
    a=$(medir "$w" "$esperado")
    if maior "$a" "$PRESENTE"; then ok "$nome ouviu os $esperado Hz do outro lado (amplitude $a)"
    else bad "$nome NÃO ouviu os $esperado Hz do outro lado (amplitude $a < $PRESENTE)"; fi
  fi
  a=$(medir "$w" "$proprio")
  if menor "$a" "$AUSENTE"; then ok "$nome não ouve o próprio tom de $proprio Hz (amplitude $a)"
  else bad "$nome ouve o PRÓPRIO tom de $proprio Hz (amplitude $a > $AUSENTE) — eco ou mistura sem mix-minus"; fi
}

estabelecida() {  # estabelecida <nome> <segundos> — e com SRTP
  local nome=$1
  if esperar "${TAG}-$nome" 'Call established' "$2"; then
    ok "$nome: chamada estabelecida"
  else
    bad "$nome: a chamada NÃO se estabeleceu em $2 s"
    logs "${TAG}-$nome" | grep -vE '^\s*$|audio=' | tail -6 | sed 's/^/       /'
    return 1
  fi
  if logs "${TAG}-$nome" | grep -q 'SRTP is Enabled'; then
    ok "$nome: media cifrada ($(logs "${TAG}-$nome" | sed -n 's/.*SRTP is Enabled (\(.*\)).*/\1/p' | head -1))"
  else
    bad "$nome: a media NÃO está cifrada (sem «SRTP is Enabled»)"
  fi
}

# ------------------------------------------------------------ selftest
selftest() {
  docker image inspect "$IMG_FS" >/dev/null 2>&1 \
    || { echo "✗ falta a imagem $IMG_FS (make freeswitch-image, ou FS_IMAGE=<a publicada>)"; exit 1; }
  local ip d="$WORK/$TAG/fs" pin=4711 visto a
  ip=$(python3 -c "import ipaddress,sys; print(list(ipaddress.ip_network(sys.argv[1]).hosts())[1])" "$SELFTEST_SUBNET")
  mkdir -p "$d"
  cat > "$d/prova.xml" <<EOF
<profile name="prova">
  <settings>
    <param name="context" value="softphone-prova"/>
    <param name="dialplan" value="XML"/>
    <param name="sip-ip" value="$ip"/>
    <param name="rtp-ip" value="$ip"/>
    <param name="ext-sip-ip" value="$ip"/>
    <param name="ext-rtp-ip" value="$ip"/>
    <param name="sip-port" value="5099"/>
    <param name="auth-calls" value="false"/>
    <param name="rfc2833-pt" value="101"/>
    <param name="inbound-codec-prefs" value="PCMA,PCMU"/>
    <param name="outbound-codec-prefs" value="PCMA,PCMU"/>
    <param name="rtp-timer-name" value="soft"/>
  </settings>
</profile>
EOF
  # O que RECUSA uma chamada em claro à entrada é a variável GLOBAL
  # `rtp_secure_media=mandatory` (no Meet: sip_profiles/internal.xml e o arranque), posta
  # no arranque do contentor, abaixo. Medido aqui, com o controlo negativo do
  # passo 2, contra o FreeSWITCH 1.11.3 (R226):
  #   · só `rtp-secure-media` no perfil  → chamada em claro ACEITE (não é um
  #     parâmetro do sofia: zero ocorrências em sofia.c);
  #   · só `require-secure-rtp` no perfil → ACEITE (o sofia lê-o para uma flag,
  #     PFLAG_SECURE, que mais nada no código consulta);
  #   · `set rtp_secure_media=mandatory` no dialplan antes do answer → ACEITE
  #     neste perfil, que negoceia o SDP à chegada; num perfil com
  #     `inbound-late-negotiation=true` a negociação espera pelo answer e o
  #     mesmo `set` recusa com 488;
  #   · a variável global → recusada com 488, venha ela do vars.xml ou de
  #     uma directiva no próprio ficheiro do perfil.
  # A mesma prova com a configuração que corre, e não com este perfil: `srtp-real`.
  cat > "$d/zz.xml" <<'EOF'
<include>
  <context name="softphone-prova">
    <extension name="pin-e-tom">
      <condition field="destination_number" expression="^9999$">
        <action application="answer"/>
        <action application="set" data="RECORD_STEREO=true"/>
        <action application="record_session" data="/tmp/rec/prova.wav"/>
        <action application="play_and_get_digits" data="4 4 1 9000 # silence_stream://500 silence_stream://250 prova_pin \d+ 4000"/>
        <action application="log" data="CRIT SOFTPHONE-PROVA-PIN=${prova_pin}"/>
        <action application="playback" data="tone_stream://%(5000,0,440)"/>
        <action application="hangup"/>
      </condition>
    </extension>
    <extension name="sala">
      <condition field="destination_number" expression="^8000$">
        <action application="answer"/>
        <action application="conference" data="prova@default"/>
      </condition>
    </extension>
  </context>
</include>
EOF
  docker network create --internal --subnet "$SELFTEST_SUBNET" "${TAG}-net" >/dev/null \
    || { echo "✗ não consegui criar a rede interna $SELFTEST_SUBNET (SOFTPHONE_SELFTEST_SUBNET para outra)"; exit 1; }
  # O ESL da vanilla escuta em todas as interfaces com a password por omissão:
  # fica só em loopback, dentro de uma rede sem saída.
  docker create --name "${TAG}-fs" --network "${TAG}-net" --ip "$ip" --entrypoint sh "$IMG_FS" -c '
    C=/usr/local/freeswitch/etc/freeswitch
    rm -rf $C/sip_profiles/*; cp /prova/prova.xml $C/sip_profiles/; cp /prova/zz.xml $C/dialplan/zz_softphone_prova.xml
    sed -i "s#name=\"listen-ip\" value=\"[^\"]*\"#name=\"listen-ip\" value=\"127.0.0.1\"#" $C/autoload_configs/event_socket.conf.xml
    sed -i "s#</include>#  <X-PRE-PROCESS cmd=\"set\" data=\"rtp_secure_media=mandatory\"/>\n</include>#" $C/vars.xml
    mkdir -p /tmp/rec; exec freeswitch -nonat -nf -nc' >/dev/null
  docker cp "$d" "${TAG}-fs:/prova" >/dev/null
  docker start "${TAG}-fs" >/dev/null
  local i pronto=0
  for i in $(seq 1 60); do
    docker exec "${TAG}-fs" fs_cli -x "sofia status" 2>/dev/null | grep -Eq 'prova.*RUNNING' && { pronto=1; break; }
    sleep 1
  done
  [ "$pronto" -eq 1 ] || { echo "✗ o FreeSWITCH do selftest não ficou pronto"; docker logs --tail 20 "${TAG}-fs"; exit 1; }
  local rede="container:${TAG}-fs" srv="$ip:5099"

  echo "1) PIN por DTMF e tons nos dois sentidos, com SRTP obrigatório"
  perna a "$rede" "$srv" prova - 9999 1000 udp srtp-mand 5080 5555 42000 "$ip" 40
  if estabelecida a 20; then
    sleep 1.5; dtmf a 5555 "$pin"
    recolher a 30
    visto=$(docker exec "${TAG}-fs" sh -c "grep -a 'SOFTPHONE-PROVA-PIN=' /usr/local/freeswitch/var/log/freeswitch/freeswitch.log | grep -v 'Action log' | tail -1 | sed 's/.*PIN=//'" | tr -d '[:space:]')
    if [ "$visto" = "$pin" ]; then ok "o FreeSWITCH recebeu o PIN por DTMF ($visto)"
    else bad "o FreeSWITCH recebeu o PIN «$visto», não «$pin»"; fi
    verificar_tons a 440 1000
    docker cp "${TAG}-fs:/tmp/rec/prova.wav" "$d/fs-prova.wav" >/dev/null 2>&1
    a=$(medir "$d/fs-prova.wav" 1000)
    if maior "$a" "$PRESENTE"; then ok "o FreeSWITCH gravou os 1000 Hz do softphone (amplitude $a)"
    else bad "o FreeSWITCH NÃO gravou os 1000 Hz do softphone (amplitude $a)"; fi
  fi
  docker rm -f "${TAG}-a" >/dev/null 2>&1

  echo "2) controlo negativo: sem SRTP a chamada tem de ser recusada"
  perna n "$rede" "$srv" prova - 9999 1000 udp nenhum 5080 5555 42000 "$ip" 15
  if esperar "${TAG}-n" 'Call established' 8; then
    bad "uma chamada SEM SRTP foi aceite — a variável global rtp_secure_media não está a valer"
  else
    ok "chamada sem SRTP recusada ($(logs "${TAG}-n" | sed -n 's/.*session closed: //p' | head -1))"
  fi
  docker rm -f "${TAG}-n" >/dev/null 2>&1

  echo "3) par de softphones na mesma conferência: cada um ouve o outro, não a si"
  perna a "$rede" "$srv" prova-a - 8000 1000 udp srtp-mand 5080 5555 42000 "$ip" 25
  perna b "$rede" "$srv" prova-b - 8000 440  udp srtp-mand 5084 5556 42200 "$ip" 25
  if estabelecida a 20 && estabelecida b 20; then
    sleep 8
    docker exec "${TAG}-a" sh -c "printf '/hangup\n' | nc -u -w1 127.0.0.1 5555" >/dev/null 2>&1
    docker exec "${TAG}-b" sh -c "printf '/hangup\n' | nc -u -w1 127.0.0.1 5556" >/dev/null 2>&1
    recolher a 10; recolher b 10
    verificar_tons a 440 1000
    verificar_tons b 1000 440
  fi
}

# ------------------------------------------------------------ srtp-real / srtp-cluster
# O controlo negativo contra a configuração que CORRE (R226, R227). Nos dois
# ambientes o FreeSWITCH arranca pelo voice/cluster/freeswitch-entrypoint.sh,
# com os ficheiros do Meet montados em /meet; o que muda é quem os monta:
#   srtp-real     o serviço `freeswitch` do compose.yaml;
#   srtp-cluster  o ConfigMap `freeswitch-meet` do scripts/cluster-voice.sh.
# A lista de ficheiros tira-se de cada um deles, para a prova não ter a sua.
# Mede os dois perfis: o dos ramais (autenticado) e o `external` do dial-in,
# que é o que o Kamailio alcança.

# «<origem no repo> <nome em /meet>», um por linha.
ficheiros_compose() {
  sed -n 's#^ *- \./\(voice/[^:]*\):/meet/\([^:]*\)\(:ro\)\{0,1\} *$#\1 \2#p' compose.yaml
}
ficheiros_cluster() {
  sed -n '/^cm freeswitch-meet/,/^cm [a-z]/p' scripts/cluster-voice.sh \
    | sed -n 's#.*--from-file=\(voice/[^ ]*\).*#\1#p' | while read -r f; do echo "$f ${f##*/}"; done
}

# fim_da_chamada <nome> <segundos> — «estabelecida», ou a resposta SIP que a fechou
fim_da_chamada() {
  esperar "${TAG}-$1" 'Call established|session closed: ' "$2" || { echo "sem resposta em $2 s"; return; }
  if logs "${TAG}-$1" | grep -q 'Call established'; then echo estabelecida
  else logs "${TAG}-$1" | sed -n 's/.*session closed: //p' | head -1; fi
}
# fs_cli_cluster <comando> — o entrypoint dá ao ESL uma password aleatória
fs_cli_cluster() {
  docker exec "${TAG}-fs" sh -c 'fs_cli -p "$(sed -n "s/.*name=\"password\" value=\"\([^\"]*\)\".*/\1/p" /conf/autoload_configs/event_socket.conf.xml)" -x "$0"' "$1" 2>/dev/null
}
# perfil_cluster <nome do perfil> — «ip:porto» onde escuta, vazio se não está RUNNING
perfil_cluster() {
  fs_cli_cluster "sofia status" | sed -n "s/^ *$1[[:space:]]\{1,\}profile[[:space:]]\{1,\}sip:mod_sofia@\([^[:space:]]*\)[[:space:]]\{1,\}RUNNING.*/\1/p" | head -1
}

# srtp_arranque <compose|cluster>
srtp_arranque() {
  local origem=$1 quem
  case "$origem" in
    compose) quem="o serviço freeswitch do compose.yaml"
             grep -q '\./voice/cluster/freeswitch-entrypoint\.sh:/entrypoint/freeswitch-entrypoint\.sh' compose.yaml \
               || { echo "✗ o compose.yaml já não arranca o FreeSWITCH pelo voice/cluster/freeswitch-entrypoint.sh"; exit 1; } ;;
    cluster) quem="o ConfigMap freeswitch-meet (scripts/cluster-voice.sh)" ;;
  esac
  docker image inspect "$IMG_FS" >/dev/null 2>&1 \
    || { echo "✗ falta a imagem $IMG_FS (make freeswitch-image, ou FS_IMAGE=<a publicada>)"; exit 1; }
  local d="$WORK/$TAG/fs" ip senha errada=senha-errada segredo f nome n=0 i ram="" ext="" resp v lip
  ip=$(python3 -c "import ipaddress,sys; print(list(ipaddress.ip_network(sys.argv[1]).hosts())[1])" "$SELFTEST_SUBNET")
  senha=$(python3 -c "import secrets; print(secrets.token_hex(12))")
  segredo=$(python3 -c "import secrets; print(secrets.token_hex(32))")
  mkdir -p "$d/meet" "$d/entrypoint"
  while read -r f nome; do
    [ -f "$f" ] || { echo "✗ $quem monta $f, e o ficheiro não existe"; exit 1; }
    cp "$f" "$d/meet/$nome"; n=$(( n + 1 ))
  done < <("ficheiros_$origem")
  [ "$n" -gt 0 ] || { echo "✗ não encontrei os ficheiros que $quem monta em /meet"; exit 1; }
  cp voice/cluster/freeswitch-entrypoint.sh "$d/entrypoint/"
  docker network create --internal --subnet "$SELFTEST_SUBNET" "${TAG}-net" >/dev/null \
    || { echo "✗ não consegui criar a rede interna $SELFTEST_SUBNET (SOFTPHONE_SELFTEST_SUBNET para outra)"; exit 1; }
  # O ambiente é o do contentor do FreeSWITCH (compose.yaml e
  # deploy/k8s/cluster/voice.yaml dão-lhe o mesmo), com os endereços do
  # servidor trocados por loopback. Numa rede sem saída o FreeSWITCH fica em
  # 127.0.0.1, e por isso a lista de acesso dos ramais é a de loopback.
  docker create --name "${TAG}-fs" --network "${TAG}-net" --ip "$ip" \
    -e DELONIX_CONTROL_URL=http://127.0.0.1:8181 \
    -e DELONIX_RAMAIS_SIP_PORT="$PORTO_RAMAIS" -e VOICE_INTERNAL_SECRET="$segredo" -e DELONIX_RAMAIS_ACL=127.0.0.0/8 \
    --entrypoint sh "$IMG_FS" /entrypoint/freeswitch-entrypoint.sh >/dev/null
  docker cp "$d/meet" "${TAG}-fs:/meet" >/dev/null
  docker cp "$d/entrypoint" "${TAG}-fs:/entrypoint" >/dev/null
  docker start "${TAG}-fs" >/dev/null
  for i in $(seq 1 60); do
    [ "$(docker inspect -f '{{.State.Running}}' "${TAG}-fs" 2>/dev/null)" = true ] || break
    ram=$(perfil_cluster internal); ext=$(perfil_cluster external)
    [ -n "$ram" ] && [ -n "$ext" ] && break
    sleep 1
  done
  echo "configuração: a que o voice/cluster/freeswitch-entrypoint.sh monta, com os $n ficheiros que $quem põe em /meet, em $IMG_FS"
  echo "andaime: um servidor que responde a tudo com o directório de um só ramal (como o ramais.rs) e guarda os pedidos; lista de acesso dos ramais em loopback"
  if [ -z "$ram" ] || [ -z "$ext" ]; then
    bad "o FreeSWITCH do entrypoint não pôs os dois perfis a correr (internal=«$ram» external=«$ext»)"
    logs "${TAG}-fs" | tail -8 | sed 's/^/       /'
    return
  fi
  lip=${ram%%:*}
  if [ "${ram##*:}" = "$PORTO_RAMAIS" ]; then ok "perfil dos ramais a escutar em $ram; perfil do dial-in em $ext"
  else bad "perfil dos ramais a escutar em $ram, não no porto $PORTO_RAMAIS"; fi
  v=$(fs_cli_cluster "global_getvar rtp_secure_media" | tr -d '[:space:]')
  if [ "$v" = mandatory ]; then ok "variável global rtp_secure_media=mandatory"
  else bad "variável global rtp_secure_media=«$v» — nada impõe SRTP à entrada"; fi

  # O directório: a resposta do servidor (server/src/ramais.rs) para um ramal,
  # servida a qualquer pedido. O HA1 é o do Digest: MD5(utilizador:domínio:password).
  python3 - "$lip" "$senha" > "$d/resposta" <<'PY'
import hashlib, sys
dominio, senha = sys.argv[1], sys.argv[2]
ha1 = hashlib.md5(f"prova:{dominio}:{senha}".encode()).hexdigest()
corpo = f"""<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<document type="freeswitch/xml">
  <section name="directory">
    <domain name="{dominio}">
      <groups><group name="default"><users>
        <user id="prova">
          <params>
            <param name="a1-hash" value="{ha1}"/>
            <param name="auth-acl" value="delonix_ramais"/>
          </params>
          <variables>
            <variable name="user_context" value="delonix_ramais"/>
          </variables>
        </user>
      </users></group></groups>
    </domain>
  </section>
</document>
"""
sys.stdout.write("HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: %d\r\nConnection: close\r\n\r\n%s"
                 % (len(corpo.encode()), corpo))
PY
  # O servidor de andaime lê o pedido INTEIRO, guarda-o, e só depois responde.
  # Um `nc` que respondesse logo à ligação deixava o FreeSWITCH fechar antes de
  # o pedido ser lido, e a prova ficava a depender de uma corrida.
  cat > "$d/servidor.sh" <<'SH'
p=$1; CR=$(printf '\r'); mkfifo "/f-$p"; exec 3<>"/f-$p"
# Um só `nc -k`: o porto fica sempre à escuta, e dois pedidos seguidos (o
# directório e logo o Lua) não encontram a porta fechada entre um e outro.
nc -l -k -p "$p" <&3 | while :; do
  cl=0; visto=0
  while IFS= read -r l; do
    visto=1; l=${l%"$CR"}; printf '%s\n' "$l" >> "/pedidos-$p"
    [ -z "$l" ] && break
    case "$l" in [Cc]ontent-[Ll]ength:*) cl=${l#*:}; cl=${cl# } ;; esac
  done
  [ "$visto" = 1 ] || exit 0
  [ "${cl:-0}" -gt 0 ] && head -c "$cl" >> "/pedidos-$p"
  printf '\n' >> "/pedidos-$p"
  cat /resposta >&3
done
SH
  docker create --name "${TAG}-dir" --network "container:${TAG}-fs" --user 0 --entrypoint sh "$IMG_BS" \
    -c 'sh /servidor.sh 8180 & sh /servidor.sh 8181 & wait' >/dev/null
  docker cp "$d/resposta" "${TAG}-dir:/resposta" >/dev/null
  docker cp "$d/servidor.sh" "${TAG}-dir:/servidor.sh" >/dev/null
  rm -f "$d/resposta"
  docker start "${TAG}-dir" >/dev/null
  local rede="container:${TAG}-fs"

  echo "1) ramais: com SRTP e a password ERRADA a chamada não entra"
  perna e "$rede" "$ram" prova errada "$DESTINO_REAL" 1000 udp srtp-mand 5082 5555 42000 "$lip" 20
  resp=$(fim_da_chamada e 15)
  case "$resp" in
    401*|403*|407*) ok "password errada: recusada ($resp)" ;;
    *) bad "com a password errada a chamada não foi recusada pela autenticação ($resp)" ;;
  esac
  docker rm -f "${TAG}-e" >/dev/null 2>&1

  echo "2) ramais, controlo positivo: com a password certa e COM SRTP, a chamada passa a negociação"
  perna p "$rede" "$ram" prova senha "$DESTINO_REAL" 1000 udp srtp-mand 5082 5555 42000 "$lip" 25
  resp=$(fim_da_chamada p 20)
  case "$resp" in
    estabelecida) ok "com SRTP: chamada estabelecida" ;;
    488*|401*|403*|407*|"sem resposta"*) bad "com SRTP a chamada não passou a autenticação e a negociação ($resp) — o controlo negativo abaixo não prova nada" ;;
    *) ok "com SRTP: autenticada e negociada; quem a fechou foi o plano de marcação ($resp)" ;;
  esac
  docker rm -f "${TAG}-p" >/dev/null 2>&1

  echo "3) ramais, controlo negativo: a MESMA chamada sem SRTP tem de levar 488"
  perna n "$rede" "$ram" prova senha "$DESTINO_REAL" 1000 udp nenhum 5082 5555 42000 "$lip" 25
  resp=$(fim_da_chamada n 20)
  case "$resp" in
    488*) ok "sem SRTP: recusada ($resp)" ;;
    estabelecida) bad "uma chamada SEM SRTP ao perfil dos ramais foi ACEITE" ;;
    *) bad "sem SRTP: a chamada não levou 488, levou «$resp»" ;;
  esac
  docker rm -f "${TAG}-n" >/dev/null 2>&1

  echo "4) dial-in, controlo positivo: COM SRTP a chamada é atendida pelo IVR"
  perna q "$rede" "$ext" tronco - "$DESTINO_DIALIN" 1000 udp srtp-mand 5082 5555 42000 "$lip" 25
  resp=$(fim_da_chamada q 20)
  case "$resp" in
    estabelecida) ok "com SRTP: chamada estabelecida"
                  sleep 2; dtmf q 5555 "$PIN_DIALIN"; sleep 4 ;;   # o IVR valida o PIN no servidor
    *) bad "com SRTP a chamada ao dial-in não foi atendida ($resp) — o controlo negativo abaixo não prova nada" ;;
  esac
  docker rm -f "${TAG}-q" >/dev/null 2>&1

  echo "5) dial-in, controlo negativo: a MESMA chamada sem SRTP tem de levar 488"
  perna m "$rede" "$ext" tronco - "$DESTINO_DIALIN" 1000 udp nenhum 5082 5555 42000 "$lip" 25
  resp=$(fim_da_chamada m 20)
  case "$resp" in
    488*) ok "sem SRTP: recusada ($resp)" ;;
    estabelecida) bad "uma chamada SEM SRTP ao dial-in foi ACEITE" ;;
    *) bad "sem SRTP: a chamada não levou 488, levou «$resp»" ;;
  esac
  docker rm -f "${TAG}-m" >/dev/null 2>&1
  v=$(docker exec "${TAG}-fs" grep -ac 'Crypto not negotiated but required' /usr/local/freeswitch/var/log/freeswitch/freeswitch.log)
  if [ "${v:-0}" -ge 2 ]; then ok "o FreeSWITCH registou a razão das duas recusas: «Crypto not negotiated but required»"
  else bad "o FreeSWITCH registou «Crypto not negotiated but required» ${v:-0} vez(es), não duas"; fi

  echo "6) o segredo de voz chega ao servidor em cabeçalhos, e não fica no log (R227)"
  # Os registos de chamada (mod_json_cdr) saem quando a chamada ACABA: os
  # softphones foram removidos sem desligar, por isso desliga-se tudo aqui.
  fs_cli_cluster "hupall" >/dev/null; sleep 4
  docker exec "${TAG}-dir" sh -c 'cat /pedidos-8180 /pedidos-8181 2>/dev/null' | tr -d '\r' > "$d/pedidos"
  while IFS='|' read -r veredicto texto; do
    if [ "$veredicto" = ok ]; then ok "$texto"; else bad "$texto"; fi
  done < <(python3 - "$d/pedidos" "$segredo" "$PIN_DIALIN" "$DESTINO_DIALIN" <<'PY'
import base64, re, sys
pedidos, segredo, pin, dialin = open(sys.argv[1], errors="replace").read(), sys.argv[2], sys.argv[3], sys.argv[4]
# Cada pedido começa numa linha «POST <caminho> HTTP/1.1».
blocos = re.split(r"(?m)^(?=POST \S+ HTTP/1\.[01]$)", pedidos)
def pedido(caminho):
    return next((b for b in blocos if b.startswith("POST " + caminho + " ")), None)
def sai(certo, texto):
    print(("ok" if certo else "falha") + "|" + texto)

linhas = [b.split("\n", 1)[0] for b in blocos if b.startswith("POST ")]
no_url = [l for l in linhas if segredo in l or "secret=" in l]
sai(bool(linhas) and not no_url,
    "nenhum dos %d pedidos ao servidor leva o segredo no URL" % len(linhas) if linhas and not no_url
    else "%d dos %d pedidos ao servidor levam o segredo no URL" % (len(no_url), len(linhas)))

b = pedido("/internal/v1/voice/ivr/directory")
m = b and re.search(r"(?mi)^Authorization: Basic (\S+)$", b)
certo = False
if m:
    try:
        certo = base64.b64decode(m.group(1)).decode().split(":", 1)[1] == segredo
    except Exception:
        certo = False
sai(certo, "directório (mod_xml_curl): o segredo vai em Authorization: Basic" if certo
    else "directório (mod_xml_curl): o pedido não leva o segredo em Authorization: Basic")

for caminho, quem, corpo in (("/internal/v1/voice/ivr/resolve-extension", "ramais_dial.lua", None),
                             ("/internal/v1/voice/ivr/validate", "dialin_ivr.lua", pin)):
    b = pedido(caminho)
    if b is None:
        sai(False, "%s: o pedido a %s nunca chegou ao servidor" % (quem, caminho))
        continue
    tem = re.search(r"(?mi)^X-Voice-Secret: (\S+)$", b)
    json_ok = re.search(r"(?mi)^Content-Type: application/json", b) is not None
    certo = bool(tem) and tem.group(1) == segredo and json_ok and (corpo is None or ('"pin":"%s"' % corpo) in b)
    sai(certo, "%s: o pedido chega com X-Voice-Secret e o corpo JSON%s" % (quem, " com o PIN marcado" if corpo else "")
        if certo else "%s: o pedido a %s não chegou como devia (X-Voice-Secret, Content-Type ou corpo)" % (quem, caminho))

# Os registos de chamada (mod_json_cdr). Um registo leva TODAS as variáveis do
# canal — o SDP com as linhas a=crypto e as chaves SRTP da perna incluídas — e
# vai em HTTP para o servidor, ou fica em disco se ele não responder. Por isso
# só a perna de um TRONCO deixa registo (R291): nos contextos dos ramais e do
# dial-in o plano de marcação desliga-o (`process_cdr=false`). O que ainda
# chega daqui são as chamadas recusadas ANTES do plano de marcação, que não
# negociaram chave nenhuma — e é por elas que se sabe que o módulo entrega.
# O módulo acrescenta «?uuid=<a chamada>» ao endereço.
cdrs = [b for b in blocos if re.match(r"POST /internal/v1/telephony/call-records[? ]", b)]
corpo = lambda b: b.split("\n\n", 1)[-1]
def basic_certo(b):
    m = re.search(r"(?mi)^Authorization: Basic (\S+)$", b)
    try:
        return bool(m) and base64.b64decode(m.group(1)).decode().split(":", 1)[1] == segredo
    except Exception:
        return False
sai(bool(cdrs) and all(basic_certo(b) for b in cdrs),
    "registos de chamada (mod_json_cdr): %d entregues, todos com o segredo em Authorization: Basic" % len(cdrs)
    if cdrs and all(basic_certo(b) for b in cdrs)
    else "registos de chamada (mod_json_cdr): %d entregues; nem todos (ou nenhum) com o segredo em Authorization: Basic" % len(cdrs))
atendidas = [b for b in cdrs if re.search(r'"answer_epoch":"[1-9]', corpo(b))]
sai(bool(cdrs) and not atendidas,
    "nenhuma chamada atendida de ramal ou de dial-in deixa registo" if cdrs and not atendidas
    else "%d chamada(s) atendida(s) de ramal ou de dial-in deixaram registo" % len(atendidas))
# Os valores vão codificados em URL: «inline:» é «inline%3A».
com_chave = [b for b in cdrs if re.search(r"inline(:|%3A)", corpo(b), re.I)]
sai(bool(cdrs) and not com_chave,
    "nenhum dos %d registos de chamada leva uma chave SRTP" % len(cdrs) if cdrs and not com_chave
    else "%d dos %d registos de chamada levam chaves SRTP (a=crypto … inline:)" % (len(com_chave), len(cdrs)))
# O PIN como número inteiro, não como parte de um mais comprido (um registo
# traz dezenas de tempos e contadores).
com_pin = [b for b in cdrs if re.search(r"(?<!\d)%s(?!\d)" % re.escape(pin), corpo(b))]
sai(bool(cdrs) and not com_pin,
    "nenhum dos %d registos de chamada leva o PIN marcado" % len(cdrs) if cdrs and not com_pin
    else "%d dos %d registos de chamada levam o PIN marcado" % (len(com_pin), len(cdrs)))
PY
  )
  v=$(docker exec "${TAG}-fs" grep -ac "$segredo" /usr/local/freeswitch/var/log/freeswitch/freeswitch.log)
  if [ "${v:-1}" -eq 0 ]; then ok "o segredo de voz não aparece no freeswitch.log"
  else bad "o segredo de voz aparece $v vez(es) no freeswitch.log"; fi
  v=$(docker exec "${TAG}-fs" grep -ac "\"pin\":\"$PIN_DIALIN\"" /usr/local/freeswitch/var/log/freeswitch/freeswitch.log)
  if [ "${v:-1}" -eq 0 ]; then ok "o PIN marcado não aparece no freeswitch.log"
  else bad "o PIN marcado aparece $v vez(es) no freeswitch.log"; fi
  # Nem dígito a dígito: sem `sensitive_dtmf` o FreeSWITCH escreve uma linha
  # «RECV DTMF <dígito>» por tecla, e o PIN lê-se de cima para baixo.
  v=$(docker exec "${TAG}-fs" grep -ac 'RECV DTMF' /usr/local/freeswitch/var/log/freeswitch/freeswitch.log)
  if [ "${v:-1}" -eq 0 ]; then ok "os dígitos marcados não aparecem no freeswitch.log (nenhuma linha «RECV DTMF»)"
  else bad "o freeswitch.log tem $v linha(s) «RECV DTMF»: o PIN lê-se dígito a dígito"; fi
  # O freeswitch.xml.fsxml (a configuração expandida, com o segredo) não pode
  # estar no directório de logs: o arranque manda-o para um directório privado.
  v=$(docker exec "${TAG}-fs" sh -c "grep -rl -a '$segredo' /usr/local/freeswitch/var/log 2>/dev/null | wc -l")
  if [ "${v:-1}" -eq 0 ]; then ok "nenhum ficheiro do directório de logs traz o segredo de voz"
  else bad "$v ficheiro(s) do directório de logs trazem o segredo de voz: $(docker exec "${TAG}-fs" sh -c "grep -rl -a '$segredo' /usr/local/freeswitch/var/log" | tr '\n' ' ')"; fi
  rm -f "$d/pedidos"
}

# ------------------------------------------------------------ chamada / par
SERVIDOR= DESTINO= PIN= TRANSPORTE=udp SEGUNDOS=10 ESPERA_PIN=3 INTERFACE=0.0.0.0 DOMINIO= REDE=host
UTIL= UTIL_A= UTIL_B= TOM=1000 ESPERA_TOM=-
argumentos() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --servidor) SERVIDOR=$2; shift ;;
      --destino) DESTINO=$2; shift ;;
      --pin) PIN=$2; shift ;;
      --transporte) TRANSPORTE=$2; shift ;;
      --segundos) SEGUNDOS=$2; shift ;;
      --espera-pin) ESPERA_PIN=$2; shift ;;
      --escuta) INTERFACE=$2; shift ;;
      --dominio) DOMINIO=$2; shift ;;
      --rede) REDE=$2; shift ;;
      --utilizador) UTIL=$2; shift ;;
      --utilizador-a) UTIL_A=$2; shift ;;
      --utilizador-b) UTIL_B=$2; shift ;;
      --tom) TOM=$2; shift ;;
      --espera-tom) ESPERA_TOM=$2; shift ;;
      *) echo "✗ argumento desconhecido: $1"; uso 2 ;;
    esac
    shift
  done
  [ -n "$SERVIDOR" ] && [ -n "$DESTINO" ] || { echo "✗ faltam --servidor e --destino"; uso 2; }
  case "$SERVIDOR" in *:*) ;; *) SERVIDOR="$SERVIDOR:5060" ;; esac
  case "$TRANSPORTE" in udp|tcp|tls) ;; *) echo "✗ --transporte é udp, tcp ou tls"; exit 2 ;; esac
}
enviar_pin() {  # enviar_pin <nome> <porto da consola>
  [ -n "$PIN" ] || return 0
  sleep "$ESPERA_PIN"; dtmf "$1" "$2" "$PIN#"
}
desligar() { docker exec "${TAG}-$1" sh -c "printf '/hangup\n' | nc -u -w1 127.0.0.1 $2" >/dev/null 2>&1; }

chamada() {
  argumentos "$@"
  [ -n "$UTIL" ] || { echo "✗ falta --utilizador"; uso 2; }
  echo "chamada: $UTIL → $DESTINO em $SERVIDOR ($TRANSPORTE, SRTP obrigatório)"
  perna a "$REDE" "$SERVIDOR" "$UTIL" SOFTPHONE_PASSWORD "$DESTINO" "$TOM" "$TRANSPORTE" srtp-mand 5082 55551 42000 "$INTERFACE" $(( SEGUNDOS + 60 ))
  if estabelecida a 25; then
    enviar_pin a 55551
    sleep "$SEGUNDOS"; desligar a 55551
    recolher a 10
    verificar_tons a "$ESPERA_TOM" "$TOM"
    [ -n "$(ouvido a)" ] && cp "$(ouvido a)" "$WORK/ultima-chamada-ouvido.wav" 2>/dev/null \
      && echo "  gravação do que o softphone ouviu: .softphone-prova/ultima-chamada-ouvido.wav"
  fi
}

par() {
  argumentos "$@"
  [ -n "$UTIL_A" ] && [ -n "$UTIL_B" ] || { echo "✗ faltam --utilizador-a e --utilizador-b"; uso 2; }
  echo "par: $UTIL_A (1000 Hz) e $UTIL_B (440 Hz) → $DESTINO em $SERVIDOR ($TRANSPORTE, SRTP obrigatório)"
  perna a "$REDE" "$SERVIDOR" "$UTIL_A" SOFTPHONE_PASSWORD_A "$DESTINO" 1000 "$TRANSPORTE" srtp-mand 5082 55551 42000 "$INTERFACE" $(( SEGUNDOS + 90 ))
  estabelecida a 25 || return
  enviar_pin a 55551
  perna b "$REDE" "$SERVIDOR" "$UTIL_B" SOFTPHONE_PASSWORD_B "$DESTINO" 440 "$TRANSPORTE" srtp-mand 5086 55552 42200 "$INTERFACE" $(( SEGUNDOS + 60 ))
  estabelecida b 25 || return
  enviar_pin b 55552
  sleep "$SEGUNDOS"; desligar a 55551; desligar b 55552
  recolher a 10; recolher b 10
  verificar_tons a 440 1000
  verificar_tons b 1000 440
}

# ------------------------------------------------------------ principal
[ $# -ge 1 ] || uso 2
modo=$1; shift
case "$modo" in -h|--help|ajuda) uso 0 ;; esac
# `medir <ficheiro.wav> <Hz>` — só a medição, para outra prova a usar com uma
# gravação sua (scripts/troncos-prova.sh mede o que a operadora de ensaio ouviu).
if [ "$modo" = medir ]; then
  trap - EXIT
  [ $# -eq 2 ] && [ -f "$1" ] || { echo "✗ uso: medir <ficheiro.wav> <Hz>"; exit 2; }
  medir "$1" "$2"; exit 0
fi
command -v docker >/dev/null 2>&1 || { echo "✗ precisa de docker (o softphone e o FreeSWITCH do selftest correm em contentores)"; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "✗ precisa de python3 (gera e mede os tons)"; exit 1; }
mkdir -p "$WORK/$TAG"
imagem_baresip
case "$modo" in
  selftest) selftest ;;
  srtp-real) srtp_arranque compose ;;
  srtp-cluster) srtp_arranque cluster ;;
  chamada)  chamada "$@" ;;
  par)      par "$@" ;;
  *) echo "✗ modo desconhecido: $modo"; uso 2 ;;
esac
if [ "$fail" -eq 0 ]; then echo "✓ prova de softphone ($modo): tudo medido e dentro dos limites"
else echo "✗ prova de softphone ($modo): há falhas acima"; fi
exit "$fail"
