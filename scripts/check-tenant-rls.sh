#!/usr/bin/env bash
# ============================================================
#  Fitness function do isolamento de tenant (ADR-0002 / avaliação #1):
#  garante que as tabelas que JÁ têm RLS não o perdem silenciosamente
#  (ENABLE + FORCE row-level security). Guarda contra uma migração ou
#  alteração futura que reabra a fuga cross-org ao nível da BD.
#
#  Requer acesso ao cluster (kubectl) + password do Postgres. É um smoke
#  test de integração; corre onde houver cluster. Sem cluster → skip.
#
#  Uso:  bash scripts/check-tenant-rls.sh
#  Tabelas com RLS esperado (à medida que o rollout avança, acrescentar):
RLS_TABLES="employee_groups"
# ============================================================
set -uo pipefail
NS=delonix-meet
PGPOD=delonix-postgres-postgresql-0

command -v kubectl >/dev/null 2>&1 || { echo "· sem kubectl — skip check-tenant-rls"; exit 0; }
kubectl -n "$NS" get pod "$PGPOD" >/dev/null 2>&1 || { echo "· sem Postgres no cluster — skip check-tenant-rls"; exit 0; }

PGPASS=$(kubectl -n "$NS" get secret delonix-secrets -o jsonpath='{.data.POSTGRES_PASSWORD}' 2>/dev/null | base64 -d)
fail=0
for t in $RLS_TABLES; do
  row=$(kubectl -n "$NS" exec "$PGPOD" -- bash -c \
    "PGPASSWORD='$PGPASS' psql -U delonix -d delonix_meet -tAc \
     \"SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE relname='$t'\"" 2>/dev/null | tr -d '[:space:]')
  if [ "$row" = "t" ]; then
    echo "  ✓ RLS ENABLE+FORCE em '$t'"
  else
    echo "✗ '$t' devia ter RLS ENABLE+FORCE (relrowsecurity+relforce), mas está '$row' — fuga cross-org ao nível da BD (ADR-0002)"
    fail=1
  fi
done
[ "$fail" = 0 ] && echo "✓ isolamento RLS intacto nas tabelas cobertas"
exit $fail
