#!/bin/sh
# FreeSWITCH do Meet no cluster local.
#
# A imagem (voice/freeswitch/image/) traz a configuração VANILLA, que escuta
# SIP com utilizadores e passwords por omissão. Este arranque copia-a para um
# volume, fecha o que a vanilla deixa aberto, e põe por cima os ficheiros do
# Meet — nunca se arranca a vanilla tal como vem.
set -eu

VANILLA=/usr/local/freeswitch/etc/freeswitch
CONF=/conf
MEET=/meet

# De raiz a cada arranque: um contentor reiniciado voltava a aplicar as
# alterações por cima das anteriores.
# Esvazia-se, não se remove: no cluster `/conf` é um volume montado.
mkdir -p "$CONF"
find "$CONF" -mindepth 1 -delete
cp -a "$VANILLA/." "$CONF/"

# 1. Utilizadores de demonstração (1000–1019, password 1234) e gateway de exemplo.
rm -f "$CONF"/directory/default/*.xml "$CONF"/sip_profiles/external/*.xml
# 2. Perfis que não usamos: IPv6 e o `internal` da vanilla (5060, sem o nosso directório).
rm -f "$CONF"/sip_profiles/*-ipv6.xml "$CONF"/sip_profiles/internal.xml
# 3. Password por omissão aleatória; nada de STUN para descobrir o endereço.
PW=$(head -c 18 /dev/urandom | od -An -tx1 | tr -d ' \n')
sed -i -E "s#default_password=[^\"]*#default_password=${PW}#" "$CONF/vars.xml"
sed -i -E 's#cmd="stun-set" data="(external_rtp_ip|external_sip_ip)=stun:[^"]*"#cmd="set" data="\1=$${local_ip_v4}"#' "$CONF/vars.xml"
#    A voz do IVR: português por omissão (DELONIX_IVR_VOICE=en/us/callie para inglês).
VOZ=${DELONIX_IVR_VOICE:-pt/BR/karina}
test -d "/usr/local/freeswitch/share/freeswitch/sounds/$VOZ" ||
  { echo "voz do IVR sem sons na imagem: $VOZ" >&2; exit 1; }
sed -i -E "s#(data=\"sound_prefix=)[^\"]*#\1\$\${sounds_dir}/${VOZ}#" "$CONF/vars.xml"
# 4. ESL só em loopback, com password aleatória.
ESL=$(head -c 18 /dev/urandom | od -An -tx1 | tr -d ' \n')
sed -i -E "s#(name=\"listen-ip\" value=)\"[^\"]*\"#\1\"127.0.0.1\"#; s#(name=\"password\" value=)\"[^\"]*\"#\1\"${ESL}\"#" \
  "$CONF/autoload_configs/event_socket.conf.xml"
# 5. Os módulos de que o IVR e os ramais precisam (a vanilla não carrega o mod_curl).
#    Na vanilla estão comentados: descomenta-se a linha, não se acrescenta outra.
for m in mod_curl mod_xml_curl; do
  sed -i "s#<!-- <load module=\"$m\"/> -->#<load module=\"$m\"/>#" "$CONF/autoload_configs/modules.conf.xml"
done
for m in mod_curl mod_lua mod_xml_curl; do
  grep -q "^[[:space:]]*<load module=\"$m\"/>" "$CONF/autoload_configs/modules.conf.xml" ||
    { echo "o módulo $m não ficou carregado" >&2; exit 1; }
done

# 6. As variáveis do Meet vêm do ambiente do pod (Secret e ConfigMap).
cat >"$CONF/vars-meet.xml" <<XML
<include>
  <X-PRE-PROCESS cmd="set" data="delonix_control_url=${DELONIX_CONTROL_URL}"/>
  <X-PRE-PROCESS cmd="set" data="delonix_api_url=${DELONIX_API_URL}"/>
  <X-PRE-PROCESS cmd="set" data="delonix_voice_secret=${VOICE_INTERNAL_SECRET}"/>
  <X-PRE-PROCESS cmd="set" data="delonix_ramais_sip_port=${DELONIX_RAMAIS_SIP_PORT:-5070}"/>
  <X-PRE-PROCESS cmd="set" data="rtp_secure_media=mandatory"/>
</include>
XML
chmod 600 "$CONF/vars-meet.xml"
sed -i 's#</include>#  <X-PRE-PROCESS cmd="include" data="vars-meet.xml"/>\n</include>#' "$CONF/vars.xml"

# 7. Os ficheiros do Meet (voice/freeswitch/ do repo, montados em /meet).
#    Os dialplans do Meet trazem o seu próprio <context>: são ficheiros de topo
#    (dialplan/*.xml), não drop-ins de dialplan/public/. Postos como drop-in, o
#    contexto `public` da vanilla respondia primeiro e desligava a chamada sem
#    chegar ao IVR. Substituem-se os da vanilla, que além disso trazem as
#    extensões de demonstração.
rm -rf "$CONF"/dialplan/*.xml "$CONF"/dialplan/public "$CONF"/dialplan/default "$CONF"/dialplan/skinny-patterns
cp "$MEET/00_delonix_dialin.xml" "$CONF/dialplan/public.xml"
cp "$MEET/00_delonix_extensions.xml" "$CONF/dialplan/delonix_ramais.xml"
cp "$MEET/conference.conf.xml" "$MEET/xml_curl.conf.xml" "$CONF/autoload_configs/"
cp "$MEET/internal.xml" "$CONF/sip_profiles/"
mkdir -p /scripts
cp "$MEET/dialin_ivr.lua" "$MEET/ramais_dial.lua" /scripts/

# 8. Dois endereços do servidor, porque são dois listeners: o IVR do dial-in
#    fala com o INTERNO (/internal/v1/voice/ivr/*, DELONIX_CONTROL_URL); os
#    ramais — mod_xml_curl e ramais_dial.lua — pedem /api/voice/ivr/*, que
#    vivem no PÚBLICO (DELONIX_API_URL). Os ficheiros do repo usam uma só
#    variável para os dois; aqui separa-se nas cópias.
sed -i 's#\$\${delonix_control_url}/api/#$${delonix_api_url}/api/#g' "$CONF/autoload_configs/xml_curl.conf.xml"
sed -i 's#global_getvar delonix_control_url#global_getvar delonix_api_url#' /scripts/ramais_dial.lua

# 9. Ramais alcançáveis de FORA da rede dos contentores (um softphone na rede
#    local): o FreeSWITCH tem de anunciar no SIP e no SDP o endereço por onde o
#    telefone o alcança — o IP da máquina —, e usar uma gama de portas de áudio
#    pequena, que é a que se publica. Sem DELONIX_EXTERNAL_IP nada muda.
if [ -n "${DELONIX_EXTERNAL_IP:-}" ]; then
  sed -i "s#<param name=\"sip-ip\" value=\"[^\"]*\"/>#&\n      <param name=\"ext-sip-ip\" value=\"${DELONIX_EXTERNAL_IP}\"/>\n      <param name=\"ext-rtp-ip\" value=\"${DELONIX_EXTERNAL_IP}\"/>#" \
    "$CONF/sip_profiles/internal.xml"
  grep -q "ext-rtp-ip" "$CONF/sip_profiles/internal.xml" ||
    { echo "não consegui pôr o endereço externo no perfil dos ramais" >&2; exit 1; }
  sed -i -E "s#<!-- <param name=\"rtp-start-port\" value=\"[0-9]+\"/> -->#<param name=\"rtp-start-port\" value=\"${DELONIX_RTP_MIN:-20000}\"/>#; s#<!-- <param name=\"rtp-end-port\" value=\"[0-9]+\"/> -->#<param name=\"rtp-end-port\" value=\"${DELONIX_RTP_MAX:-20100}\"/>#" \
    "$CONF/autoload_configs/switch.conf.xml"
fi

# 10. De onde um ramal se pode registar. O servidor responde ao directório com
#     `auth-acl=delonix_ramais` (server/src/ramais.rs); a lista não existia em
#     lado nenhum, e sem ela o FreeSWITCH recusa TODOS os registos com 403
#     («Rejected by user acl»). Por omissão, as redes privadas; em produção
#     define-se DELONIX_RAMAIS_ACL com as redes de onde os ramais falam.
ACL=""
for cidr in $(echo "${DELONIX_RAMAIS_ACL:-10.0.0.0/8,172.16.0.0/12,192.168.0.0/16}" | tr ',' ' '); do
  ACL="$ACL      <node type=\"allow\" cidr=\"$cidr\"/>\n"
done
sed -i "s#</network-lists>#    <list name=\"delonix_ramais\" default=\"deny\">\n${ACL}    </list>\n  </network-lists>#" \
  "$CONF/autoload_configs/acl.conf.xml"
grep -q 'list name="delonix_ramais"' "$CONF/autoload_configs/acl.conf.xml" ||
  { echo "não consegui definir a lista de acesso dos ramais" >&2; exit 1; }

# 11. O log sem o nível DEBUG. A esse nível o mod_curl escreve cada cabeçalho e
#     cada corpo que os scripts mandam ao servidor: o segredo de voz e o PIN de
#     quem liga (R227). DELONIX_FS_LOG_DEBUG=1 volta a ligá-lo para diagnóstico
#     — e volta a pôr os dois no log.
if [ "${DELONIX_FS_LOG_DEBUG:-0}" != 1 ]; then
  for f in logfile console; do
    sed -i -E 's#(<map name="all" value=")console,debug,#\1console,#' "$CONF/autoload_configs/$f.conf.xml"
    grep -q '<map name="all" value="console,info,' "$CONF/autoload_configs/$f.conf.xml" ||
      { echo "não consegui tirar o nível DEBUG de $f.conf.xml" >&2; exit 1; }
  done
fi

# 12. O directório de logs sem a configuração. No directório do `-log` o
#     FreeSWITCH grava também o freeswitch.xml.fsxml: a configuração já
#     expandida, com o segredo de voz lá dentro (R227). O `-log` passa a ser um
#     directório privado, ao lado da configuração, e o freeswitch.log fica por
#     caminho explícito onde sempre esteve — quem lê os logs não leva o segredo.
LOGS=/usr/local/freeswitch/var/log/freeswitch
PRIV="$CONF/.estado"
mkdir -p "$LOGS" "$PRIV"
chmod 700 "$PRIV"
sed -i "s#<!--<param name=\"logfile\" value=\"[^\"]*\"/>-->#<param name=\"logfile\" value=\"$LOGS/freeswitch.log\"/>#" \
  "$CONF/autoload_configs/logfile.conf.xml"
grep -q "<param name=\"logfile\" value=\"$LOGS/freeswitch.log\"/>" "$CONF/autoload_configs/logfile.conf.xml" ||
  { echo "não consegui fixar o caminho do freeswitch.log" >&2; exit 1; }

# -conf, -log e -db vão os três ou nenhum.
exec /usr/local/freeswitch/bin/freeswitch -conf "$CONF" \
  -log "$PRIV" -db /usr/local/freeswitch/var/lib/freeswitch/db \
  -scripts /scripts -nonat -nf -nc
