#!/usr/bin/env bash
# ============================================================
#  Fitness function: catraca do clippy.
#
#  Porquê uma catraca e não `-D warnings`: a árvore herda 32 avisos, quase
#  todos mecânicos, mas alguns ficam em `sfu.rs` e `recorder.rs` — o caminho
#  RTP e o de gravação. Limpá-los em bloco, à pressa, num servidor de media é
#  exactamente o refactor cego que a Regra 0 da arquitectura proíbe. A catraca
#  resolve o que interessa já: o número NÃO PODE SUBIR. Código novo entra
#  limpo, a dívida herdada baixa quando for tratada com o cuidado devido, e
#  baixar o número é a única forma de mexer no ficheiro de referência.
#
#  Uso:  bash scripts/check-clippy-ratchet.sh
#        BLESS=1 bash scripts/check-clippy-ratchet.sh   (baixa a fasquia)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

BASELINE_FILE="scripts/clippy-baseline.txt"
baseline=$(cat "$BASELINE_FILE" 2>/dev/null || echo 0)

# O clippy não reemite avisos de uma compilação em cache; sem isto a contagem
# vinha a zero e a catraca aprovava tudo. Limpa-se SÓ a nossa crate — as
# dependências ficam compiladas e o custo é de segundos, não de minutos.
cargo clean -p delonix-server --manifest-path server/Cargo.toml 2>/dev/null || true
count=$(cargo clippy --manifest-path server/Cargo.toml \
          --all-targets --all-features --message-format=short 2>&1 \
        | grep -cE ': warning: ' || true)

if [ "${BLESS:-0}" = "1" ]; then
  echo "$count" > "$BASELINE_FILE"
  echo "✓ fasquia do clippy fixada em $count"
  exit 0
fi

if [ "$count" -gt "$baseline" ]; then
  echo "✗ clippy: $count avisos, a fasquia é $baseline — código novo entra LIMPO."
  echo "   Vê quais são:  cd server && cargo clippy --all-targets --all-features"
  exit 1
fi
if [ "$count" -lt "$baseline" ]; then
  echo "✗ clippy: $count avisos (menos que a fasquia de $baseline) — obrigado."
  echo "   Baixa a fasquia no mesmo commit:  BLESS=1 bash scripts/check-clippy-ratchet.sh"
  exit 1
fi
echo "✓ clippy na fasquia ($count avisos, sem subir)"
