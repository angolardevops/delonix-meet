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

avaliar() { # stdin: JSON do resumo → OK | FALHA | INCONCLUSIVO + motivo
  python3 -c '
import json,sys
s=json.load(sys.stdin); m=[]
if s["video_ativos_min"] < 0.98*s["video_esperados"]: m.append(f"vídeo activo {s[\"video_ativos_min\"]:.0f}/{s[\"video_esperados\"]}")
if s["perda_video_pct"] >= 2: m.append(f"perda {s[\"perda_video_pct\"]}%")
if s["jitter_p95_ms"] >= 30: m.append(f"jitter p95 {s[\"jitter_p95_ms\"]} ms")
if s["pc_falhadas"] > 0 or s["clientes_com_erro"] > 0: m.append(f"{s[\"pc_falhadas\"]} PC falhadas, {s[\"clientes_com_erro\"]} clientes com erro")
gen = s["gerador_ticks_atrasados_pct"] >= 5
print(("INCONCLUSIVO" if gen and m else "FALHA" if m else "OK") + " " + ("; ".join(m) or "-") + (" [gerador saturado]" if gen else ""))
'
}

for d in $DEGRAUS; do
  if [ "$CEN" = grande ]; then R=1; P=$d; else R=$d; P=$PS; fi
  for tentativa in 1 2; do
    RES=$(taskset -c "$GEN_CPUS" "$GEN" --api "$API" --rooms "$R" --per-room "$P" \
          --server-pid "$SP" --label "$CEN-${R}x$P" --out "$OUT" "$@" 2>>"${OUT%.jsonl}.log" | tail -1)
    V=$(echo "$RES" | avaliar)
    echo "$RES" | python3 -c '
import json,sys; s=json.load(sys.stdin)
print(f"  {s[\"label\"]:>16}  {s[\"participantes\"]:>4} pessoas  vídeo {s[\"video_ativos_min\"]:.0f}/{s[\"video_esperados\"]}  perda {s[\"perda_video_pct\"]:>5}%  jitter95 {s[\"jitter_p95_ms\"]:>5} ms  {s[\"video_mbps\"]:>6} Mbps  srv {s[\"srv_cores\"]:>5} cores  RSS {s[\"srv_rss_mb\"]:.0f} MB  gerador {s[\"gerador_cores\"]} cores  máquina {s[\"maquina_ocupada_pct\"]:.0f}%")'
    echo "      → $V"
    sleep 5 # deixa o servidor fechar as PCs antes do degrau seguinte
    case "$V" in OK*) break;; esac
  done
  case "$V" in OK*) ;; *) echo "PARAGEM no degrau $d ($CEN)"; break;; esac
done
