#!/usr/bin/env bash
# Limpeza semanal de builds do Delonix Meet — evita o disco-cheio recorrente
# (cada make image-push deixa ~1.5GB de cache + 2 tags novas no nó kind).
# Instalado no crontab do utilizador (domingo 03:00); correr à mão é seguro.
#
# NÃO toca: volumes docker (backups pgdata/redisdata), PVCs do cluster
# (gravações, modelos Ollama), server/target, imagem atualmente pinada.
set -euo pipefail

echo "[prune-builds] $(date -Is)"

# 1) Cache BuildKit: mantém os 8GB mais recentes (o próximo build fica rápido).
docker builder prune -af --keep-storage 8GB | tail -1

# 2) Imagens não usadas no host (as ativas de containers ficam).
docker image prune -af | tail -1

# 3) Tags delonix antigas no nó kind: mantém a tag PINADA nos Deployments + latest.
NODE=delonix-stage-control-plane
if docker ps --format '{{.Names}}' | grep -qx "$NODE"; then
  PINNED=$(kubectl --context kind-delonix-stage -n delonix-meet get deploy delonix-server \
    -o jsonpath='{.spec.template.spec.containers[0].image}' 2>/dev/null | cut -d: -f2)
  echo "[prune-builds] tag pinada: ${PINNED:-?}"
  docker exec "$NODE" crictl images 2>/dev/null \
    | awk '/delonix-(server|web)/ {print $2}' | sort -u \
    | grep -vE "^(latest|${PINNED:-nunca})$" \
    | while read -r tag; do
        docker exec "$NODE" crictl rmi \
          "docker.io/library/delonix-server:$tag" \
          "docker.io/library/delonix-web:$tag" 2>/dev/null | grep -c Deleted || true
      done | paste -sd+ - | bc | xargs -I{} echo "[prune-builds] {} tags removidas do nó"
  # Órfãs (não usadas por nenhum pod) — pause/infra em uso ficam.
  docker exec "$NODE" crictl rmi --prune 2>/dev/null | tail -1 || true

  # Rede de segurança: com o disco >85% o kubelet faz image-GC por conta
  # própria e pode apagar a tag pinada (aconteceu a 14/07 — rollout ficou em
  # ImagePullBackOff). Confirmar que a pinada continua no nó.
  for img in delonix-server delonix-web; do
    if ! docker exec "$NODE" crictl inspecti "docker.io/library/$img:$PINNED" >/dev/null 2>&1; then
      echo "[prune-builds] AVISO: $img:$PINNED AUSENTE do nó — correr 'make image-push' antes de qualquer rollout!"
    fi
  done
fi

df -h / | tail -1
