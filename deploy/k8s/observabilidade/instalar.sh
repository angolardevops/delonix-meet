#!/usr/bin/env bash
# ============================================================
#  A observabilidade do Meet (ADR-0020, fase 4).
#
#  Depois da plataforma: o Loki e o Tempo guardam no MinIO, e o MinIO é da
#  fase 3.
#
#  O que isto NÃO faz: encaminhar alertas para alguém. O receptor do
#  Alertmanager está vazio de propósito — ver o README.
# ============================================================
set -euo pipefail
cd "$(dirname "$0")"

NS=observabilidade
g=$'\033[1;32m'; y=$'\033[1;33m'; z=$'\033[0m'
ok() { printf "  %s✓%s %s\n" "$g" "$z" "$1"; }
passo() { printf "\n%s▶ %s%s\n" "$y" "$1" "$z"; }
morre() { printf "  %s✗%s %s\n" "$y" "$z" "$1" >&2; exit 1; }

command -v helm >/dev/null || morre "falta o helm"
kubectl cluster-info >/dev/null 2>&1 || morre "o kubectl não fala com cluster nenhum"
kubectl get ns "$NS" >/dev/null 2>&1 || kubectl create ns "$NS"

kubectl -n "$NS" get secret meet-grafana-credenciais >/dev/null 2>&1 \
  || morre "falta o Secret meet-grafana-credenciais em $NS — ver o README. NÃO invento passwords."

# O Loki e o Tempo escrevem no MinIO com as mesmas credenciais do resto.
kubectl -n "$NS" get secret meet-minio-credenciais >/dev/null 2>&1 \
  || morre "falta meet-minio-credenciais em $NS (copia-o de delonix-meet)"

passo "kube-prometheus-stack (Prometheus, Alertmanager, Grafana)"
helm repo add prometheus-community https://prometheus-community.github.io/helm-charts >/dev/null
helm upgrade --install kube-prometheus-stack prometheus-community/kube-prometheus-stack \
  -n "$NS" -f kube-prometheus-stack-values.yaml --wait --timeout 15m
ok "métricas e alertas"

passo "Loki (registos)"
helm repo add grafana https://grafana.github.io/helm-charts >/dev/null
helm upgrade --install loki grafana/loki -n "$NS" -f loki-values.yaml --wait --timeout 10m
ok "Loki"

passo "Alloy (recolhe os registos dos pods e entrega ao Loki)"
helm upgrade --install alloy grafana/alloy -n "$NS" --wait --timeout 10m
ok "Alloy"

passo "Tempo (rastos)"
helm upgrade --install tempo grafana/tempo -n "$NS" -f tempo-values.yaml --wait --timeout 10m
ok "Tempo"

passo "OpenTelemetry Collector (recebe OTLP do servidor)"
helm repo add open-telemetry https://open-telemetry.github.io/opentelemetry-helm-charts >/dev/null
helm upgrade --install otel-collector open-telemetry/opentelemetry-collector \
  -n "$NS" -f otel-collector-values.yaml --wait --timeout 10m
ok "colector"

passo "Os alertas e o ServiceMonitor do Meet"
kubectl apply -f servicemonitor.yaml
kubectl apply -f alertas.yaml
ok "16 alertas e 2 regras de SLO"

printf "\n%s✓ observabilidade instalada%s\n" "$g" "$z"
printf "  Grafana:    kubectl -n %s port-forward svc/kube-prometheus-stack-grafana 3000:80\n" "$NS"
printf "  OTLP:       otel-collector-opentelemetry-collector.%s.svc.cluster.local:4317\n" "$NS"
printf "\n  %sO servidor ainda NÃO emite rastos%s: falta o OTEL_EXPORTER_OTLP_ENDPOINT e a\n" "$y" "$z"
printf "  instrumentação em Rust (fase 5). As MÉTRICAS já são lidas.\n"
printf "  %sNenhum alerta é encaminhado para ninguém%s — o receptor está vazio (README).\n" "$y" "$z"
