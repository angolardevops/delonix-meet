#!/usr/bin/env bash
# ============================================================
#  Fitness function: o bordo SIP não se anuncia como 0.0.0.0 (R277).
#
#  Um `listen=…:0.0.0.0:…` sem `advertise` faz o Kamailio escrever `0.0.0.0` no
#  Record-Route. Quem liga manda o ACK para lá, o ACK não sai, e o FreeSWITCH
#  desliga a chamada aos 32 s. As chamadas de prova duravam 4 s e não o viam.
#
#  Estático: lê voice/kamailio/kamailio.cfg. A prova com chamada é
#  `bash scripts/pbx-tronco-prova.sh longa` (fora do CI).
#
#  Uso:  bash scripts/check-bordo-anuncia.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
CFG=voice/kamailio/kamailio.cfg
[ -f "$CFG" ] || { echo "✗ R277: falta $CFG — o portão não está a olhar para onde devia"; exit 1; }
n=$(grep -cE '^[[:space:]]*listen=' "$CFG")
[ "$n" -ge 1 ] || { echo "✗ R277: $CFG não tem nenhum listen="; exit 1; }
maus=$(grep -nE '^[[:space:]]*listen=[a-z]+:(0\.0\.0\.0|\*|\[::\])' "$CFG" | grep -v 'advertise' || true)
if [ -n "$maus" ]; then
  echo "✗ R277: o bordo escuta em todas as interfaces sem dizer que endereço anunciar —"
  echo "  o Record-Route sai com 0.0.0.0 e o ACK de quem liga nunca chega:"
  echo "$maus" | sed 's/^/     /'
  exit 1
fi
printf "  ✓ o bordo SIP anuncia um endereço real (%d listen, nenhum em 0.0.0.0 sem advertise)\n" "$n"
