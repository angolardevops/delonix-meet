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

# 3) As versões das crates ESTRUTURAIS anunciadas na doc têm de bater com o
#    Cargo.toml. Foi drift a sério: a doc dizia axum 0.7 / sqlx 0.7 quando o
#    código já ia em 0.8 — um agente (ou um humano novo) a ler a tabela
#    raciocina sobre uma API que não é a que está lá, e escreve código que não
#    compila. Só se verificam as que mudam a forma do código.
CARGO="server/Cargo.toml"
for crate in axum sqlx webrtc redis reqwest; do
  # `name = "X.Y..."` ou `name = { version = "X.Y..." }` → fica com X.Y
  real=$(grep -E "^$crate[[:space:]]*=" "$CARGO" \
         | grep -oE '"[0-9]+\.[0-9]+' | head -1 | tr -d '"')
  [ -z "$real" ] && continue
  for doc in HARNESS.md AGENTS.md GEMINI.md; do
    [ -f "$doc" ] || continue
    # Só falha se a doc CITAR uma versão desta crate e for outra.
    claimed=$(grep -oiE "$crate[^0-9a-z]{0,3}[0-9]+\.[0-9]+" "$doc" \
              | grep -oE '[0-9]+\.[0-9]+' | sort -u)
    for c in $claimed; do
      if [ "$c" != "$real" ]; then
        echo "✗ drift: $doc diz '$crate $c' mas $CARGO tem '$real'"
        fail=1
      fi
    done
  done
done

# 4) A doc não pode anunciar verificação de SQL em compile time enquanto o
#    código usa a API de runtime. Esta mentira é cara: manda o leitor esperar
#    que um nome de coluna errado falhe no build, quando falha em produção.
macros=$(grep -rEo 'sqlx::(query|query_as|query_scalar)!' server/src | wc -l)
if [ "$macros" -eq 0 ]; then
  for doc in HARNESS.md AGENTS.md GEMINI.md; do
    [ -f "$doc" ] || continue
    if grep -qE 'query(_as)?!.*(compile.time|verificação em compile)' "$doc" \
       || grep -qiE 'macros? .*compile-time checking' "$doc"; then
      echo "✗ drift: $doc anuncia SQL verificado em compile time, mas server/src usa 0 macros \`query!\` (só API de runtime)"
      fail=1
    fi
  done
fi

if [ "$fail" = 0 ]; then
  echo "✓ docs em sincronia com o código (módulos + migrações + versões + SQL)"
fi
exit $fail
