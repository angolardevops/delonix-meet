#!/usr/bin/env bash
# ============================================================
#  make compose-voice-check — a voz do compose.yaml, medida.
#
#  As mesmas medições do cluster (scripts/cluster-voice.sh), contra os
#  contentores do compose: o tronco por allowlist (dial-in por número) e o
#  tronco da CENTRAL da organização, por TLS e autenticado (ADR-0016). Não prova
#  o telefone dentro da sala WebRTC: com o PIN certo a chamada entra na
#  conferência local do FreeSWITCH, porque a ponte para o SFU (ADR-0010) não
#  está ligada no compose.
# ============================================================
set -uo pipefail
# VOICE_CHECK_EXEC escolhe o motor quando a máquina tem os dois e o compose
# correu no outro (ex.: VOICE_CHECK_EXEC="docker exec").
if [ -n "${VOICE_CHECK_EXEC:-}" ]; then EXEC=$VOICE_CHECK_EXEC
elif command -v delonix >/dev/null 2>&1; then EXEC="delonix container exec"; else EXEC="docker exec"; fi
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
  $EXEC delonix-pbx asterisk -rx "pjsip show contacts" 2>/dev/null | grep -q "meet/sip:.*Avail" && { tronco=1; break; }
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
# ---- a central da organização (ADR-0016) ----
# O MESMO PBX, pelo seu outro tronco: por TLS, fora da allowlist (que só cobre
# a porta 5060 de origem), autenticado com a conta SIP da organização.
central=0
for _ in 1 2 3 4 5 6 7 8; do
  $EXEC delonix-pbx asterisk -rx "pjsip show contacts" 2>/dev/null | grep -q "meet-central/sip:.*Avail" && { central=1; break; }
  sleep 5
done
[ "$central" = 1 ] && ok "central → bordo: tronco por TLS alcançável, com o certificado do bordo conferido" ||
  avisa "central → bordo: o tronco por TLS NÃO responde"

sala_txt="$(dirname "$0")/../deploy/compose/generated/sala-telefone.txt"
sala=$(sed -n 's/^sala=//p' "$sala_txt" 2>/dev/null | head -1)
pin=$(sed -n 's/^pin=//p' "$sala_txt" 2>/dev/null | head -1)
autenticadas() { $EXEC delonix-kamailio kamcmd cnt.get script centrais_autenticadas 2>/dev/null | grep -oE '[0-9]+' | head -1; }
entradas() { $EXEC delonix-freeswitch sh -c "grep -acE 'conference\($1@|\[delonix ponte\] sala=$1 ' $log || true" 2>/dev/null | tail -1; }
if [ -z "$sala" ] || [ -z "$pin" ]; then
  avisa "central: sem deploy/compose/generated/sala-telefone.txt (corre «make seed») — a chamada da central fica por medir"
else
  # O PIN é a extensão marcada no contexto de prova do PBX: atende, marca-o
  # por DTMF e fica em linha.
  a0=$(autenticadas); e0=$(entradas "$sala")
  $EXEC delonix-pbx asterisk -rx "channel originate PJSIP/+244222000001@meet-central extension ${pin}@prova-pin" >/dev/null 2>&1 || true
  sleep 16
  a1=$(autenticadas); e1=$(entradas "$sala")
  [ "${a1:-0}" -gt "${a0:-0}" ] &&
    ok "central: o bordo autenticou-a com a conta SIP da organização (fora da allowlist)" ||
    avisa "central: o bordo NÃO a autenticou — o «Registo SIP» está gravado? (make seed)"
  [ "${e1:-0}" -gt "${e0:-0}" ] &&
    ok "central: o PIN da sala $sala, marcado por DTMF, abriu-a — procurada na organização da central" ||
    avisa "central: a chamada NÃO entrou na sala $sala"
  # Controlo negativo: um PIN que não é de nenhuma sala da organização.
  errado=$([ "$pin" = 000000 ] && echo 000001 || echo 000000)
  $EXEC delonix-pbx asterisk -rx "channel originate PJSIP/+244222000001@meet-central extension ${errado}@prova-pin" >/dev/null 2>&1 || true
  sleep 16
  a2=$(autenticadas); e2=$(entradas "$sala")
  [ "${a2:-0}" -gt "${a1:-0}" ] && [ "${e2:-0}" -eq "${e1:-0}" ] &&
    ok "central: com um PIN errado, autenticada no bordo e recusada pelo IVR" ||
    avisa "central: o controlo do PIN errado não se comportou como esperado (autenticadas $a1→$a2, entradas $e1→$e2)"
fi
avisa "por provar aqui: o telefone a entrar na sala WebRTC — com PIN certo entra na conferência local do FreeSWITCH, porque a ponte para o SFU não está ligada neste ambiente"
