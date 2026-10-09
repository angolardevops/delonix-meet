#!/usr/bin/env bash
# Prova no emulador: o SDK do delonix-push dentro da app, contra o servidor delonix-push REAL.
#   1. app sem Activity, serviço em primeiro plano ligado ao servidor;
#   2. chamada a entrar: o servidor entrega, a app confirma, sai a notificação de ecrã inteiro e a Activity abre;
#   3. o sistema mata o processo: o serviço renasce sozinho e a mensagem seguinte chega;
#   4. aparelho revogado: o serviço pára e não volta a ligar.
# Precisa: emulador a correr, APK debug construído, push-pg (delonix), binário do delonix-push (cargo build).
set -uo pipefail
. "$(dirname "$0")/env.sh"
PUSH_REPO="${PUSH_REPO:-$HOME/workspace/ngolacloud/delonix-push}"
PKG=ao.ngolacloud.delonixphone
APK="$(cd "$(dirname "$0")/../delonixphone" && pwd)/build/app/outputs/flutter-apk/app-debug.apk"
PORT=${PORT:-18480}; DB="push_prova_$$"
CONT="${PG_CONTAINER:-push-pg}"; PGU="${PG_USER:-delonix}"; PGP="${PG_PASSWORD:-delonix_dev}"; PGPORT="${PG_PORT:-55533}"
ok=0; ko=0
t() { if [ "$2" = ok ]; then echo "  ✓ $1"; ok=$((ok+1)); else echo "  ✗ $1  ← $3"; ko=$((ko+1)); fi; }
api() { curl -s -m 8 "$@"; }

delonix container exec "$CONT" psql -U "$PGU" -d postgres -c "CREATE DATABASE $DB" >/dev/null || exit 2
SRV=""
fim() { [ -n "$SRV" ] && kill "$SRV" 2>/dev/null; delonix container exec "$CONT" psql -U "$PGU" -d postgres -c "DROP DATABASE IF EXISTS $DB" >/dev/null 2>&1; }
trap fim EXIT
DATABASE_URL="postgres://$PGU:$PGP@127.0.0.1:$PGPORT/$DB" PUSH_BIND="0.0.0.0:$PORT" PUSH_ADMIN_TOKEN=admin-prova \
  "$PUSH_REPO/target/debug/delonix-push" >/tmp/delonix-push-prova.log 2>&1 &
SRV=$!
for _ in $(seq 50); do [ "$(api -o /dev/null -w '%{http_code}' localhost:$PORT/healthz)" = 200 ] && break; sleep 0.2; done
KEY=$(api -H 'x-admin-token: admin-prova' -H 'content-type: application/json' -d '{"name":"prova"}' localhost:$PORT/admin/v1/projects | python3 -c 'import sys,json;print(json.load(sys.stdin)["server_key"])')
DEV=$(api -H "authorization: Bearer $KEY" -H 'content-type: application/json' -d '{"platform":"android"}' localhost:$PORT/v1/devices)
DID=$(echo "$DEV" | python3 -c 'import sys,json;print(json.load(sys.stdin)["device_id"])'); SEC=$(echo "$DEV" | python3 -c 'import sys,json;print(json.load(sys.stdin)["device_secret"])')
estado() { api -H "authorization: Bearer $KEY" localhost:$PORT/v1/messages/$1 | python3 -c 'import sys,json;print(json.load(sys.stdin).get("state","?"))'; }
enviar() { api -H "authorization: Bearer $KEY" -H 'content-type: application/json' \
  -d "{\"device_id\":\"$DID\",\"priority\":\"high\",\"payload\":{\"kind\":\"incoming_call\",\"call_uuid\":\"$1\",\"caller\":\"1902\"}}" localhost:$PORT/v1/messages \
  | python3 -c 'import sys,json;print(json.load(sys.stdin)["message_ids"][0])'; }
espera_estado() { for _ in $(seq 40); do [ "$(estado "$1")" = "$2" ] && return 0; sleep 0.5; done; return 1; }
servico_vivo() { adb shell dumpsys activity services $PKG 2>/dev/null | grep -q "PushService"; }

echo "== instalar e configurar =="
adb install -r -g "$APK" >/dev/null 2>&1 || { echo "falhou a instalação de $APK"; exit 2; }
adb shell pm grant $PKG android.permission.POST_NOTIFICATIONS 2>/dev/null
adb shell appops set $PKG USE_FULL_SCREEN_INTENT allow 2>/dev/null
adb shell am force-stop $PKG; adb logcat -c
# O Android 12+ só deixa arrancar o serviço em primeiro plano com a app visível (é o fluxo real: a pessoa
# abre a app e entra). Depois fecha-se a Activity e fica só o serviço.
adb shell input keyevent KEYCODE_WAKEUP; adb shell wm dismiss-keyguard
adb shell am start -n $PKG/.MainActivity >/dev/null; sleep 5
adb shell am broadcast -n $PKG/.PushConfigReceiver -a ao.ngolacloud.delonixphone.PUSH_CONFIGURAR \
  --es url "http://10.0.2.2:$PORT" --es segredo "$SEC" >/dev/null
adb shell input keyevent KEYCODE_BACK; sleep 2
for _ in $(seq 30); do servico_vivo && break; sleep 0.5; done
servico_vivo && t "serviço em primeiro plano a correr, sem Activity" ok || t "serviço em primeiro plano a correr, sem Activity" ko "PushService não aparece"
adb shell dumpsys activity services $PKG | grep -q "isForeground=true" && t "é mesmo de primeiro plano (notificação permanente)" ok || t "é mesmo de primeiro plano" ko "isForeground=false"

echo "== chamada a entrar com a app sem Activity e o ecrã desligado =="
adb shell input keyevent KEYCODE_SLEEP; sleep 1
M1=$(enviar 11111111-0000-0000-0000-00000000aaaa)
espera_estado "$M1" delivered && t "o servidor real viu o ack da app" ok || t "o servidor real viu o ack da app" ko "estado=$(estado "$M1")"
sleep 2
adb logcat -d | grep -q "chamada a entrar por push.*call=11111111-0000-0000-0000-00000000aaaa caller=1902" && t "o handler recebeu o call_uuid e o número curto" ok || t "o handler recebeu o call_uuid e o número curto" ko "sem linha no logcat"
adb shell dumpsys notification --noredact | grep -q "pkg=$PKG.*channel=chamadas" && t "a notificação de chamada (canal «chamadas») foi publicada" ok || t "a notificação de chamada foi publicada" ko "sem canal chamadas no dumpsys"
adb shell dumpsys activity activities | grep -E "topResumedActivity|mResumedActivity" | grep -q "$PKG/.MainActivity" && t "a Activity abriu com o ecrã desligado (só o ecrã inteiro o permite a um serviço)" ok || t "a Activity da app abriu" ko "$(adb shell dumpsys activity activities | grep -E 'topResumedActivity' | head -1)"

echo "== o sistema mata o processo =="
adb shell input keyevent KEYCODE_WAKEUP; adb shell input keyevent KEYCODE_HOME
PID=$(adb shell pidof $PKG | tr -d '\r' | awk '{print $1}')
adb shell run-as $PKG kill -9 "$PID" 2>/dev/null
sleep 1
for _ in $(seq 40); do NP=$(adb shell pidof $PKG | tr -d '\r' | awk '{print $1}'); [ -n "$NP" ] && [ "$NP" != "$PID" ] && break; sleep 1; done
[ -n "${NP:-}" ] && [ "$NP" != "$PID" ] && t "o serviço renasceu sozinho (START_STICKY), pid $PID → $NP" ok || t "o serviço renasceu sozinho" ko "sem processo novo"
sleep 4
M2=$(enviar 22222222-0000-0000-0000-00000000bbbb)
espera_estado "$M2" delivered && t "depois de renascer, a mensagem seguinte chega e é confirmada" ok || t "depois de renascer, a mensagem seguinte chega" ko "estado=$(estado "$M2")"

echo "== aparelho revogado =="
api -X DELETE -H "authorization: Bearer $KEY" localhost:$PORT/v1/devices/$DID -o /dev/null
adb shell run-as $PKG kill -9 "$(adb shell pidof $PKG | tr -d '\r' | awk '{print $1}')" 2>/dev/null
sleep 12
adb shell dumpsys activity services $PKG | grep -q "isForeground=true" && t "o serviço pára com o segredo revogado" ko "continua em primeiro plano" || t "o serviço pára com o segredo revogado" ok

echo; echo "passou $ok, falhou $ko"; [ "$ko" = 0 ]
