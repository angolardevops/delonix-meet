#!/usr/bin/env bash
# ============================================================
#  make compose-voice-check — a voz do compose.yaml, medida.
#
#  As mesmas quatro medições do cluster (scripts/cluster-voice.sh), contra os
#  contentores do compose. Não prova o telefone dentro da sala WebRTC: com o
#  PIN certo a chamada entra na conferência local do FreeSWITCH, porque a ponte
#  para o SFU (ADR-0010) não está ligada no compose.
# ============================================================
set -uo pipefail
if command -v delonix >/dev/null 2>&1; then EXEC="delonix container exec"; else EXEC="docker exec"; fi
g=$'\033[1;32m'; y=$'\033[1;33m'; z=$'\033[0m'
ok() { printf "  %s✓%s %s\n" "$g" "$z" "$1"; }
avisa() { printf "  %s!%s %s\n" "$y" "$z" "$1"; }
fs() { $EXEC delonix-freeswitch sh -c 'P=$(sed -n "s/.*name=\"password\" value=\"\([^\"]*\)\".*/\1/p" /conf/autoload_configs/event_socket.conf.xml); /usr/local/freeswitch/bin/fs_cli -p "$P" -x "'"$1"'"' 2>/dev/null; }

if $EXEC delonix-kamailio kamcmd dispatcher.list 2>/dev/null | grep -q "FLAGS: AP"; then
  ok "bordo → FreeSWITCH: activo no dispatcher (OPTIONS respondido)"
else
  avisa "bordo → FreeSWITCH: o dispatcher NÃO vê o FreeSWITCH activo"
fi

tronco=0
for _ in 1 2 3 4 5 6 7 8; do
  $EXEC delonix-pbx asterisk -rx "pjsip show contacts" 2>/dev/null | grep -q "Avail" && { tronco=1; break; }
  sleep 5
done
[ "$tronco" = 1 ] && ok "PBX de cliente → bordo: tronco alcançável (OPTIONS respondido)" ||
  avisa "PBX de cliente → bordo: o tronco NÃO responde"

log=/usr/local/freeswitch/var/log/freeswitch/freeswitch.log
conta() { $EXEC delonix-freeswitch sh -c "grep -ac 'lua(dialin_ivr.lua)' $log || true" 2>/dev/null | tail -1; }
antes=$(conta)
$EXEC delonix-pbx asterisk -rx "channel originate PJSIP/+244222000001@meet application Wait 4" >/dev/null 2>&1 || true
sleep 6
depois=$(conta)
[ "${depois:-0}" -gt "${antes:-0}" ] &&
  ok "chamada de prova: PBX → bordo → FreeSWITCH → IVR do Meet (dialin_ivr.lua correu)" ||
  avisa "chamada de prova: NÃO chegou ao IVR do Meet"

fs "curl http://delonix-server:8181/internal/v1/voice/ivr/validate post {}" | grep -q '"code":"unsupported_media_type"\|"code":"auth' &&
  ok "FreeSWITCH → servidor (listener interno): responde" ||
  avisa "FreeSWITCH → servidor (listener interno): NÃO responde"
fs "curl http://delonix-server:8180/api/voice/ivr/dialplan-did post {}" | grep -q '"code":"auth' &&
  ok "FreeSWITCH → servidor (listener público): responde e exige o segredo" ||
  avisa "FreeSWITCH → servidor (listener público): NÃO responde"
avisa "por provar aqui: o telefone a entrar na sala WebRTC — com PIN certo entra na conferência local do FreeSWITCH, porque a ponte para o SFU não está ligada neste ambiente"
