#!/usr/bin/env bash
# ============================================================
#  Fitness function de arquitetura evolutiva (Martin Fowler #8):
#  falha se a documentação (HARNESS.md) divergir do código real —
#  módulos backend e range de migrações. Corre em CI / pre-commit.
#
#  Porquê: este projeto usa a doc como harness para agentes de IA e
#  humanos. Doc desatualizada nas fronteiras é pior do que nenhuma —
#  os revisores raciocinam sobre um sistema que já não existe.
#
#  Uso:  bash scripts/check-docs-drift.sh   (exit 1 se houver drift)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

DOC="HARNESS.md"
fail=0

# 1) Cada módulo em server/src/*.rs (exceto main) tem de ser mencionado no HARNESS.md.
#    (main.rs é o bootstrap; não precisa de linha própria na tabela de módulos.)
for f in server/src/*.rs; do
  mod=$(basename "$f" .rs)
  [ "$mod" = "main" ] && continue
  if ! grep -q "\`$mod\.rs\`" "$DOC"; then
    echo "✗ drift: módulo 'server/src/$mod.rs' NÃO está documentado em $DOC (tabela §2)"
    fail=1
  fi
done

# 2) O range de migrações na doc tem de cobrir a última migração real.
last_mig=$(ls server/migrations/*.sql 2>/dev/null | sed -E 's/.*\/([0-9]+)_.*/\1/' | sort -n | tail -1)
if [ -n "${last_mig:-}" ]; then
  if ! grep -qE "0001[–-]$last_mig" "$DOC"; then
    echo "✗ drift: a última migração é '$last_mig' mas $DOC não refere o range '0001–$last_mig'"
    fail=1
  fi
fi

if [ "$fail" = 0 ]; then
  echo "✓ docs em sincronia com o código (módulos + migrações)"
fi
exit $fail
