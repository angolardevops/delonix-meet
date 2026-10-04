#!/usr/bin/env bash
# ============================================================
#  make cluster — o stack do Delonix Meet num cluster local.
#
#  Cria (se faltar) um cluster `delonix cluster` de um nó — o equivalente ao
#  kind, sem Docker — e põe lá dentro, por esta ordem:
#    ingress-nginx · Postgres e Redis por Helm (com volume persistente) ·
#    configuração e segredos a partir do .env · servidor e web (as imagens de
#    `make build`) · coturn · a voz (Kamailio, FreeSWITCH, PBX de cliente) ·
#    o Ingress em https://$MEET_HOST.
#
#  O cluster é rootless: a rede dos nós NÃO é alcançável do host. O que tem de
#  se ver de fora — o ingress (443) e o TURN (3478/udp) — é publicado em
#  127.0.0.1 com `delonix net ingress publish`, e é para aí que o nome aponta.
#
#  Idempotente. Não pede sudo: a linha de /etc/hosts é dita no fim.
#
#  uso: scripts/cluster.sh up | status | reset-db | down
# ============================================================
set -euo pipefail
cd "$(dirname "$0")/.."

CLUSTER_NAME=${CLUSTER_NAME:-meet}
MEET_HOST=${MEET_HOST:-meet.ngolacloud.local}
IMAGE_TAG=${IMAGE_TAG:-latest}
NS=delonix-meet
INGRESS_NGINX_VERSION=${INGRESS_NGINX_VERSION:-controller-v1.12.1}
LOCAL_PATH_VERSION=${LOCAL_PATH_VERSION:-v0.0.30}
NODE="${CLUSTER_NAME}-control-plane"
# Onde o host alcança o que o cluster publica.
HOST_IP=127.0.0.1
export PATH="$PWD/.tools/bin:$PATH"
mkdir -p .dev
export KUBECONFIG="$PWD/.dev/cluster-${CLUSTER_NAME}.kubeconfig"

c=$'\033[1;36m'; g=$'\033[1;32m'; y=$'\033[1;33m'; r=$'\033[1;31m'; z=$'\033[0m'
passo() { printf "%s▶ %s%s\n" "$c" "$1" "$z"; }
ok() { printf "  %s✓%s %s\n" "$g" "$z" "$1"; }
avisa() { printf "  %s!%s %s\n" "$y" "$z" "$1"; }
morre() { printf "%s✗ %s%s\n" "$r" "$1" "$z" >&2; exit 1; }

existe() { delonix cluster ls 2>/dev/null | awk 'NR>1 {print $1}' | grep -qx "$CLUSTER_NAME"; }
kubeconfig() {
  (umask 077 && delonix cluster kubeconfig "$CLUSTER_NAME" >"$KUBECONFIG" 2>/dev/null) && [ -s "$KUBECONFIG" ] && chmod 600 "$KUBECONFIG"
}
node_ip() { kubectl get nodes -o jsonpath='{.items[0].status.addresses[?(@.type=="InternalIP")].address}'; }

# Publica uma porta do nó em 127.0.0.1. Idempotente: se já está, não repete.
publica() {
  local spec=$1
  # A listagem mostra a especificação inteira («13478:3478/udp»).
  delonix net ingress ls "$NODE" 2>/dev/null | grep -q "publish[[:space:]]\+${spec}[[:space:]]" && return 0
  delonix net ingress publish "$NODE" "$spec" >/dev/null 2>&1
}

fumo() {
  local ip=$1 codigo
  codigo=$(curl -sk -o /dev/null -w '%{http_code}' --max-time 10 \
    --resolve "${MEET_HOST}:443:${ip}" "https://${MEET_HOST}/api/openapi.json" || true)
  [ "$codigo" = 200 ] && ok "https://${MEET_HOST}/api/openapi.json → 200 (o servidor, pelo ingress)" ||
    avisa "https://${MEET_HOST}/api/openapi.json → ${codigo:-sem resposta}"
  codigo=$(curl -sk -o /dev/null -w '%{http_code}' --max-time 10 \
    --resolve "${MEET_HOST}:443:${ip}" "https://${MEET_HOST}/" || true)
  [ "$codigo" = 200 ] && ok "https://${MEET_HOST}/ → 200 (a web)" ||
    avisa "https://${MEET_HOST}/ → ${codigo:-sem resposta}"
}

hosts() {
  local ip=$1
  if grep -qE "^${ip}[[:space:]]+(.*[[:space:]])?${MEET_HOST}([[:space:]]|\$)" /etc/hosts 2>/dev/null; then
    ok "/etc/hosts já aponta ${MEET_HOST} → ${ip}"
  else
    printf "  %sFalta um passo manual%s (pede sudo, por isso não o faço):\n" "$y" "$z"
    printf "    echo '%s %s' | sudo tee -a /etc/hosts\n" "$ip" "$MEET_HOST"
  fi
}

case "${1:-}" in
up)
  for t in delonix kubectl helm curl; do
    command -v "$t" >/dev/null 2>&1 || morre "falta «$t» — corre «make bootstrap»"
  done
  [ -f .env ] || morre "falta o .env — corre «make bootstrap»"
  [ -f "deploy/certs/${MEET_HOST}.crt" ] || morre "falta o certificado de ${MEET_HOST} — corre «make bootstrap»"
  set -a; . ./.env; set +a
  for v in POSTGRES_PASSWORD JWT_SECRET TURN_SECRET PROVISIONING_SECRET VOICE_INTERNAL_SECRET \
    VOICE_CENTRAL_PASSWORD DATA_ENCRYPTION_KEYS; do
    [ -n "${!v:-}" ] || morre "o .env não tem ${v} — corre «make bootstrap»"
  done
  for img in "delonix-server:${IMAGE_TAG}" "delonix-web:${IMAGE_TAG}"; do
    delonix image ls 2>/dev/null | grep -q "${img%%:*}[[:space:]:].*${IMAGE_TAG}\|${img}" ||
      morre "falta a imagem ${img} — corre «make build»"
  done

  passo "cluster «${CLUSTER_NAME}»"
  if existe; then
    ok "já existe"
  else
    # Um nó precisa de delegação de cgroup2; sem o scope o kubelet não arranca.
    systemd-run --user --scope -q -p Delegate=yes delonix cluster create --name "$CLUSTER_NAME"
    ok "criado"
  fi
  kubeconfig || morre "não consegui obter o kubeconfig do cluster «${CLUSTER_NAME}»"
  kubectl wait --for=condition=Ready node --all --timeout=180s >/dev/null
  NODE_IP=$(node_ip)
  ok "nó pronto (${NODE_IP})"

  passo "storage: local-path-provisioner (${LOCAL_PATH_VERSION})"
  # O nó não traz StorageClass: sem isto nenhum PVC sai de Pending.
  kubectl apply -f "https://raw.githubusercontent.com/rancher/local-path-provisioner/${LOCAL_PATH_VERSION}/deploy/local-path-storage.yaml" >/dev/null
  kubectl patch storageclass local-path \
    -p '{"metadata":{"annotations":{"storageclass.kubernetes.io/is-default-class":"true"}}}' >/dev/null
  kubectl -n local-path-storage rollout status deployment/local-path-provisioner --timeout=180s >/dev/null
  ok "StorageClass «local-path» por omissão"

  passo "ingress-nginx (${INGRESS_NGINX_VERSION})"
  kubectl label node --all ingress-ready=true --overwrite >/dev/null
  kubectl apply -f "https://raw.githubusercontent.com/kubernetes/ingress-nginx/${INGRESS_NGINX_VERSION}/deploy/static/provider/kind/deploy.yaml" >/dev/null
  kubectl -n ingress-nginx rollout status deployment/ingress-nginx-controller --timeout=300s >/dev/null
  publica 443:443 || morre "não consegui publicar a porta 443 do nó no host (ocupada por outro processo?)"
  # A 80 só serve para redireccionar; se o host já a usa, o HTTPS chega.
  publica 80:80 || avisa "porta 80 do host ocupada — só HTTPS"
  ok "ingress publicado em ${HOST_IP}:443"

  # O TURN tem de estar publicado ANTES da configuração: o servidor diz ao
  # browser onde ele está. A 3478 do host costuma estar ocupada pelo coturn
  # de desenvolvimento (`make infra`); nesse caso publica-se noutra.
  # Se uma corrida anterior já o publicou na porta alternativa, é essa que vale.
  if delonix net ingress ls "$NODE" 2>/dev/null | grep -q "publish[[:space:]]\+13478:3478/udp[[:space:]]"; then
    TURN_PORT=13478
  else
    TURN_PORT=3478
    if ! publica "${TURN_PORT}:3478/udp"; then
      TURN_PORT=13478
      publica "${TURN_PORT}:3478/udp" || { TURN_PORT=""; avisa "não consegui publicar o TURN no host — media por relay indisponível"; }
    fi
  fi
  [ -n "$TURN_PORT" ] && ok "TURN publicado em ${HOST_IP}:${TURN_PORT}/udp"

  passo "namespace, TLS e configuração (a partir do .env)"
  kubectl apply -f deploy/k8s/00-namespace.yaml >/dev/null
  kubectl -n "$NS" create secret tls delonix-tls-secret \
    --cert="deploy/certs/${MEET_HOST}.crt" --key="deploy/certs/${MEET_HOST}.key" \
    --dry-run=client -o yaml | kubectl apply -f - >/dev/null
  kubectl -n "$NS" create configmap delonix-config \
    --from-literal=BIND_ADDR=0.0.0.0:8180 \
    --from-literal=TURN_HOST="${MEET_HOST}:${TURN_PORT:-3478}" \
    --from-literal=FORCE_TURN_RELAY=1 \
    --from-literal=SFU_EXTERNAL_IP= \
    --from-literal=RECORDINGS_DIR=/var/lib/delonix/recordings \
    --from-literal=CORS_ORIGINS="https://${MEET_HOST}" \
    --from-literal=VOICE_RAMAIS_PUBLIC_HOST="${MEET_HOST}" \
    --from-literal=VOICE_RAMAIS_PUBLIC_PORT=5070 \
    --from-literal=VOICE_RAMAIS_PUBLIC_TRANSPORT=udp \
    --from-literal=REDIS_URL="redis://delonix-redis-master.${NS}.svc.cluster.local:6379" \
    --dry-run=client -o yaml | kubectl apply -f - >/dev/null
  kubectl -n "$NS" create secret generic delonix-secrets \
    --from-literal=DATABASE_URL="postgres://delonix:${POSTGRES_PASSWORD}@delonix-postgres-postgresql.${NS}.svc.cluster.local:5432/delonix_meet" \
    --from-literal=JWT_SECRET="$JWT_SECRET" \
    --from-literal=TURN_SECRET="$TURN_SECRET" \
    --from-literal=PROVISIONING_SECRET="$PROVISIONING_SECRET" \
    --from-literal=POSTGRES_PASSWORD="$POSTGRES_PASSWORD" \
    --from-literal=DATA_ENCRYPTION_KEYS="$DATA_ENCRYPTION_KEYS" \
    --dry-run=client -o yaml | kubectl apply -f - >/dev/null
  kubectl -n "$NS" create secret generic delonix-voice \
    --from-literal=VOICE_INTERNAL_SECRET="$VOICE_INTERNAL_SECRET" \
    --dry-run=client -o yaml | kubectl apply -f - >/dev/null
  ok "delonix-config, delonix-secrets, delonix-voice, delonix-tls-secret"

  passo "Postgres e Redis (Helm, com volume persistente)"
  helm repo add bitnami https://charts.bitnami.com/bitnami >/dev/null 2>&1 || true
  helm repo update bitnami >/dev/null
  helm upgrade --install delonix-postgres bitnami/postgresql -n "$NS" \
    -f deploy/k8s/helm-values/postgres-stage-values.yaml \
    --set auth.password="$POSTGRES_PASSWORD" --set auth.postgresPassword="$POSTGRES_PASSWORD" \
    --set primary.persistence.size=5Gi \
    --wait --timeout 10m >/dev/null
  helm upgrade --install delonix-redis bitnami/redis -n "$NS" \
    -f deploy/k8s/helm-values/redis-stage-values.yaml \
    --set master.persistence.size=1Gi \
    --wait --timeout 10m >/dev/null
  ok "delonix-postgres e delonix-redis prontos"

  passo "imagens → nós do cluster (sem registo)"
  # A tag versionada E a `latest`: os manifestos referem `latest`, e se ela
  # ficasse a apontar para uma imagem antiga no nó, o `apply` abaixo arrancava
  # por instantes o servidor ANTIGO — que aplica as migrações dele e deixa a
  # base inutilizável para o novo (visto a 2026-10-03: «VersionMissing»).
  imagens=("delonix-server:${IMAGE_TAG}" "delonix-web:${IMAGE_TAG}")
  [ "$IMAGE_TAG" = latest ] || imagens+=("delonix-server:latest" "delonix-web:latest")
  delonix cluster load "${imagens[@]}" --name "$CLUSTER_NAME" >/dev/null
  ok "delonix-server:${IMAGE_TAG} e delonix-web:${IMAGE_TAG}"

  passo "servidor, web, coturn e ingress"
  kubectl apply -f deploy/k8s/02-server.yaml -f deploy/k8s/03-web.yaml >/dev/null
  kubectl -n "$NS" set image deployment/delonix-server "server=delonix-server:${IMAGE_TAG}" >/dev/null
  kubectl -n "$NS" set image deployment/delonix-web "web=delonix-web:${IMAGE_TAG}" >/dev/null
  # Com a mesma tag de antes (`latest`), o `set image` não muda nada: só um
  # reinício põe os pods a correr a imagem acabada de carregar.
  if [ "$IMAGE_TAG" = latest ]; then
    kubectl -n "$NS" rollout restart deployment/delonix-server deployment/delonix-web >/dev/null
  fi
  # Um nó só: uma réplica de cada chega, e o volume das gravações é ReadWriteOnce.
  kubectl -n "$NS" scale deployment/delonix-server deployment/delonix-web --replicas=1 >/dev/null
  # O browser fala com o TURN em ${MEET_HOST}:3478 (publicado no host); o
  # relay anuncia o IP do nó, que é onde os pods do servidor o alcançam.
  sed "s/__NODE_IP__/${NODE_IP}/g" deploy/k8s/cluster/coturn.yaml | kubectl apply -f - >/dev/null
  # O ingress de stage, com o host deste ambiente e sem cert-manager (o
  # certificado é o do `make bootstrap`). O /asr só entra se o whisper existir.
  sed -e "s/meet\.delonix\.local/${MEET_HOST}/g" \
    -e '/cert-manager\.io\/cluster-issuer/d' deploy/k8s/04-ingress.yaml | kubectl apply -f - >/dev/null
  kubectl -n "$NS" rollout status deployment/delonix-server --timeout=300s >/dev/null
  kubectl -n "$NS" rollout status deployment/delonix-web --timeout=180s >/dev/null
  kubectl -n "$NS" rollout status deployment/coturn --timeout=180s >/dev/null
  ok "delonix-server, delonix-web e coturn a correr"

  if [ -f deploy/k8s/cluster/voice.yaml ]; then
    passo "voz: Kamailio, FreeSWITCH e PBX de cliente"
    CLUSTER_NAME="$CLUSTER_NAME" NS="$NS" NODE_IP="$NODE_IP" MEET_HOST="$MEET_HOST" \
      VOICE_CENTRAL_PASSWORD="$VOICE_CENTRAL_PASSWORD" bash scripts/cluster-voice.sh
  fi

  passo "prova de fumo"
  fumo "$HOST_IP"
  printf "\n%s✓ cluster «%s» pronto%s — %shttps://%s%s\n" "$g" "$CLUSTER_NAME" "$z" "$y" "$MEET_HOST" "$z"
  hosts "$HOST_IP"
  printf "  kubectl:  export KUBECONFIG=%s\n" "$KUBECONFIG"
  ;;
status)
  existe || morre "o cluster «${CLUSTER_NAME}» não existe — corre «make cluster»"
  kubeconfig || morre "não consegui obter o kubeconfig do cluster «${CLUSTER_NAME}»"
  kubectl get nodes -o wide
  echo
  kubectl -n "$NS" get deploy,statefulset,svc,ingress,pvc 2>/dev/null || true
  echo
  kubectl -n "$NS" get pods -o wide 2>/dev/null || true
  echo
  passo "prova de fumo"
  fumo "$HOST_IP"
  hosts "$HOST_IP"
  ;;
reset-db)
  # Recria a base do cluster. Preciso quando as migrações mudam de forma
  # incompatível (por exemplo, uma renumeração): o servidor recusa arrancar
  # sobre uma base cujo histórico de migrações não bate com o do binário.
  for t in kubectl helm; do
    command -v "$t" >/dev/null 2>&1 || morre "falta «$t» — corre «make bootstrap»"
  done
  existe || morre "o cluster «${CLUSTER_NAME}» não existe — corre «make cluster»"
  kubeconfig || morre "não consegui obter o kubeconfig do cluster «${CLUSTER_NAME}»"
  kubectl -n "$NS" scale deployment/delonix-server --replicas=0 >/dev/null 2>&1 || true
  # Primeiro sai o Postgres; só depois o volume — com o pod de pé, apagar o
  # PVC fica à espera para sempre.
  if helm status delonix-postgres -n "$NS" >/dev/null 2>&1; then
    helm uninstall delonix-postgres -n "$NS" --wait --timeout 3m >/dev/null
  fi
  kubectl -n "$NS" delete pvc data-delonix-postgres-postgresql-0 --ignore-not-found --timeout=120s >/dev/null
  ok "base de dados do cluster apagada — corre «make cluster» para a recriar"
  ;;
down)
  if existe; then
    delonix cluster destroy "$CLUSTER_NAME" 2>/dev/null || delonix cluster destroy --name "$CLUSTER_NAME"
    rm -f "$KUBECONFIG"
    ok "cluster «${CLUSTER_NAME}» destruído"
  else
    ok "o cluster «${CLUSTER_NAME}» não existe"
  fi
  ;;
*)
  echo "uso: $0 up|status|reset-db|down" >&2
  exit 2
  ;;
esac
