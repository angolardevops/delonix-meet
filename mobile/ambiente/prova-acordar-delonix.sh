#!/usr/bin/env bash
# A cadeia completa com o delonix-push REAL (não o fornecedor «lab» de papel):
#   FreeSWITCH real → Meet real → delonix-push real → SDK na app → motor SIP → chamada a tocar.
# Passos: base e servidor do delonix-push; laboratório com PUSH_DELONIX_*; APK com a raiz do laboratório; a prova.
# Variáveis: LAB (worktree do laboratório), LAN_IP, PUSH_REPO, LAB_DB_NAME (outra base no mesmo Postgres: não apaga a existente). Não destrói nada; deixa o delonix-push a correr
# (PID em /tmp/delonix-push-lab.pid) e a pessoa de prova no laboratório.
set -euo pipefail
. "$(dirname "$0")/env.sh"
AQUI="$(cd "$(dirname "$0")" && pwd)"
LAB="${LAB:-$HOME/workspace/ngolacloud/.worktrees/delonix-meet/laboratorio-develop}"
LAN_IP="${LAN_IP:-$(ip -4 route get 1.1.1.1 | grep -oP 'src \K[\d.]+')}"
PUSH_REPO="${PUSH_REPO:-$HOME/workspace/ngolacloud/delonix-push}"
PORT=18480; DB=push_lab
CONT="${PG_CONTAINER:-push-pg}"; PGU="${PG_USER:-delonix}"; PGP="${PG_PASSWORD:-delonix_dev}"; PGPORT="${PG_PORT:-55533}"
DBURL="postgres://$PGU:$PGP@127.0.0.1:$PGPORT/$DB"

echo "▶ delonix-push em $LAN_IP:$PORT"
# O servidor anterior tem de sair antes: uma base com ligações abertas não se apaga.
[ -f /tmp/delonix-push-lab.pid ] && kill "$(cat /tmp/delonix-push-lab.pid)" 2>/dev/null || true
sleep 1
delonix container exec "$CONT" psql -U "$PGU" -d postgres -c "DROP DATABASE IF EXISTS $DB" >/dev/null
delonix container exec "$CONT" psql -U "$PGU" -d postgres -c "CREATE DATABASE $DB" >/dev/null
DATABASE_URL="$DBURL" PUSH_BIND="0.0.0.0:$PORT" PUSH_ADMIN_TOKEN=admin-lab \
  nohup "$PUSH_REPO/target/debug/delonix-push" >/tmp/delonix-push-lab.log 2>&1 &
echo $! > /tmp/delonix-push-lab.pid
for _ in $(seq 50); do [ "$(curl -s -o /dev/null -w '%{http_code}' localhost:$PORT/healthz)" = 200 ] && break; sleep 0.2; done
KEY=$(curl -s -H 'x-admin-token: admin-lab' -H 'content-type: application/json' -d '{"name":"laboratorio"}' localhost:$PORT/admin/v1/projects | python3 -c 'import sys,json;print(json.load(sys.stdin)["server_key"])')

echo "▶ laboratório com o fornecedor delonix"
cd "$LAB"
make compose-down >/dev/null 2>&1 || true
LAB_DB_NAME="${LAB_DB_NAME:-delonix_meet_push}" PUSH_DELONIX_URL="http://$LAN_IP:$PORT" PUSH_DELONIX_KEY="$KEY" make compose-up LAN_IP="$LAN_IP" >/tmp/lab-up.log 2>&1 || { tail -20 /tmp/lab-up.log; exit 2; }
for _ in $(seq 90); do curl -sk -o /dev/null -w '%{http_code}' "https://$LAN_IP:8443/api/health" 2>/dev/null | grep 200 >/dev/null && break; sleep 2; done

make seed BASE="https://$LAN_IP:8443" >/tmp/lab-seed.log 2>&1 || { tail -15 /tmp/lab-seed.log; exit 2; }

echo "▶ APK com a raiz do laboratório"
cd "$AQUI/../delonixphone"
flutter build apk --debug --dart-define=LAB_CA_B64="$(base64 -w0 "$LAB/deploy/compose/generated/lan-tls/ca.crt")" >/tmp/dp-build.log 2>&1 || { tail -20 /tmp/dp-build.log; exit 2; }
adb install -r -g build/app/outputs/flutter-apk/app-debug.apk >/dev/null
adb shell pm grant ao.ngolacloud.delonixphone android.permission.POST_NOTIFICATIONS 2>/dev/null || true
adb shell appops set ao.ngolacloud.delonixphone USE_FULL_SCREEN_INTENT allow 2>/dev/null || true

echo "▶ a prova"
python3 -I "$AQUI/prova-acordar-delonix.py" --lab "$LAB" --push-url "http://$LAN_IP:$PORT" --push-key "$KEY" --push-db "$DBURL"
