#!/usr/bin/env bash
# ============================================================
#  Fitness function: guarda a invariante de shard da ADR-0001
#  (hash(room) → sempre o mesmo pod). Falha se a topologia que
#  impõe a afinidade por sala for quebrada — a regressão R3, que
#  já custou "media num só sentido".
#
#  Verifica (estático, nos manifests):
#   1. o /ws usa um ingress com upstream-hash-by:$arg_room;
#   2. esse ingress aponta para um Service DEDICADO (delonix-server-ws),
#      NÃO o mesmo do /api (senão o ingress-nginx funde e descarta o hash);
#   3. o cliente envia &room=CODE no URL do /ws.
#
#  Opcional (se KUBECTL=1 e cluster acessível): confirma que o Service
#  dedicado existe no cluster.
#
#  Uso:  bash scripts/check-room-affinity.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0
ING=deploy/k8s/04-ingress.yaml
SRV=deploy/k8s/02-server.yaml
CLI=web/src/signaling.ts

# 1) hash por $arg_room presente num ingress /ws
if ! grep -q 'upstream-hash-by: "\?\$arg_room"\?' "$ING"; then
  echo "✗ R3: falta 'upstream-hash-by: \$arg_room' em $ING (afinidade por sala)"; fail=1
fi

# 2) o /ws aponta para um Service DEDICADO (delonix-server-ws), não o partilhado.
if grep -q 'delonix-server-ws' "$ING" && grep -q 'name: delonix-server-ws' "$SRV"; then
  : # ok — Service dedicado existe e é referenciado
else
  echo "✗ R3: o /ws NÃO usa o Service dedicado 'delonix-server-ws' (ingress+service)."
  echo "     Partilhar Service com /api faz o ingress-nginx descartar o hash → split-brain."
  fail=1
fi

# 3) o cliente envia room= no URL do /ws
if ! grep -q 'room=' "$CLI"; then
  echo "✗ R3: o cliente ($CLI) não envia '&room=CODE' → o hash não tem por onde pegar"; fail=1
fi

# 4) opcional: confirmar no cluster vivo
if [ "${KUBECTL:-0}" = "1" ]; then
  if kubectl -n delonix-meet get svc delonix-server-ws >/dev/null 2>&1; then
    echo "  ✓ (live) Service delonix-server-ws existe no cluster"
  else
    echo "✗ (live) Service delonix-server-ws AUSENTE no cluster"; fail=1
  fi
fi

[ "$fail" = 0 ] && echo "✓ afinidade por sala (ADR-0001) intacta — hash(room)→mesmo pod"
exit $fail
