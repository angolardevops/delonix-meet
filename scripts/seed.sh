#!/usr/bin/env bash
# ============================================================
#  make seed — a organização e a conta de validação.
#
#  Cria (se não existir) a organização «ngolacloud» e o seu administrador,
#  pela API pública — o mesmo caminho de um cliente que se regista. Os acessos
#  estão no .env (MEET_ADMIN_PASSWORD, gerado por `make bootstrap`).
#
#  E a voz dessa organização, também pela API, como o administrador a faria:
#  o «Registo SIP» (a conta com que a central da organização entra no bordo,
#  ADR-0016), um número de acesso e uma sala com PIN para entrar por telefone
#  — escrita em deploy/compose/generated/sala-telefone.txt.
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
else
  corpo=$(printf '{"org_name":"%s","email":"%s","username":"admin","password":"%s"}' "$ORG" "$EMAIL" "$PW")
  codigo=$(pede POST /api/auth/register "$corpo")
  if [ "$codigo" = 200 ] || [ "$codigo" = 201 ]; then
    printf "  %s✓%s organização «%s» e administrador %s criados\n" "$g" "$z" "$ORG" "$EMAIL"
  else
    printf "  %s!%s o registo devolveu %s — a conta de validação NÃO foi criada\n" "$y" "$z" "$codigo"
    exit 1
  fi
fi

# ---- a voz da organização: a conta SIP da central e uma sala com PIN ----
# Falhar aqui não desfaz a conta de validação: avisa-se e segue-se.
CENTRAL_PW=$(sed -n 's/^VOICE_CENTRAL_PASSWORD=//p' .env 2>/dev/null | head -1)
SALA_TXT=${SALA_TXT:-deploy/compose/generated/sala-telefone.txt}
if [ -z "$CENTRAL_PW" ]; then
  printf "  %s!%s o .env não tem VOICE_CENTRAL_PASSWORD — corre «make bootstrap»; a central fica sem conta SIP\n" "$y" "$z"
  exit 0
fi
command -v python3 >/dev/null 2>&1 || {
  printf "  %s!%s sem python3: a conta SIP da central e a sala com PIN ficam por criar\n" "$y" "$z"
  exit 0
}
mkdir -p "$(dirname "$SALA_TXT")"
BASE="$BASE" EMAIL="$EMAIL" PW="$PW" CENTRAL_PW="$CENTRAL_PW" SALA_TXT="$SALA_TXT" \
  CENTRAL_DOMAIN="${VOICE_CENTRAL_DOMAIN:-pbx.ngolacloud.local}" DID="${VOICE_ACCESS_NUMBER:-+244222000001}" \
  python3 - <<'PY' || printf "  %s!%s a voz da organização NÃO ficou semeada (ver acima)\n" "$y" "$z"
import json, os, ssl, sys, urllib.error, urllib.request

base, sala_txt = os.environ["BASE"], os.environ["SALA_TXT"]
ctx = ssl.create_default_context()
ctx.check_hostname = False
ctx.verify_mode = ssl.CERT_NONE  # o certificado do laboratório é self-signed


def call(method, path, body=None, token=None):
    req = urllib.request.Request(base + path, method=method,
                                 data=json.dumps(body).encode() if body is not None else None)
    req.add_header("content-type", "application/json")
    if token:
        req.add_header("authorization", "Bearer " + token)
    try:
        with urllib.request.urlopen(req, timeout=20, context=ctx) as r:
            t = r.read().decode()
            return r.status, (json.loads(t) if t else None)
    except urllib.error.HTTPError as e:
        t = e.read().decode()
        try:
            return e.code, json.loads(t)
        except ValueError:
            return e.code, None


def ok(msg):
    print("  \033[1;32m✓\033[0m " + msg)


st, login = call("POST", "/api/auth/login", {"email": os.environ["EMAIL"], "password": os.environ["PW"]})
if st != 200:
    sys.exit(f"  login devolveu {st}")
tok = login["access_token"]
st, orgs = call("GET", "/api/orgs", token=tok)
if st != 200 or not orgs:
    sys.exit(f"  GET /api/orgs devolveu {st}")
org = orgs[0]["id"]

# O «Registo SIP»: a conta com que a central da organização se autentica no bordo.
dominio = os.environ["CENTRAL_DOMAIN"]
st, r = call("PUT", f"/api/orgs/{org}/telephony/sip-settings",
             {"domain": dominio, "transport": "tls", "srtp": "mandatory",
              "username": "central", "password": os.environ["CENTRAL_PW"]}, tok)
if st != 200:
    sys.exit(f"  o «Registo SIP» devolveu {st}: {(r or {}).get('code')}")
ok(f"«Registo SIP» da organização: {dominio}, utilizador central")

# Uma sala com PIN para entrar por telefone. Se o ficheiro já a descreve, fica.
if os.path.exists(sala_txt):
    ok("sala com PIN: a de " + sala_txt)
    sys.exit(0)
did = os.environ["DID"]
st, r = call("POST", f"/api/orgs/{org}/voice/dids", {"e164": did, "org_scoped": True}, tok)
if st not in (200, 201, 409):
    sys.exit(f"  o número de acesso devolveu {st}: {(r or {}).get('code')}")
st, sala = call("POST", "/api/rooms", {"name": "Sala com telefone", "topology": "sfu"}, tok)
if st != 200:
    sys.exit(f"  criar a sala devolveu {st}")
st, voz = call("POST", f"/api/orgs/{org}/voice/rooms", {"room_code": sala["code"]}, tok)
if st != 200:
    sys.exit(f"  criar a sala de voz devolveu {st}: {(voz or {}).get('code')}")
fd = os.open(sala_txt, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    f.write(f"sala={sala['code']}\npin={voz['pin']}\nnumero={did}\n")
ok(f"sala com PIN: {sala['code']} (em {sala_txt})")
PY
