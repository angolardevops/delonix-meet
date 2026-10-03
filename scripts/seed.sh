#!/usr/bin/env bash
# ============================================================
#  make seed — a organização e a conta de validação.
#
#  Cria (se não existir) a organização «ngolacloud» e o seu administrador,
#  pela API pública — o mesmo caminho de um cliente que se regista. Os acessos
#  estão no .env (MEET_ADMIN_PASSWORD, gerado por `make bootstrap`).
#
#  uso: scripts/seed.sh <url-base>      ex.: https://meet.ngolacloud.local:8443
# ============================================================
set -euo pipefail
cd "$(dirname "$0")/.."

BASE=${1:?uso: $0 <url-base>}
ORG=${MEET_ORG:-ngolacloud}
EMAIL=${MEET_ADMIN_EMAIL:-admin@ngolacloud.local}
PW=$(sed -n 's/^MEET_ADMIN_PASSWORD=//p' .env 2>/dev/null | head -1)
[ -n "$PW" ] || { echo "✗ o .env não tem MEET_ADMIN_PASSWORD — corre «make bootstrap»" >&2; exit 1; }

g=$'\033[1;32m'; y=$'\033[1;33m'; z=$'\033[0m'

# A borda pode demorar uns segundos a responder depois do `up`.
for _ in $(seq 1 40); do
  [ "$(curl -sk -o /dev/null -w '%{http_code}' --max-time 4 "$BASE/api/openapi.json" || true)" = 200 ] && break
  sleep 2
done

pede() { # método caminho corpo → imprime o código HTTP
  curl -sk -o /dev/null -w '%{http_code}' --max-time 15 -X "$1" "$BASE$2" \
    -H 'content-type: application/json' -d "$3" || true
}
corpo_login=$(printf '{"email":"%s","password":"%s"}' "$EMAIL" "$PW")

if [ "$(pede POST /api/auth/login "$corpo_login")" = 200 ]; then
  printf "  %s✓%s a conta %s já existe na organização «%s»\n" "$g" "$z" "$EMAIL" "$ORG"
  exit 0
fi
corpo=$(printf '{"org_name":"%s","email":"%s","username":"admin","password":"%s"}' "$ORG" "$EMAIL" "$PW")
codigo=$(pede POST /api/auth/register "$corpo")
if [ "$codigo" = 200 ] || [ "$codigo" = 201 ]; then
  printf "  %s✓%s organização «%s» e administrador %s criados\n" "$g" "$z" "$ORG" "$EMAIL"
else
  printf "  %s!%s o registo devolveu %s — a conta de validação NÃO foi criada\n" "$y" "$z" "$codigo"
  exit 1
fi
