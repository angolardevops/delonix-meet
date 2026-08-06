#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════
#  Demo Kaeso × Delonix Meet — sobe/pára a stack local da apresentação.
#
#      bash deploy/demo-kaeso.sh up      # sobe tudo
#      bash deploy/demo-kaeso.sh status  # estado + URLs
#      bash deploy/demo-kaeso.sh down    # pára (dados persistem)
#
#  Topologia — e porque é esta:
#
#    browser ──► dlxmeet-web (nginx)  :8081 http / :8443 https
#                   │  serve web/dist + proxy /api,/ws,/rtc
#                   ▼
#                dlxmeet-server (Rust) :8180 ──► dlxmeet-db (postgres)
#                   ▲
#    kaeso-odoo ────┘  http://meet.kaeso.local  (via /etc/hosts do container)
#
#  Tudo vive na rede `kaeso-net` porque o engine é ROOTLESS: a bridge está
#  numa netns que o host não vê, e o slirp corre com --disable-host-loopback.
#  Um servidor no host seria inalcançável a partir do container do Odoo — foi
#  o que aconteceu na primeira tentativa.
# ═══════════════════════════════════════════════════════════════════════
set -euo pipefail

NET=kaeso-net
REPO="$(cd "$(dirname "$0")/.." && pwd)"
CFG="${DEMO_CFG:-/tmp/claude-1000/-home-walter-workspace-ngolacloud/5641d811-feac-44ff-8430-bd87e4d0371b/scratchpad/nginx}"

up() {
  delonix volumes create dlxmeet-pgdata >/dev/null 2>&1 || true

  if ! delonix container ps 2>/dev/null | grep -q dlxmeet-db; then
    delonix container run -d --name dlxmeet-db --net $NET \
      -e POSTGRES_USER=delonix -e POSTGRES_PASSWORD=delonix_dev \
      -e POSTGRES_DB=delonix_meet \
      -v dlxmeet-pgdata:/var/lib/postgresql/data postgres:17-alpine >/dev/null
  fi
  until delonix container exec dlxmeet-db pg_isready -U delonix -d delonix_meet >/dev/null 2>&1
  do sleep 2; done

  # ubuntu:24.04, não debian:12 — o binário é compilado no host contra
  # GLIBC 2.39 e o bookworm só tem 2.36 ("version GLIBC_2.39 not found").
  if ! delonix container ps 2>/dev/null | grep -q dlxmeet-server; then
    delonix container run -d --name dlxmeet-server --net $NET --knows dlxmeet-db \
      -p 8180:8180 \
      -e DELONIX_ALLOW_INSECURE=1 \
      -e DATABASE_URL=postgres://delonix:delonix_dev@dlxmeet-db:5432/delonix_meet \
      -e BIND_ADDR=0.0.0.0:8180 -e COOKIE_INSECURE=1 \
      -e PROVISIONING_SECRET=kaeso_demo_provisioning_secret \
      -e TURN_HOST=127.0.0.1:3478 \
      -e WEBHOOK_ALLOW_HOSTS=127.0.0.1,localhost,meet.kaeso.local,kaeso-odoo \
      -v "$REPO/server/target/release/delonix-server:/app/delonix-server:ro" \
      ubuntu:24.04 /app/delonix-server >/dev/null
  fi
  until curl -s -o /dev/null --max-time 2 http://127.0.0.1:8180/health; do sleep 2; done

  # O upstream do nginx é um IP, não um nome: o nginx resolve upstreams no
  # ARRANQUE e o `--knows` do delonix não lhe dá DNS. Como o IP muda sempre
  # que o dlxmeet-server é recriado, reescreve-se aqui — senão o front serve
  # 502 até alguém reparar. (Foi exactamente o que aconteceu uma vez.)
  local apiip
  apiip=$(delonix container inspect dlxmeet-server | python3 -c \
    "import sys,json;d=json.load(sys.stdin);d=d[0] if isinstance(d,list) else d;print(d.get('ip',''))")
  python3 - "$apiip" "$CFG/default.conf" <<'PY'
import re, sys
ip, path = sys.argv[1], sys.argv[2]
s = open(path).read()
new = re.sub(r'server [0-9.]+:8180;', 'server %s:8180;' % ip, s)
if new != s:
    open(path, 'w').write(new)
    print('  upstream do nginx actualizado ->', ip)
PY

  if ! delonix container ps 2>/dev/null | grep -q dlxmeet-web; then
    delonix container run -d --name dlxmeet-web --net $NET -p 8081:80 -p 8443:443 \
      -v "$REPO/web/dist:/usr/share/nginx/html:ro" \
      -v "$CFG/default.conf:/etc/nginx/conf.d/default.conf:ro" \
      -v "$CFG/certs:/etc/nginx/certs:ro" nginx:1.27-alpine >/dev/null
  else
    delonix container exec dlxmeet-web nginx -s reload >/dev/null 2>&1 || true
  fi

  # O Odoo resolve `meet.kaeso.local` para o nginx. É o mesmo nome que o
  # browser usa, o que deixa o Odoo usar UM só `delonix_base_url`.
  local webip
  webip=$(delonix container inspect dlxmeet-web | python3 -c \
    "import sys,json;d=json.load(sys.stdin);d=d[0] if isinstance(d,list) else d;print(d.get('ip',''))")
  delonix container exec kaeso-odoo sh -c \
    "grep -q meet.kaeso.local /etc/hosts || echo '$webip meet.kaeso.local' >> /etc/hosts" || true

  status
}

status() {
  printf '\n\033[1mDemo Kaeso × Delonix Meet\033[0m\n'
  delonix container ps 2>/dev/null | awk 'NR==1 || /dlxmeet|kaeso-odoo /'
  printf '\n  App  (garantido) : \033[32mhttp://localhost:8081\033[0m'
  printf '   ← localhost é contexto seguro: câmara/mic funcionam\n'
  printf '  App  (nome)      : https://meet.kaeso.local:8443'
  printf '   ← precisa da entrada em /etc/hosts\n'
  printf '  Odoo 16          : \033[32mhttp://localhost:8079\033[0m  (BD kaeso_prod_last)\n'
  printf '  API Delonix      : http://localhost:8180\n\n'
  printf '  Para o nome bonito, uma vez:\n'
  printf "    \033[33mecho '127.0.0.1 meet.kaeso.local' | sudo tee -a /etc/hosts\033[0m\n\n"
}

down() {
  delonix container stop dlxmeet-web dlxmeet-server dlxmeet-db 2>/dev/null || true
  echo "parado (os dados ficam no volume dlxmeet-pgdata)"
}

case "${1:-status}" in
  up) up ;;
  down) down ;;
  status) status ;;
  *) echo "uso: $0 {up|down|status}"; exit 1 ;;
esac
