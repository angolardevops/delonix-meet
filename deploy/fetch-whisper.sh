#!/usr/bin/env bash
# ============================================================
#  Descarrega o modelo Whisper-tiny (transcrição local, self-hosted) para o
#  frontend. FORA do git — igual à estratégia do modelo RVM (ver deploy.sh).
#  Fonte: HuggingFace Xenova/whisper-tiny. Integridade por SHA-256 fixado.
#
#  Uso: deploy/fetch-whisper.sh [DIR_DESTINO]
#       (default: web/public/models/Xenova/whisper-tiny relativo à raiz do repo)
# ============================================================
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-$ROOT/web/public/models/Xenova/whisper-tiny}"
BASE="${WHISPER_BASE_URL:-https://huggingface.co/Xenova/whisper-tiny/resolve/main}"

# ficheiro  SHA-256 (pinado dos artefactos conhecidos-bons)
FILES="
config.json 2b2e4e519084e0ea028b19b153f95202735a971870d6844aa26e559edd292e94
generation_config.json 68ac791fcb4999461a313472125042934656240ba1cba7d1c2627fcbb19ac24c
preprocessor_config.json a6a76d28c93edb273669eb9e0b0636a2bddbb1272c3261e47b7ca6dfdbac1b8d
tokenizer.json 27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566
tokenizer_config.json 2a4c4281cf9f51ac6ccc406fdc711a087afe6530f671fa7b80953edc498275ce
onnx/encoder_model_quantized.onnx fd9d995b9dcb0520f0dbf6cf68651af639fc385f594d9d876e69ca2802dc438e
onnx/decoder_model_merged_quantized.onnx 6c0c125986b007d2e3734bec84c18bda0152071b90b87fadac6d7764499927a0
"

sha_ok() { command -v sha256sum >/dev/null || return 0; [ "$(sha256sum "$1" | awk '{print $1}')" = "$2" ]; }

echo "▶ Whisper-tiny → $DEST"
echo "$FILES" | while read -r rel sha; do
  [ -z "$rel" ] && continue
  out="$DEST/$rel"
  if [ -f "$out" ] && sha_ok "$out" "$sha"; then
    echo "  ✓ $rel (em cache)"; continue
  fi
  mkdir -p "$(dirname "$out")"
  echo "  ↓ $rel"
  curl -fsSL "$BASE/$rel" -o "$out"
  sha_ok "$out" "$sha" || { echo "✗ SHA-256 não confere: $rel" >&2; rm -f "$out"; exit 1; }
done
echo "✓ Whisper-tiny pronto."
