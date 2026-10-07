#!/usr/bin/env bash
# ============================================================
#  Cria (ou actualiza) o Secret `delonix-secrets` a partir do `.env` desta
#  máquina — a fonte ÚNICA dos segredos da aplicação num cluster que não usa
#  o chart Helm (`make stage`, `make prod`, `make cluster`).
#
#  PORQUE EXISTE
#
#  Até 2026-10-04 o Secret vinha escrito em deploy/k8s/01-config.yaml, num
#  repositório público: o JWT_SECRET, o TURN_SECRET, o PROVISIONING_SECRET e a
#  password da base estavam ao alcance de qualquer clone. Com o JWT_SECRET
#  forja-se uma sessão de qualquer conta em qualquer cluster que tenha
#  aplicado aquele ficheiro. Esses valores estão queimados
#  (scripts/leaked-secrets-accepted.txt) e o servidor recusa-os ao arrancar.
#
#  O `.env` é gerado por `make bootstrap` com valores aleatórios e nunca entra
#  no git. O `make cluster` já montava o Secret assim; agora os três caminhos
#  chamam este script em vez de cada um ter a sua cópia.
#
#  Uso:  bash scripts/k8s-app-secrets.sh <host-da-base> [namespace]
#        (o host é o Service do Postgres visto de dentro do cluster)
#
#  Nada do que aqui passa é escrito no terminal.
# ============================================================
set -euo pipefail
cd "$(dirname "$0")/.."

DB_HOST=${1:?uso: k8s-app-secrets.sh <host-da-base> [namespace]}
NS=${2:-${NS:-ngolacloud-meet}}

[ -f .env ] || { echo "✗ falta o .env — corre «make bootstrap»" >&2; exit 1; }
set -a
# shellcheck disable=SC1091
. ./.env
set +a

for v in POSTGRES_PASSWORD JWT_SECRET TURN_SECRET PROVISIONING_SECRET DATA_ENCRYPTION_KEYS; do
  [ -n "${!v:-}" ] || { echo "✗ o .env não tem ${v} — corre «make bootstrap»" >&2; exit 1; }
done

kubectl -n "$NS" create secret generic delonix-secrets \
  --from-literal=DATABASE_URL="postgres://delonix:${POSTGRES_PASSWORD}@${DB_HOST}:5432/delonix_meet" \
  --from-literal=JWT_SECRET="$JWT_SECRET" \
  --from-literal=TURN_SECRET="$TURN_SECRET" \
  --from-literal=PROVISIONING_SECRET="$PROVISIONING_SECRET" \
  --from-literal=DATA_ENCRYPTION_KEYS="$DATA_ENCRYPTION_KEYS" \
  --from-literal=POSTGRES_PASSWORD="$POSTGRES_PASSWORD" \
  --from-literal=POSTGRES_USER=delonix \
  --dry-run=client -o yaml | kubectl apply -f - >/dev/null
echo "   ✓ delonix-secrets (a partir do .env; base em ${DB_HOST})"
