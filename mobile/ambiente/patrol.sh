#!/usr/bin/env bash
# Corre os testes Patrol da app num emulador já arrancado (emulador.sh), com o gatilho de GSM.
#   patrol.sh [ficheiro de teste]   ·   por omissão: integration_test/convivencia_gsm_test.dart
. "$(dirname "$0")/env.sh"
export PATH="$PATH:$HOME/.pub-cache/bin" PATROL_ANALYTICS_ENABLED=false
ALVO=${1:-integration_test/convivencia_gsm_test.dart}
python3 -I "$(dirname "$0")/gatilho-gsm.py" 8765 & GATILHO=$!
trap 'kill $GATILHO 2>/dev/null' EXIT
cd "$(dirname "$0")/../delonixphone" && patrol test --target "$ALVO"
