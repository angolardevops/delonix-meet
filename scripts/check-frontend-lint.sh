#!/usr/bin/env bash
# Catraca do ESLint do frontend — o número de problemas só pode DESCER.
#
# PORQUE É UMA CATRACA E NÃO UM ZERO: o repo tinha 52
# `// eslint-disable-next-line react-hooks/exhaustive-deps` e NENHUM ESLint —
# comentários decorativos a silenciar uma regra que nada aplicava. Exigir zero
# hoje parava o trabalho; não exigir nada deixava tudo como estava. A catraca é
# a mesma figura do `scripts/clippy-baseline.txt` no backend: fecha a porta sem
# a trancar.
#
# O que se conta são DUAS coisas, e a segunda é o que torna isto honesto:
#   1. os problemas que o ESLint reporta (erros + avisos);
#   2. os `eslint-disable` de regras de hooks — porque um disable é um problema
#      escondido, e sem o contar bastava silenciar para a catraca descer.
#
# BLESS=1 grava a contagem actual (só depois de a olhar).
set -euo pipefail
cd "$(dirname "$0")/.."

BASE=scripts/eslint-baseline.txt
WEB=web

if [ ! -d "$WEB/node_modules/eslint" ]; then
  echo "  · eslint ausente — corre 'cd web && npm install' (o CI instala-o)"
  exit 0
fi

# `|| true`: o eslint sai com 1 quando há erros, e aqui a saída é o número.
saida=$("$WEB/node_modules/.bin/eslint" "$WEB/src" --format json 2>/dev/null || true)
if [ -z "$saida" ]; then
  echo "✗ lint: o eslint não produziu relatório"
  exit 1
fi

problemas=$(printf '%s' "$saida" | python3 -c '
import json,sys
d=json.load(sys.stdin)
print(sum(f["errorCount"]+f["warningCount"] for f in d))
')
# Os disables contam-se no código, não no relatório: é o que o relatório não vê.
# SÓ os que NÃO dizem porquê (mudança de 2026-10-08): a razão está escrita no
# `scripts/eslint-disables-sem-razao.py`. Em resumo: um disable com a razão no
# comentário acima é uma decisão registada, não um problema escondido — e
# contar só os outros faz de «escrever porquê» o caminho para a catraca descer.
escondidos=$(python3 scripts/eslint-disables-sem-razao.py "$WEB/src")
total=$((problemas + escondidos))

if [ "${BLESS:-}" = "1" ]; then
  printf '%s\n' "$total" > "$BASE"
  echo "✓ lint: catraca gravada ($total = $problemas reportados + $escondidos silenciados SEM razão)"
  exit 0
fi

if [ ! -f "$BASE" ]; then
  echo "✗ lint: falta $BASE — corre BLESS=1 bash scripts/check-frontend-lint.sh"
  exit 1
fi
fasquia=$(cat "$BASE")

if [ "$total" -gt "$fasquia" ]; then
  echo "✗ lint: problemas do frontend subiram de $fasquia para $total"
  echo "     ($problemas reportados pelo eslint + $escondidos silenciados SEM razão escrita)"
  echo "     Um disable com a razão no comentário acima não conta — escreve porquê."
  echo "     Vê quais:  cd web && node_modules/.bin/eslint src"
  echo "     Um 'eslint-disable' novo NÃO baixa a conta — é contado na mesma."
  exit 1
fi
if [ "$total" -lt "$fasquia" ]; then
  echo "✓ lint: $total problemas (a fasquia era $fasquia — baixa-a com BLESS=1)"
  exit 0
fi
echo "✓ lint: $total problemas, na fasquia ($problemas reportados + $escondidos silenciados)"
