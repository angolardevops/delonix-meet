#!/usr/bin/env bash
# ============================================================
#  Integração: o ai-worker contra o delonix-server REAL, por gRPC.
#
#  Prova, com Postgres e servidor verdadeiros (sem GPU — TRANSCRIBER=fake):
#   1. texto claro (GRPC_INSECURE=1 ↔ DELONIX_ALLOW_INSECURE=1): o worker
#      reserva, entrega, e a transcrição chega à base JÁ censurada pelo DLP;
#   2. ficheiro em falta → FailJob(retryable=false) → a gravação sai da fila
#      (transcription_failed_at + transcription_error);
#   3. mTLS (certificados openssl): o worker com certificado da CA interna
#      entrega; um worker com certificado de OUTRA CA é recusado no handshake;
#      um worker em texto claro contra o listener mTLS também.
#
#  Pré-requisitos:
#   - venv com grpcio/grpcio-tools/protobuf (requirements.txt/-build.txt):
#     VENV=~/.cache/delonix-ai-worker-venv (omissão)
#   - servidor compilado: (cd server && cargo build --release)
#   - Postgres acessível com um utilizador que possa CREATE DATABASE
#     (omissão: o do docker-compose de dev, localhost:5435).
#
#  Uso (da raiz do repo):  bash ai-worker/tests/it_grpc.sh
#  Estado de trabalho em .cache/ai-worker-it/ (ignorado pelo git); a base de
#  teste é criada e APAGADA no fim, passe ou falhe.
# ============================================================
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

VENV="${VENV:-$HOME/.cache/delonix-ai-worker-venv}"
PY="$VENV/bin/python"
SERVER_BIN="${SERVER_BIN:-$ROOT/server/target/release/delonix-server}"
PG_ADMIN_URL="${PG_ADMIN_URL:-postgres://delonix:delonix_dev@localhost:5435/postgres}"
DB_NAME="${DB_NAME:-meet_aiworker_it}"
DB_URL="${PG_ADMIN_URL%/*}/$DB_NAME"
HTTP_PORT="${HTTP_PORT:-18180}"
GRPC_PORT="${GRPC_PORT:-19180}"
WORK="$ROOT/.cache/ai-worker-it"
REC_DIR="$WORK/recordings"
PKI="$WORK/pki"
SECRET="sk-$(printf 'abcdefghijklmnopqrstuvwxyzabcdef')"

ok() { echo "  ✓ $*"; }
die() { echo "✗ $*" >&2; [ -f "$WORK/server.log" ] && tail -20 "$WORK/server.log" >&2; exit 1; }

[ -x "$PY" ] || die "falta o venv $VENV (grpcio grpcio-tools protobuf)"
[ -x "$SERVER_BIN" ] || die "falta $SERVER_BIN — (cd server && cargo build --release)"
command -v psql >/dev/null || PSQL_DOCKER="${PSQL_DOCKER:-wt-merge-postgres-1}"
command -v openssl >/dev/null || die "falta openssl"

psql_admin() {
  if [ -n "${PSQL_DOCKER:-}" ]; then
    docker exec -i "$PSQL_DOCKER" psql -v ON_ERROR_STOP=1 -qAt -U delonix -d postgres "$@"
  else psql -v ON_ERROR_STOP=1 -qAt "$PG_ADMIN_URL" "$@"; fi
}
psql_it() {
  if [ -n "${PSQL_DOCKER:-}" ]; then
    docker exec -i "$PSQL_DOCKER" psql -v ON_ERROR_STOP=1 -qAt -U delonix -d "$DB_NAME" "$@"
  else psql -v ON_ERROR_STOP=1 -qAt "$DB_URL" "$@"; fi
}

SERVER_PID=""
stop_server() {
  if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID"; wait "$SERVER_PID" 2>/dev/null || true
  fi
  SERVER_PID=""
}
cleanup() {
  stop_server
  psql_admin -c "DROP DATABASE IF EXISTS $DB_NAME WITH (FORCE)" >/dev/null 2>&1 || true
}
trap cleanup EXIT

rm -rf "$WORK" && mkdir -p "$REC_DIR" "$PKI"
psql_admin -c "DROP DATABASE IF EXISTS $DB_NAME WITH (FORCE)" >/dev/null
psql_admin -c "CREATE DATABASE $DB_NAME" >/dev/null
ok "base $DB_NAME criada"

PYTHON="$PY" bash ai-worker/gen_protos.sh server/proto "$WORK/gen" >/dev/null
ok "stubs gerados de server/proto"

# $1: extra env (ex.: GRPC_TLS_*), em forma de palavras VAR=valor
start_server() {
  env DATABASE_URL="$DB_URL" DELONIX_ALLOW_INSECURE=1 REGISTRATION_MODE=open \
      BIND_ADDR="127.0.0.1:$HTTP_PORT" GRPC_BIND_ADDR="127.0.0.1:$GRPC_PORT" \
      RECORDINGS_DIR="$REC_DIR" "$@" "$SERVER_BIN" >>"$WORK/server.log" 2>&1 &
  SERVER_PID=$!
  for _ in $(seq 1 120); do
    curl -fs "http://127.0.0.1:$HTTP_PORT/health" >/dev/null 2>&1 && \
      (exec 3<>"/dev/tcp/127.0.0.1/$GRPC_PORT") 2>/dev/null && return 0
    kill -0 "$SERVER_PID" 2>/dev/null || die "o servidor morreu no arranque"
    sleep 0.5
  done
  die "o servidor não ficou pronto"
}

# Corre o worker uma vez. Devolve o código de saída sem abortar o script.
run_worker() {
  set +e
  env -i PATH="$PATH" HOME="$HOME" \
    DELONIX_GRPC_ADDR="127.0.0.1:$GRPC_PORT" RECORDINGS_DIR="$REC_DIR" \
    DELONIX_PROTO_GEN_DIR="$WORK/gen" WORKER_ID=it-worker LEASE_SECONDS=120 \
    TRANSCRIBER=fake FAKE_TRANSCRIPT="Bom dia. A chave é $SECRET e ficou decidido o prazo." \
    "$@" "$PY" ai-worker/transcribe_worker.py --once >>"$WORK/worker.log" 2>&1
  local rc=$?
  set -e
  echo "$rc"
}

seed_recording() { # $1 email da conta
  psql_it -c "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes)
              SELECT r.id, u.id, 'it.webm', 4 FROM rooms r, users u
               WHERE u.email = '$1' ORDER BY r.created_at LIMIT 1 RETURNING id" | head -1
}

# ---------------------------------------------------------------- 1. texto claro
start_server
ok "servidor em :$HTTP_PORT (HTTP) e :$GRPC_PORT (gRPC texto claro)"

EMAIL="admin@aiworker-it.test"
curl -fs -X POST "http://127.0.0.1:$HTTP_PORT/api/auth/register" -H 'content-type: application/json' \
  -d "{\"org_name\":\"AI Worker IT\",\"email\":\"$EMAIL\",\"password\":\"UmaPasswordForte123!\"}" >/dev/null \
  || die "registo falhou"
TOKEN=$(curl -fs -X POST "http://127.0.0.1:$HTTP_PORT/api/auth/login" -H 'content-type: application/json' \
  -d "{\"email\":\"$EMAIL\",\"password\":\"UmaPasswordForte123!\"}" | "$PY" -c 'import json,sys; print(json.load(sys.stdin)["access_token"])') \
  || die "login falhou"
curl -fs -X POST "http://127.0.0.1:$HTTP_PORT/api/rooms" -H 'content-type: application/json' \
  -H "authorization: Bearer $TOKEN" -d '{"name":"x"}' >/dev/null || die "criar sala falhou"
ok "org, conta e sala criadas pela API"

REC=$(seed_recording "$EMAIL"); [ -n "$REC" ] || die "INSERT da gravação falhou"
printf '\x1a\x45\xdf\xa3' >"$REC_DIR/$REC.webm"

rc=$(run_worker GRPC_INSECURE=1)
[ "$rc" = 0 ] || die "worker (texto claro) saiu com $rc — $(tail -5 "$WORK/worker.log")"
row=$(psql_it -c "SELECT transcribed_at IS NOT NULL, transcript, minutes <> '', transcription_lease_token IS NULL
                   FROM recordings WHERE id = '$REC'")
case "$row" in t\|*CHAVE\ API\ CENSURADA*\|t\|t) ok "texto claro: entregue, transcribed_at preenchido, lease libertada" ;;
  *) die "linha inesperada: $row" ;; esac
case "$row" in *"$SECRET"*) die "o segredo chegou à base sem DLP: $row" ;; esac
ok "DLP aplicado no servidor: «CHAVE API CENSURADA», o sk-… não está na base"

rc=$(run_worker GRPC_INSECURE=1)
[ "$rc" = 3 ] || die "fila vazia devia dar 3, deu $rc"
ok "fila vazia: --once sai com 3"

# ---------------------------------------------------------------- 2. ficheiro em falta
REC2=$(seed_recording "$EMAIL")
rc=$(run_worker GRPC_INSECURE=1)
[ "$rc" = 0 ] || die "worker (ficheiro em falta) saiu com $rc"
row=$(psql_it -c "SELECT transcription_failed_at IS NOT NULL, transcription_error, transcribed_at IS NULL
                   FROM recordings WHERE id = '$REC2'")
case "$row" in "t|ficheiro em falta: $REC2.webm|t") ok "ficheiro em falta: FailJob não-retryable, fora da fila" ;;
  *) die "linha inesperada: $row" ;; esac

# Sem certificados e sem GRPC_INSECURE: recusa antes de ligar.
rc=$(run_worker)
[ "$rc" = 2 ] || die "sem mTLS nem GRPC_INSECURE devia sair com 2, deu $rc"
ok "sem certificados nem GRPC_INSECURE=1: recusa com código 2"
stop_server

# ---------------------------------------------------------------- 3. mTLS
cd "$PKI"
ecparam() { openssl ecparam -name prime256v1 -genkey -noout -out "$1" 2>/dev/null; }
ecparam ca.key
openssl req -x509 -new -key ca.key -subj "/CN=delonix-meet-internal-ca" -days 1 -out ca.crt 2>/dev/null
ecparam server.key
openssl req -new -key server.key -subj "/CN=localhost" -out server.csr 2>/dev/null
printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n' >server.ext
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 1 -extfile server.ext -out server.crt 2>/dev/null
ecparam client.key
openssl req -new -key client.key -subj "/CN=delonix-ai-worker" -out client.csr 2>/dev/null
printf 'extendedKeyUsage=clientAuth\n' >client.ext
openssl x509 -req -in client.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 1 -extfile client.ext -out client.crt 2>/dev/null
ecparam rogue-ca.key
openssl req -x509 -new -key rogue-ca.key -subj "/CN=rogue-ca" -days 1 -out rogue-ca.crt 2>/dev/null
ecparam rogue.key
openssl req -new -key rogue.key -subj "/CN=rogue" -out rogue.csr 2>/dev/null
openssl x509 -req -in rogue.csr -CA rogue-ca.crt -CAkey rogue-ca.key -CAcreateserial -days 1 -extfile client.ext -out rogue.crt 2>/dev/null
cd "$ROOT"
ok "PKI de teste gerada (CA interna, servidor, cliente, e uma CA estranha)"

start_server GRPC_TLS_CERT="$PKI/server.crt" GRPC_TLS_KEY="$PKI/server.key" GRPC_CLIENT_CA="$PKI/ca.crt"
ok "servidor com mTLS em :$GRPC_PORT"

REC3=$(seed_recording "$EMAIL")
printf '\x1a\x45\xdf\xa3' >"$REC_DIR/$REC3.webm"

rc=$(run_worker DELONIX_GRPC_ADDR="localhost:$GRPC_PORT" GRPC_CLIENT_CERT="$PKI/rogue.crt" GRPC_CLIENT_KEY="$PKI/rogue.key" GRPC_CA="$PKI/ca.crt")
[ "$rc" = 1 ] || die "certificado de outra CA devia falhar (1), deu $rc"
ok "mTLS: certificado de cliente de OUTRA CA recusado"

rc=$(run_worker DELONIX_GRPC_ADDR="localhost:$GRPC_PORT" GRPC_INSECURE=1)
[ "$rc" = 1 ] || die "texto claro contra listener mTLS devia falhar (1), deu $rc"
ok "mTLS: cliente em texto claro recusado"

done3=$(psql_it -c "SELECT transcribed_at IS NULL FROM recordings WHERE id = '$REC3'")
[ "$done3" = t ] || die "uma tentativa recusada não pode ter escrito nada"

rc=$(run_worker DELONIX_GRPC_ADDR="localhost:$GRPC_PORT" GRPC_CLIENT_CERT="$PKI/client.crt" GRPC_CLIENT_KEY="$PKI/client.key" GRPC_CA="$PKI/ca.crt")
[ "$rc" = 0 ] || die "worker com mTLS saiu com $rc — $(tail -5 "$WORK/worker.log")"
row=$(psql_it -c "SELECT transcribed_at IS NOT NULL, transcript LIKE '%CHAVE API CENSURADA%' FROM recordings WHERE id = '$REC3'")
[ "$row" = "t|t" ] || die "mTLS: linha inesperada: $row"
ok "mTLS: certificado da CA interna entrega, com DLP"

echo "✓ ai-worker ↔ delonix-server por gRPC: texto claro, falha definitiva e mTLS provados"
