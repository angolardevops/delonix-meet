#!/usr/bin/env bash
# ============================================================
#  Fitness function: os compose das réplicas de prova lêem-se.
#
#  As réplicas de voz (voice/*/compose.yaml) só correm à mão, fora do CI. Duas
#  entregas acrescentaram a MESMA chave ao ambiente do servidor de uma delas,
#  cada uma na sua linha: o git fundiu sem conflito, o YAML ficou com a chave
#  repetida, e o `docker compose` passou a recusar o ficheiro — a réplica deixou
#  de arrancar e ninguém soube, porque nada a lia (medido a 2026-10-05).
#
#  Aqui o próprio `docker compose` lê cada um (`config -q`), com valores de
#  enchimento nas variáveis que eles exigem. Não sobe nada.
#
#  Uso:  bash scripts/check-replicas-compose.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
if ! docker compose version >/dev/null 2>&1; then
  echo "  ! réplicas de prova: NÃO CORREU — esta máquina não tem o docker compose"
  exit 0
fi
erros=0; n=0
for f in voice/*/compose.yaml; do
  [ -f "$f" ] || continue
  n=$((n + 1))
  # As variáveis que o ficheiro interpola, todas com um valor qualquer — um
  # caminho absoluto, porque algumas são directórios de montagens.
  amb=()
  while read -r v; do amb+=("$v=/enchimento"); done < <(grep -oE '\$\{[A-Z_][A-Z0-9_]*' "$f" | tr -d '${' | sort -u)
  if ! saida=$(env "${amb[@]}" docker compose -f "$f" config -q 2>&1); then
    echo "✗ $f não se lê:"
    printf '%s\n' "$saida" | head -4 | sed 's/^/     /'
    erros=1
  fi
done
[ "$n" -ge 1 ] || { echo "✗ nenhum voice/*/compose.yaml — o portão não está a olhar para onde devia"; exit 1; }
[ "$erros" -eq 0 ] || exit 1
printf "  ✓ os compose das réplicas de prova lêem-se (%d ficheiro(s), pelo docker compose)\n" "$n"
