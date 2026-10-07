#!/usr/bin/env bash
# ============================================================
#  A plataforma do cluster de produção do Meet (ADR-0020 fase 3, ADR-0021).
#
#  Por esta ordem, e a ordem importa: as classes de prioridade primeiro (o chart
#  do Meet refere-lhes os nomes, e um nome que não exista faz o API server
#  recusar o pod), o MinIO antes do Postgres (o backup do Postgres escreve lá),
#  e o cert-manager antes do ingress ter certificados para pedir.
#
#  DOIS MODOS, e escolher mal é o que faz isto estragar o trabalho de outros:
#
#    MODO=dedicado    (omissão) — o cluster é só do Meet. Instala tudo,
#                     incluindo ingress-nginx e cert-manager.
#    MODO=partilhado  — o cluster serve OUTROS inquilinos (ADR-0021: o Meet
#                     partilha o `ngola-lda` com o `ngolacloud-system`).
#                     NÃO instala ingress-nginx nem cert-manager: o primeiro
#                     seria um segundo controlador de ingress a competir com
#                     o Envoy Gateway que já lá está e a gastar um IP do
#                     MetalLB; o segundo tentaria adoptar o cert-manager de
#                     outra pessoa — e é ele que serve o TLS do
#                     `ngolacloud-system`.
#
#  Idempotente: `helm upgrade --install` e `kubectl apply`. Correr duas vezes
#  não parte nada — e a SEGUNDA passagem é esperada, porque o ServiceMonitor
#  do MinIO só pode ser ligado depois da fase 4 (ver abaixo).
#
#  Os SEGREDOS não estão aqui e não se geram aqui: ver o README. Este script
#  RECUSA-SE a correr sem eles, em vez de inventar passwords que ninguém
#  anotou.
# ============================================================
set -euo pipefail
cd "$(dirname "$0")"

NS=${NS:-ngolacloud-meet}
MODO=${MODO:-dedicado}
g=$'\033[1;32m'; y=$'\033[1;33m'; z=$'\033[0m'
ok() { printf "  %s✓%s %s\n" "$g" "$z" "$1"; }
salta() { printf "  %s·%s saltado: %s\n" "$y" "$z" "$1"; }
passo() { printf "\n%s▶ %s%s\n" "$y" "$1" "$z"; }
morre() { printf "  %s✗%s %s\n" "$y" "$z" "$1" >&2; exit 1; }

case "$MODO" in
  dedicado|partilhado) ;;
  *) morre "MODO=$MODO não existe. É «dedicado» ou «partilhado» — ver o cabeçalho." ;;
esac

command -v helm >/dev/null || morre "falta o helm"
command -v kubectl >/dev/null || morre "falta o kubectl"
kubectl cluster-info >/dev/null 2>&1 || morre "o kubectl não fala com cluster nenhum — exporta o KUBECONFIG"

printf "%s▶ modo: %s · namespace: %s · cluster: %s%s\n" "$y" "$MODO" "$NS" \
  "$(kubectl config current-context 2>/dev/null || echo '?')" "$z"

kubectl get ns "$NS" >/dev/null 2>&1 || kubectl create ns "$NS"
kubectl get ns observabilidade >/dev/null 2>&1 || kubectl create ns observabilidade

# Os segredos primeiro: falhar aqui é barato, falhar a meio da instalação não.
for s in meet-pg-credenciais meet-redis-credenciais meet-minio-credenciais; do
  kubectl -n "$NS" get secret "$s" >/dev/null 2>&1 \
    || morre "falta o Secret $s em $NS — ver o README §Segredos. NÃO invento passwords."
done
ok "os três Secrets existem"

# As classes de prioridade PRIMEIRO: são objectos de CLUSTER, não têm
# dependência nenhuma, e o `values-production.yaml` do chart refere-lhes os
# nomes — um `priorityClassName` que não exista faz o API server RECUSAR o pod.
# Falhar aqui é barato; falhar no `helm upgrade` do Meet não.
passo "classes de prioridade (meet-critico, meet-normal)"
kubectl apply -f priorityclasses.yaml
ok "classes de prioridade"

if [ "$MODO" = dedicado ]; then
  passo "ingress-nginx"
  helm repo add ingress-nginx https://kubernetes.github.io/ingress-nginx >/dev/null
  helm upgrade --install ingress-nginx ingress-nginx/ingress-nginx \
    -n ingress-nginx --create-namespace -f ingress-nginx-values.yaml --wait --timeout 10m
  ok "ingress-nginx"

  passo "cert-manager"
  helm repo add jetstack https://charts.jetstack.io >/dev/null
  helm upgrade --install cert-manager jetstack/cert-manager \
    -n cert-manager --create-namespace --set crds.enabled=true --wait --timeout 10m
  kubectl apply -f clusterissuer.yaml
  ok "cert-manager e os dois ClusterIssuer"
else
  passo "ingress e certificados (modo partilhado)"
  salta "ingress-nginx — o cluster já tem Envoy Gateway; dois controladores competiam e gastava um IP do MetalLB"
  salta "cert-manager — já instalado e a servir outro inquilino; um upgrade adoptava a instalação dele"
  if kubectl get deploy -n cert-manager cert-manager >/dev/null 2>&1; then
    ok "cert-manager encontrado: $(kubectl -n cert-manager get deploy cert-manager \
         -o jsonpath='{.metadata.labels.app\.kubernetes\.io/version}' 2>/dev/null || echo 'versão?')"
  else
    morre "modo partilhado mas não há cert-manager no cluster — sem ele não há TLS. Instala-o com quem é dono do cluster."
  fi
  printf "    %sO caminho de entrada é teu:%s em partilhado este script NÃO cria ingress nem rota.\n" "$y" "$z"
  printf "    O chart do Meet pede \`ingress.className\`, e o cluster serve por Gateway API —\n"
  printf "    os HTTPRoutes são trabalho à parte (ADR-0021, §«o que isto NÃO decide»).\n"
fi

passo "StorageClass do MinIO (uma réplica — o MinIO já faz erasure coding)"
kubectl apply -f storageclass-minio.yaml
ok "longhorn-minio"

# O CRD `ServiceMonitor` vem do kube-prometheus-stack, que é a FASE 4. Numa
# instalação de zero ainda não existe, e o chart do MinIO falharia com «no
# matches for kind ServiceMonitor». Em vez de obrigar a editar os values à
# mão, detecta-se — e diz-se que a segunda passagem é precisa.
SM_EXTRA=()
if kubectl get crd servicemonitors.monitoring.coreos.com >/dev/null 2>&1; then
  ok "CRD ServiceMonitor presente — as métricas do MinIO vão ser recolhidas"
else
  SM_EXTRA=(--set metrics.serviceMonitor.enabled=false)
  printf "  %s!%s sem o CRD ServiceMonitor (vem da fase 4): métricas do MinIO DESLIGADAS nesta passagem.\n" "$y" "$z"
  printf "    Depois de \`../observabilidade/instalar.sh\`, corre este script OUTRA VEZ para as ligar.\n"
fi

passo "MinIO (antes do Postgres: o backup dele escreve aqui)"
helm repo add bitnami https://charts.bitnami.com/bitnami >/dev/null
helm upgrade --install minio bitnami/minio \
  -n minio --create-namespace -f minio-values.yaml "${SM_EXTRA[@]}" --wait --timeout 15m
ok "MinIO, com os buckets meet-gravacoes e meet-backups"

passo "Redis (Sentinel, COM autenticação)"
helm upgrade --install redis bitnami/redis \
  -n "$NS" -f redis-values.yaml --wait --timeout 10m
ok "Redis"

passo "CloudNativePG (operador)"
helm repo add cnpg https://cloudnative-pg.github.io/charts >/dev/null
helm upgrade --install cnpg cnpg/cloudnative-pg \
  -n cnpg-system --create-namespace --wait --timeout 10m
ok "operador"

passo "Postgres do Meet (3 instâncias, backup contínuo para o MinIO)"
kubectl apply -f cnpg-cluster.yaml
# O `--wait` do kubectl não espera por um Cluster do CNPG: é um recurso do
# operador e o kubectl não sabe quando está pronto.
kubectl -n "$NS" wait --for=condition=Ready cluster/meet-pg --timeout=15m \
  || printf "  %s!%s o cluster Postgres ainda não está pronto — «kubectl -n %s describe cluster meet-pg»\n" "$y" "$z" "$NS"
ok "Postgres"

printf "\n%s✓ plataforma instalada (modo %s)%s\n" "$g" "$MODO" "$z"
printf "  Postgres (escrita): meet-pg-rw.%s.svc.cluster.local:5432\n" "$NS"
printf "  Postgres (leitura): meet-pg-ro.%s.svc.cluster.local:5432\n" "$NS"
printf "  Redis:              redis.%s.svc.cluster.local:6379 (Sentinel: redis-headless)\n" "$NS"
printf "  MinIO:              minio.minio.svc.cluster.local:9000\n"
printf "\n  O DATABASE_URL e o REDIS_URL do Secret da aplicação apontam para estes.\n"
if [ ${#SM_EXTRA[@]} -gt 0 ]; then
  printf "\n  %sFalta:%s correr a fase 4 e DEPOIS este script outra vez, para as métricas do MinIO.\n" "$y" "$z"
fi
