#!/usr/bin/env bash
# ============================================================
#  Fitness function: contratos gRPC internos (ADR-0006 §3).
#
#  1. `buf lint` — nomes, pacotes versionados, Request/Response por RPC.
#  2. `buf breaking` contra a origin/main — um campo removido ou renumerado
#     parte o ai-worker ou o IVR que já corre com a versão anterior. Um campo
#     que sai fica `reserved`.
#     Enquanto a origin/main não tiver `server/proto`, não há contra o que
#     comparar e o passo 2 é saltado COM AVISO (não passa a verde em silêncio).
#
#  Uso:  bash scripts/check-proto.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

if ! command -v buf >/dev/null 2>&1; then
  echo "✗ proto: 'buf' não está instalado (https://buf.build/docs/installation) — o portão não corre sem ele"
  exit 1
fi

(cd server/proto && buf lint) || { echo "✗ proto: buf lint falhou"; exit 1; }

git fetch -q origin main 2>/dev/null || true
if git cat-file -e origin/main:server/proto/buf.yaml 2>/dev/null; then
  (cd server/proto && buf breaking --against "../../.git#branch=origin/main,subdir=server/proto") \
    || { echo "✗ proto: buf breaking — contrato incompatível com a origin/main"; exit 1; }
  echo "✓ proto: lint e compatibilidade com a origin/main"
else
  echo "✓ proto: lint (· breaking saltado: a origin/main ainda não tem server/proto)"
fi
