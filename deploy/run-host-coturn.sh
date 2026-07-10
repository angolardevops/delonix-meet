#!/usr/bin/env bash
# ============================================================
#  coturn (TURN/STUN) no HOST, com network=host — o caminho fiável para a
#  media WebRTC quando NÃO há IP público (stage/kind) OU em produção com IP
#  próprio. Corre fora do cluster: o browser e os pods (SFU) relayam por aqui.
#
#  Uso:  deploy/run-host-coturn.sh [EXTERNAL_IP]
#        TURN_SECRET=<segredo>  deploy/run-host-coturn.sh 172.16.31.207
#
#  EXTERNAL_IP  IP alcançável a anunciar (default: IP principal do host).
#  TURN_SECRET  MESMO valor que o TURN_SECRET da app (obrigatório).
#
#  Portas: 3478 (STUN/TURN) + 50300-50400 UDP (relay). Abrir no firewall se
#  aplicável.
# ============================================================
set -euo pipefail
EXTERNAL_IP="${1:-$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{print $7; exit}')}"
: "${TURN_SECRET:?Define TURN_SECRET (mesmo valor da app)}"
NAME="delonix-coturn-host"

echo "▶ coturn host  external-ip=$EXTERNAL_IP  relay=50300-50400"

# Liberta o 3478: pára coturns anteriores (compose de dev ou execução prévia).
docker rm -f "$NAME" delonix-meet-coturn-1 >/dev/null 2>&1 || true

docker run -d --name "$NAME" --restart unless-stopped --network host \
  coturn/coturn:4.6 \
  -n --log-file=stdout --simple-log \
  --listening-port=3478 \
  --fingerprint --use-auth-secret \
  --static-auth-secret="$TURN_SECRET" \
  --realm=delonix \
  --min-port=50300 --max-port=50400 \
  --external-ip="$EXTERNAL_IP" \
  --no-cli --no-tlsv1 --no-tlsv1_1 >/dev/null

sleep 2
docker logs "$NAME" 2>&1 | tail -4
echo "✓ coturn a correr em $EXTERNAL_IP:3478 (container '$NAME')"
