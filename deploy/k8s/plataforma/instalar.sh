#!/usr/bin/env bash
# ============================================================
#  A plataforma do cluster de produção do Meet (ADR-0020, fase 3).
#
#  Por esta ordem, e a ordem importa: o MinIO antes do Postgres (o backup do
#  Postgres escreve lá), e o cert-manager antes do ingress ter certificados
#  para pedir.
#
#  Idempotente: `helm upgrade --install` e `kubectl apply`. Correr duas vezes
#  não parte nada.
#
#  Os SEGREDOS não estão aqui e não se geram aqui: ver o README. Este script
#  RECUSA-SE a correr sem eles, em vez de inventar passwords que ninguém
#  anotou.
# ============================================================
set -euo pipefail
cd "$(dirname "$0")"

NS=${NS:-delonix-meet}
g=$'\033[1;32m'; y=$'\033[1;33m'; z=$'\033[0m'
ok() { printf "  %s✓%s %s\n" "$g" "$z" "$1"; }
passo() { printf "\n%s▶ %s%s\n" "$y" "$1" "$z"; }
morre() { printf "  %s✗%s %s\n" "$y" "$z" "$1" >&2; exit 1; }

command -v helm >/dev/null || morre "falta o helm"
command -v kubectl >/dev/null || morre "falta o kubectl"
kubectl cluster-info >/dev/null 2>&1 || morre "o kubectl não fala com cluster nenhum — exporta o KUBECONFIG"

kubectl get ns "$NS" >/dev/null 2>&1 || kubectl create ns "$NS"
kubectl get ns observabilidade >/dev/null 2>&1 || kubectl create ns observabilidade

# Os segredos primeiro: falhar aqui é barato, falhar a meio da instalação não.
for s in meet-pg-credenciais meet-redis-credenciais meet-minio-credenciais; do
  kubectl -n "$NS" get secret "$s" >/dev/null 2>&1 \
    || morre "falta o Secret $s em $NS — ver o README §Segredos. NÃO invento passwords."
done
ok "os três Secrets existem"

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

passo "MinIO (antes do Postgres: o backup dele escreve aqui)"
helm repo add bitnami https://charts.bitnami.com/bitnami >/dev/null
helm upgrade --install minio bitnami/minio \
  -n minio --create-namespace -f minio-values.yaml --wait --timeout 15m
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

printf "\n%s✓ plataforma instalada%s\n" "$g" "$z"
printf "  Postgres (escrita): meet-pg-rw.%s.svc.cluster.local:5432\n" "$NS"
printf "  Postgres (leitura): meet-pg-ro.%s.svc.cluster.local:5432\n" "$NS"
printf "  Redis:              redis.%s.svc.cluster.local:6379 (Sentinel: redis-headless)\n" "$NS"
printf "  MinIO:              minio.minio.svc.cluster.local:9000\n"
printf "\n  O DATABASE_URL e o REDIS_URL do Secret da aplicação apontam para estes.\n"
