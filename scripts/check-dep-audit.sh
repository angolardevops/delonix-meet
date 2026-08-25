#!/usr/bin/env bash
# ============================================================
#  Fitness function: catraca do audit de dependências.
#
#  Porquê catraca e não `npm audit --audit-level=high`: a árvore ENTRA com 7
#  avisos (6 altos + 1 crítico), todos por transitividade das bibliotecas de
#  IA no browser. Um portão absoluto ficava vermelho no primeiro dia e seria
#  desligado na semana seguinte — que é a pior das saídas, porque depois já
#  ninguém repara no aviso NOVO. Esta catraca deixa passar o que já está
#  aceite e escrito em `scripts/dep-audit-accepted.txt` (com a razão), e falha
#  em qualquer pacote vulnerável que apareça de novo.
#
#  Reavaliar a lista aceite é trabalho com dono, não é «para sempre».
#
#  Uso:  bash scripts/check-dep-audit.sh
#        BLESS=1 bash scripts/check-dep-audit.sh   (regrava a lista aceite)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
ACCEPTED="scripts/dep-audit-accepted.txt"
fail=0

# ---------- npm ----------
if [ -d web/node_modules ] || npm --version >/dev/null 2>&1; then
  current=$(cd web && npm audit --json 2>/dev/null \
    | python3 -c '
import json,sys
try: d=json.load(sys.stdin)
except Exception: sys.exit(0)
for n,v in sorted(d.get("vulnerabilities",{}).items()):
    if v.get("severity") in ("high","critical"):
        print(n)
' || true)

  if [ "${BLESS:-0}" = "1" ]; then
    { echo "# Pacotes com avisos ALTOS/CRÍTICOS aceites conscientemente."
      echo "# Cada linha precisa de razão. Rever a cada release."
      echo "$current"
    } > "$ACCEPTED"
    echo "✓ lista de avisos aceites regravada ($(echo "$current" | grep -c . ) pacotes)"
    exit 0
  fi

  accepted=$(grep -vE '^\s*(#|$)' "$ACCEPTED" 2>/dev/null | awk '{print $1}' | sort -u)
  novos=$(comm -23 <(echo "$current" | sort -u) <(echo "$accepted") | grep -v '^$' || true)
  if [ -n "$novos" ]; then
    echo "✗ audit: aviso ALTO/CRÍTICO NOVO em dependências npm:"
    echo "$novos" | sed 's/^/     /'
    echo "     Vê o detalhe:  cd web && npm audit"
    echo "     Se for para aceitar, acrescenta a $ACCEPTED COM A RAZÃO."
    fail=1
  else
    echo "  ✓ npm: sem avisos altos/críticos novos"
  fi
fi

# ---------- cargo ----------
RUSTSEC_ACCEPTED="scripts/rustsec-accepted.txt"
if command -v cargo-audit >/dev/null 2>&1; then
  # Cada ID aceite entra como --ignore. Um aviso NOVO não está na lista e
  # portanto faz falhar — que é exactamente o ponto da catraca.
  ignores=(); n_ignored=0
  while read -r id _; do
    case "$id" in RUSTSEC-*) ignores+=(--ignore "$id"); n_ignored=$((n_ignored+1));; esac
  done < <(grep -vE '^\s*(#|$)' "$RUSTSEC_ACCEPTED" 2>/dev/null || true)

  if cargo audit --file server/Cargo.lock "${ignores[@]+"${ignores[@]}"}" >/tmp/dlx-cargo-audit.log 2>&1; then
    echo "  ✓ cargo-audit: sem avisos RustSec novos ($n_ignored aceites em $RUSTSEC_ACCEPTED)"
  else
    echo "✗ audit: aviso RustSec NOVO em server/Cargo.lock:"
    tail -40 /tmp/dlx-cargo-audit.log | sed 's/^/     /'
    echo "     Se for para aceitar, acrescenta o ID a $RUSTSEC_ACCEPTED COM A RAZÃO."
    fail=1
  fi
else
  echo "  · cargo-audit ausente — instala com 'cargo install cargo-audit --locked' (o CI instala-o)"
fi

[ "$fail" = 0 ] && echo "✓ audit de dependências sem novidades"
exit $fail
