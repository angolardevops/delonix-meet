#!/usr/bin/env bash
# Rampa de carga do Delonix Meet: sobe um degrau de cada vez e PÁRA no primeiro
# degrau que falhe o critério de qualidade (confirmado por uma 2.ª corrida, para
# não confundir um pico de outra coisa na máquina com o limite do servidor).
#
#   loadgen-rampa.sh <cenário> <por_sala|salas> "<degraus>" [args extra do loadgen]
#
#   cenário «salas»  : degraus = nº de salas, com <por_sala> pessoas cada
#   cenário «grande» : degraus = pessoas numa só sala (<por_sala> é ignorado)
#
# Critério de «suportado» (todos, em média nas janelas estáveis):
#   fluxos de vídeo activos ≥ 98% dos esperados, perda de vídeo < 2%,
#   jitter p95 < 30 ms, nenhuma PeerConnection falhada, e o GERADOR não
#   saturado (ticks atrasados < 5%) — senão o degrau mede o gerador, não o
#   servidor, e é marcado «inconclusivo».
set -uo pipefail
CEN=$1; PS=$2; DEGRAUS=$3; shift 3
API=${API:-http://127.0.0.1:8280}
RAIZ=$(cd "$(dirname "$0")/../.." && pwd)
GEN="$RAIZ/server/target/release/examples/loadgen"
OUT=${OUT:-$RAIZ/.carga/resultados/$CEN-$(date +%H%M%S).jsonl}
GEN_CPUS=${GEN_CPUS:-8-15,24-31}
PORTA=${API##*:}
SP=$(ss -ltnpH "sport = :$PORTA" | grep -oP 'pid=\K[0-9]+' | head -1)
[ -n "$SP" ] || { echo "servidor não encontrado em :$PORTA"; exit 1; }
echo "servidor pid=$SP ($(taskset -cp "$SP" | awk -F: '{print $2}')) · gerador em $GEN_CPUS · resultados → $OUT"


for d in $DEGRAUS; do
  if [ "$CEN" = grande ]; then R=1; P=$d; else R=$d; P=$PS; fi
  for tentativa in 1 2; do
    RES=$(taskset -c "$GEN_CPUS" "$GEN" --api "$API" --rooms "$R" --per-room "$P" \
          --server-pid "$SP" --label "$CEN-${R}x$P" --out "$OUT" "$@" 2>>"${OUT%.jsonl}.log" | tail -1)
    V=$(echo "$RES" | python3 "$RAIZ/server/examples/loadgen-avaliar.py" veredicto)
    echo "$RES" | python3 "$RAIZ/server/examples/loadgen-avaliar.py" linha
    echo "      → $V"
    sleep 5 # deixa o servidor fechar as PCs antes do degrau seguinte
    case "$V" in OK*) break;; esac
  done
  case "$V" in OK*) ;; *) echo "PARAGEM no degrau $d ($CEN)"; break;; esac
done
