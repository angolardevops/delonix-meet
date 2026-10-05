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
# 4. ESL: em loopback com password aleatória, a menos que o ambiente traga
#    TELEPHONY_ESL_PASSWORD — então o servidor do Meet liga-se por rede (originar
#    chamadas, estado do registo) e o ESL escuta em todas as interfaces, MAS com
#    ACL: só as redes privadas entram — ou só as de DELONIX_ESL_CIDRS, se vier
#    — e mesmo assim só com a password. O ESL
#    manda originar chamadas — quem o alcança com a password gasta dinheiro de
#    operadora; por isso nunca se publica no host e a password vem do segredo.
if [ -n "${TELEPHONY_ESL_PASSWORD:-}" ]; then
  ESL=$TELEPHONY_ESL_PASSWORD
  [ ${#ESL} -ge 32 ] || { echo "TELEPHONY_ESL_PASSWORD com menos de 32 caracteres" >&2; exit 1; }
  case $ESL in *[!A-Za-z0-9._-]*) echo "TELEPHONY_ESL_PASSWORD com caracteres que o XML não aceita" >&2; exit 1;; esac
  ESL_LISTEN=0.0.0.0
  # DELONIX_ESL_CIDRS (opcional, separadas por vírgula): de onde o SERVIDOR
  # fala. Sem ela entram todas as redes privadas — num compose ou num cluster
  # isso é toda a gente, e a password é a única barreira. Com ela, só essas
  # redes (e loopback, para o `fs_cli` deste contentor): quem não é o servidor
  # nem com a password certa entra (R300).
  ESL_NODES='      <node type="allow" cidr="10.0.0.0/8"/>\n      <node type="allow" cidr="172.16.0.0/12"/>\n      <node type="allow" cidr="192.168.0.0/16"/>\n'
  if [ -n "${DELONIX_ESL_CIDRS:-}" ]; then
    ESL_NODES=
    set -f   # a lista parte-se por espaços; um `*` nela não é para expandir em nomes de ficheiros
    for cidr in $(echo "$DELONIX_ESL_CIDRS" | tr ',' ' '); do
      case "$cidr" in
        */*/*|*[!0-9a-fA-F.:/]*) echo "DELONIX_ESL_CIDRS: $cidr não é um endereço" >&2; exit 1 ;;
      esac
      # A máscara: só dígitos, sem zero à esquerda, e nunca vazia. O FreeSWITCH
      # lê-a com `atoi` (switch_parse_cidr): vazia, `00` ou com letras dá 0
      # bits, que casa com TUDO — a lista «estreita» ficava aberta a toda a
      # gente. Um valor sem `/` é ignorado por ele: aqui recusa-se.
      case "$cidr" in */*) m=${cidr##*/} ;; *) m= ;; esac
      case "$m" in
        ''|0*|*[!0-9]*) echo "DELONIX_ESL_CIDRS: $cidr não tem uma máscara válida (ou abre o ESL a toda a gente)" >&2; exit 1 ;;
      esac
      ESL_NODES="$ESL_NODES      <node type=\"allow\" cidr=\"$cidr\"/>\n"
    done
    set +f
    [ -n "$ESL_NODES" ] || { echo "DELONIX_ESL_CIDRS sem nenhuma rede" >&2; exit 1; }
  fi
  sed -i "s#</network-lists>#  <list name=\"delonix_esl\" default=\"deny\">\n${ESL_NODES}      <node type=\"allow\" cidr=\"127.0.0.0/8\"/>\n    </list>\n  </network-lists>#" \
    "$CONF/autoload_configs/acl.conf.xml"
  grep -q 'name="delonix_esl"' "$CONF/autoload_configs/acl.conf.xml" ||
    { echo "a ACL do ESL não ficou escrita" >&2; exit 1; }
  ESL_ACL='<param name="apply-inbound-acl" value="delonix_esl"/>'
else
  ESL=$(head -c 18 /dev/urandom | od -An -tx1 | tr -d ' \n')
  ESL_LISTEN=127.0.0.1
  ESL_ACL=
fi
cat >"$CONF/autoload_configs/event_socket.conf.xml" <<XML
<configuration name="event_socket.conf" description="Socket Client">
  <settings>
    <param name="nat-map" value="false"/>
    <param name="listen-ip" value="${ESL_LISTEN}"/>
    <param name="listen-port" value="8021"/>
    <param name="password" value="${ESL}"/>
    ${ESL_ACL}
  </settings>
</configuration>
XML
# 5. Os módulos de que o IVR e os ramais precisam (a vanilla não carrega o mod_curl).
#    Na vanilla estão comentados: descomenta-se a linha, não se acrescenta outra.
for m in mod_curl mod_xml_curl; do
  sed -i "s#<!-- <load module=\"$m\"/> -->#<load module=\"$m\"/>#" "$CONF/autoload_configs/modules.conf.xml"
done
#    O mod_json_cdr (os registos de chamada da telefonia, passo 7b) nem
#    comentado lá está: acrescenta-se.
sed -i 's#</modules>#  <load module="mod_json_cdr"/>\n  </modules>#' "$CONF/autoload_configs/modules.conf.xml"
for m in mod_curl mod_lua mod_xml_curl mod_json_cdr mod_hash; do
  grep -q "^[[:space:]]*<load module=\"$m\"/>" "$CONF/autoload_configs/modules.conf.xml" ||
    { echo "o módulo $m não ficou carregado" >&2; exit 1; }
done

# 6. As variáveis do Meet vêm do ambiente do pod (Secret e ConfigMap).
#    Os registos de chamada que o servidor não aceitou ficam FORA de /conf,
#    que é esvaziado a cada arranque. Sobrevivem a reiniciar o contentor
#    (compose); num pod, um reinício pelo kubelet é um contentor NOVO e
#    leva-os — não há volume para eles.
CDR_PENDENTES=/usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes
mkdir -p "$CDR_PENDENTES"
chmod 700 "$CDR_PENDENTES"
# `outbound_redirect_fatal`, global: nenhuma perna que o FreeSWITCH origina
# segue um 3xx — nem a do tronco, nem a da chamada de teste pelo ESL, nem a
# que toca num ramal. Segui-lo era ligar ao Contact que o outro lado escolhe,
# sem passar pela guarda de saída (R213). Uma variável de canal que não exista
# lê-se das globais (switch_channel.c).
cat >"$CONF/vars-meet.xml" <<XML
<include>
  <X-PRE-PROCESS cmd="set" data="delonix_control_url=${DELONIX_CONTROL_URL}"/>
  <X-PRE-PROCESS cmd="set" data="delonix_voice_secret=${VOICE_INTERNAL_SECRET}"/>
  <X-PRE-PROCESS cmd="set" data="delonix_ramais_sip_port=${DELONIX_RAMAIS_SIP_PORT:-5070}"/>
  <X-PRE-PROCESS cmd="set" data="delonix_cdr_dir=${CDR_PENDENTES}"/>
  <X-PRE-PROCESS cmd="set" data="rtp_secure_media=mandatory"/>
  <X-PRE-PROCESS cmd="set" data="outbound_redirect_fatal=true"/>
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
cp "$MEET/conference.conf.xml" "$MEET/xml_curl.conf.xml" "$MEET/json_cdr.conf.xml" "$CONF/autoload_configs/"
cp "$MEET/internal.xml" "$CONF/sip_profiles/"
mkdir -p /scripts
cp "$MEET/dialin_ivr.lua" "$MEET/ramais_dial.lua" /scripts/

# 7b. A telefonia de troncos (ADR-0009). Os troncos que as organizações criam
#     na consola são gateways do perfil `external` — o mesmo por onde o bordo
#     entrega as chamadas (5080) e o que o servidor nomeia por omissão
#     (TELEPHONY_SOFIA_PROFILE). Na vanilla o perfil procura gateways em
#     TODOS os domínios do directório (`<domain name="all" parse="true"/>`);
#     passa a procurá-los num só, `delonix-trunks`, que é o que o servidor
#     serve pelo mod_xml_curl (xml_curl.conf.xml, binding `delonix_telefonia`).
#     Troca-se, não se acrescenta: com os dois a lista era lida duas vezes.
#     Sem troncos na base a resposta é um domínio vazio, e nada muda.
sed -i 's#<domain name="all" alias="false" parse="true"/>#<domain name="delonix-trunks" alias="false" parse="true"/>#' \
  "$CONF/sip_profiles/external.xml"
grep -q '<domain name="delonix-trunks" alias="false" parse="true"/>' "$CONF/sip_profiles/external.xml" ||
  { echo "não consegui pôr o domínio dos troncos no perfil external" >&2; exit 1; }
#     Sem transferências (REFER) no perfil dos troncos: um REFER vindo do
#     lado da operadora punha a perna de quem marcou a passar outra vez pelo
#     plano de marcação, com a organização dela e para onde a operadora
#     mandasse (R292). O dos ramais tem a mesma regra, em internal.xml.
sed -i 's#<settings>#&\n    <param name="disable-transfer" value="true"/>#' "$CONF/sip_profiles/external.xml"
grep -q '<param name="disable-transfer" value="true"/>' "$CONF/sip_profiles/external.xml" ||
  { echo "não consegui desligar as transferências no perfil external" >&2; exit 1; }

# 8. Um só endereço do servidor: tudo o que o FreeSWITCH lhe pede — o IVR do
#    dial-in, o directório e o dialplan dos ramais (mod_xml_curl) e o
#    ramais_dial.lua — vive no listener INTERNO (/internal/v1/voice/ivr/*,
#    DELONIX_CONTROL_URL). Até à R286 os ramais pediam /api/voice/ivr/* ao
#    listener público e este passo reescrevia as cópias para o apontar.

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

# 10b. De onde fala o BORDO (ADR-0016). O IVR só acredita no cabeçalho
#      `X-Delonix-Central` — «esta chamada é da central da organização X» — se
#      a chamada veio de um endereço desta lista. Sem DELONIX_EDGE_CIDRS a
#      lista fica vazia e nenhuma chamada entra como central: fecha por omissão.
BORDO=""
for cidr in $(echo "${DELONIX_EDGE_CIDRS:-}" | tr ',' ' '); do
  BORDO="$BORDO      <node type=\"allow\" cidr=\"$cidr\"/>\n"
done
sed -i "s#</network-lists>#    <list name=\"delonix_bordo\" default=\"deny\">\n${BORDO}    </list>\n  </network-lists>#" \
  "$CONF/autoload_configs/acl.conf.xml"
grep -q 'list name="delonix_bordo"' "$CONF/autoload_configs/acl.conf.xml" ||
  { echo "não consegui definir a lista de endereços do bordo" >&2; exit 1; }

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

# 13. Os troncos voltam a ler-se sozinhos. O FreeSWITCH só pergunta pelos
#     gateways quando o perfil arranca: se o servidor ainda não respondia
#     nesse instante ficava sem tronco nenhum, e um tronco criado depois na
#     consola só aparecia reiniciando-o. O `rescan` volta a perguntar e
#     ACRESCENTA os gateways que faltam; não mexe nos que já existem nem nas
#     chamadas em curso. Não é de graça: volta a aplicar as definições ao
#     perfil vivo e zera os contadores de chamadas dele (sofia.c), por isso o
#     `sofia status profile external` passa a contar desde o último ciclo.
#     Um tronco ALTERADO ou APAGADO não é com ele: precisa de `killgw`, que
#     hoje só o servidor sabe mandar pelo ESL — com TELEPHONY_ESL_PASSWORD o
#     servidor chega-lhe (passo 4); sem ela o ESL fica em loopback. DELONIX_TRUNKS_RESCAN_SECS=0 desliga o ciclo.
RESCAN=${DELONIX_TRUNKS_RESCAN_SECS:-60}
case "$RESCAN" in ''|*[!0-9]*) echo "DELONIX_TRUNKS_RESCAN_SECS não é um número de segundos: $RESCAN" >&2; exit 1 ;; esac
if [ "$RESCAN" -gt 0 ]; then
  ( while sleep "$RESCAN"; do
      /usr/local/freeswitch/bin/fs_cli -T 3000 -t 10000 -p "$ESL" -x "sofia profile external rescan" >/dev/null 2>&1 || true
    done ) &
fi

# -conf, -log e -db vão os três ou nenhum.
exec /usr/local/freeswitch/bin/freeswitch -conf "$CONF" \
  -log "$PRIV" -db /usr/local/freeswitch/var/lib/freeswitch/db \
  -scripts /scripts -nonat -nf -nc
