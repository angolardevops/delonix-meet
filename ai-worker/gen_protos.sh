#!/usr/bin/env bash
# ============================================================
#  Gera os stubs Python do TranscriptionService a partir de server/proto.
#
#  Os stubs NÃO se versionam: o .proto é a única fonte, e uma cópia gerada no
#  git é uma segunda fonte que deriva em silêncio. São gerados:
#   - no build da imagem (ai-worker/Dockerfile, contexto = raiz do repo);
#   - à mão para testes locais: `PYTHON=~/.cache/delonix-ai-worker-venv/bin/python
#     bash ai-worker/gen_protos.sh`.
#
#  Uso:  gen_protos.sh [PROTO_ROOT] [OUT_DIR]
#        (defaults: server/proto e ai-worker/gen, relativos à raiz do repo)
#  Precisa de grpcio-tools na mesma versão do grpcio de runtime.
# ============================================================
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PROTO_ROOT="${1:-$HERE/../server/proto}"
OUT_DIR="${2:-$HERE/gen}"
PYTHON="${PYTHON:-python3}"
PROTO=delonix/meet/transcription/v1/transcription.proto

[ -f "$PROTO_ROOT/$PROTO" ] || { echo "✗ $PROTO_ROOT/$PROTO não existe" >&2; exit 1; }
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
"$PYTHON" -m grpc_tools.protoc -I "$PROTO_ROOT" \
  --python_out="$OUT_DIR" --grpc_python_out="$OUT_DIR" "$PROTO"
echo "✓ stubs gerados em $OUT_DIR"
