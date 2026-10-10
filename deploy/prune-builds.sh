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
if nos=$(docker ps --format '{{.Names}}') && grep -qx "$NODE" <<<"$nos"; then
  PINNED=$(kubectl --context kind-delonix-stage -n ngolacloud-meet get deploy delonix-server \
    -o jsonpath='{.spec.template.spec.containers[0].image}' 2>/dev/null | cut -d: -f2) || PINNED=
  # Sem a tag pinada não se apaga nada do nó: o filtro abaixo só poupava a
  # `latest`. Com `set -e` e `pipefail` um `kubectl` que falha já parava o
  # script aqui — mas calado; o cron de domingo ficava sem saber porquê (R303).
  if [ -z "$PINNED" ]; then
    echo "[prune-builds] ✗ não consegui ler a tag pinada do delonix-server (kubectl, contexto kind-delonix-stage) — as tags do nó ficam como estão" >&2
    exit 1
  fi
  echo "[prune-builds] tag pinada: $PINNED"
  # `|| true` no `grep -v`: num nó já limpo (só a pinada e a `latest`) ele não
  # deixa passar nada, sai com 1, e com `pipefail` o script morria aqui — antes
  # da rede de segurança que está mais abaixo.
  docker exec "$NODE" crictl images 2>/dev/null \
    | awk '/delonix-(server|web)/ {print $2}' | sort -u \
    | { grep -vE "^(latest|${PINNED})$" || true; } \
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
