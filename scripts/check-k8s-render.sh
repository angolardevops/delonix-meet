#!/usr/bin/env bash
# ============================================================
#  Fitness function: os manifestos Kubernetes RENDERIZAM, e o que sai deles
#  respeita as fronteiras do ADR-0001 e do ADR-0005 §3/§4.
#
#  Porquê: um overlay que não renderiza só se descobre no `kubectl apply` do
#  dia do deploy, e uma porta interna exposta por engano num Ingress não dá
#  erro nenhum — dá uma superfície de voz (/api/voice/ivr/*) ou gRPC aberta à
#  Internet. O `check-room-affinity.sh` lê os ficheiros da BASE; este lê o que
#  os overlays PRODUZEM, que é o que chega ao cluster.
#
#  Verifica:
#   1. `kubectl kustomize` corre sem erro em deploy/k8s, e nos overlays
#      deploy/k8s-overlays/saas e deploy/k8s-overlays/enterprise;
#   2. em TODOS: nenhum Ingress aponta para o Service delonix-server-internal,
#      nem para as portas 8181/9180 (número ou nome internal/grpc);
#   3. nos overlays: a afinidade por sala do ADR-0001 sobrevive ao patch
#      (/ws → delonix-server-ws, upstream-hash-by:$arg_room);
#   4. nos overlays: o Service interno é ClusterIP, existe a NetworkPolicy, o
#      gRPC tem os três ficheiros de mTLS e NINGUÉM liga DELONIX_ALLOW_INSECURE;
#   5. saas: edição/registo/tenancy, DELONIX_MIGRATE=0, Job `migrate` com a
#      MESMA imagem do Deployment, REDIS_URL não vazio, e o HPA igual ao
#      deploy/k8s/21-server-hpa.yaml (a cópia do overlay não pode derivar);
#   6. enterprise: edição/registo/tenancy, DELONIX_MIGRATE=1, uma réplica.
#
#  Opcional (DRYRUN=1): `kubectl apply --dry-run=client` sobre o renderizado.
#  Precisa de um API server acessível (o client dry-run faz discovery dos
#  tipos) e dos CRDs do cert-manager e do MetalLB.
#
#  Uso:  bash scripts/check-k8s-render.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

if ! command -v kubectl >/dev/null 2>&1; then
  echo "✗ kubectl não encontrado — sem ele não há render; este portão não passa em silêncio"
  exit 2
fi

OUT=$(mktemp -d "${TMPDIR:-/tmp}/check-k8s-render.XXXXXX")
trap 'rm -rf "$OUT"' EXIT
fail=0

declare -A TARGETS=(
  [base]=deploy/k8s
  [saas]=deploy/k8s-overlays/saas
  [enterprise]=deploy/k8s-overlays/enterprise
)

for name in base saas enterprise; do
  dir=${TARGETS[$name]}
  if kubectl kustomize "$dir" >"$OUT/$name.yaml" 2>"$OUT/$name.err"; then
    echo "  ✓ render $dir ($(grep -c '^kind:' "$OUT/$name.yaml") recursos)"
  else
    echo "✗ render falhou: $dir"; sed 's/^/    /' "$OUT/$name.err"; fail=1
  fi
done
[ "$fail" = 0 ] || exit 1

if [ "${DRYRUN:-0}" = "1" ]; then
  for name in base saas enterprise; do
    if kubectl apply --dry-run=client -f "$OUT/$name.yaml" >"$OUT/$name.dry" 2>&1; then
      echo "  ✓ (dry-run client) $name"
    else
      echo "✗ (dry-run client) $name"; sed 's/^/    /' "$OUT/$name.dry" | head -20; fail=1
    fi
  done
fi

OUT="$OUT" HPA_SRC=deploy/k8s/21-server-hpa.yaml python3 - <<'PYEOF' || fail=1
import os, sys, yaml

out = os.environ["OUT"]
erros = []

def carregar(nome):
    with open(f"{out}/{nome}.yaml") as f:
        return [d for d in yaml.safe_load_all(f) if d]

def um(docs, kind, name):
    hits = [d for d in docs if d.get("kind") == kind and d["metadata"]["name"] == name]
    return hits[0] if hits else None

def env_de(container):
    return {e["name"]: e for e in container.get("env", [])}

INTERNAL_SVC = "delonix-server-internal"
INTERNAL_PORTS = {8181, 9180, "internal", "grpc"}

def backends(ing):
    spec = ing.get("spec", {})
    if spec.get("defaultBackend"):
        yield "defaultBackend", spec["defaultBackend"]
    for rule in spec.get("rules", []) or []:
        for p in (rule.get("http") or {}).get("paths", []) or []:
            yield p.get("path", "?"), p.get("backend", {})

def svc_ref(b):
    s = b.get("service") or {}
    port = s.get("port") or {}
    return s.get("name"), port.get("number", port.get("name"))

for nome in ("base", "saas", "enterprise"):
    docs = carregar(nome)

    # 2. nenhum Ingress chega às portas internas
    for ing in (d for d in docs if d.get("kind") == "Ingress"):
        for path, b in backends(ing):
            svc, port = svc_ref(b)
            if svc == INTERNAL_SVC or port in INTERNAL_PORTS:
                erros.append(f"[{nome}] Ingress {ing['metadata']['name']} {path} → {svc}:{port} — "
                             "porta interna (8181/9180) exposta por ingress (ADR-0005 §3)")

    if nome == "base":
        continue

    # 3. ADR-0001 sobrevive ao overlay
    ws_ok = False
    for ing in (d for d in docs if d.get("kind") == "Ingress"):
        ann = (ing["metadata"].get("annotations") or {})
        for path, b in backends(ing):
            svc, _ = svc_ref(b)
            if path == "/ws" and svc == "delonix-server-ws" and ann.get(
                    "nginx.ingress.kubernetes.io/upstream-hash-by") == "$arg_room":
                ws_ok = True
    if not ws_ok:
        erros.append(f"[{nome}] /ws não vai para delonix-server-ws com upstream-hash-by:$arg_room (ADR-0001, R3)")
    if not um(docs, "Service", "delonix-server-ws"):
        erros.append(f"[{nome}] falta o Service delonix-server-ws (ADR-0001)")

    # 4. superfície interna
    isvc = um(docs, "Service", INTERNAL_SVC)
    if not isvc:
        erros.append(f"[{nome}] falta o Service {INTERNAL_SVC}")
    else:
        if isvc["spec"].get("type", "ClusterIP") != "ClusterIP":
            erros.append(f"[{nome}] {INTERNAL_SVC} tem de ser ClusterIP")
        portas = {p["port"] for p in isvc["spec"]["ports"]}
        if portas != {8181, 9180}:
            erros.append(f"[{nome}] {INTERNAL_SVC} expõe {sorted(portas)}, esperado [8181, 9180]")
    for svc in (d for d in docs if d.get("kind") == "Service"
                and d["metadata"]["name"] in ("delonix-server", "delonix-server-ws")):
        if {p["port"] for p in svc["spec"]["ports"]} & {8181, 9180}:
            erros.append(f"[{nome}] o Service público {svc['metadata']['name']} não pode levar 8181/9180")
    if not um(docs, "NetworkPolicy", "delonix-server"):
        erros.append(f"[{nome}] falta a NetworkPolicy delonix-server")

    dep = um(docs, "Deployment", "delonix-server")
    c = [x for x in dep["spec"]["template"]["spec"]["containers"] if x["name"] == "server"][0]
    env = env_de(c)
    for k in ("GRPC_TLS_CERT", "GRPC_TLS_KEY", "GRPC_CLIENT_CA", "GRPC_BIND_ADDR", "INTERNAL_BIND_ADDR"):
        if k not in env:
            erros.append(f"[{nome}] falta {k} no delonix-server")
    for d in docs:
        if "DELONIX_ALLOW_INSECURE" in yaml.safe_dump(d):
            erros.append(f"[{nome}] {d['kind']}/{d['metadata']['name']} menciona DELONIX_ALLOW_INSECURE — "
                         "o gRPC é mTLS no Kubernetes, sem excepção")
    if env.get("LOG_FORMAT", {}).get("value") != "json":
        erros.append(f"[{nome}] LOG_FORMAT tem de ser json")
    if not c.get("startupProbe"):
        erros.append(f"[{nome}] falta startupProbe no delonix-server")

    def val(k):
        return env.get(k, {}).get("value")

    esperado = {
        "saas": {"DELONIX_EDITION": "saas", "REGISTRATION_MODE": "open",
                 "TENANCY_MODE": "multi", "DELONIX_MIGRATE": "0"},
        "enterprise": {"DELONIX_EDITION": "enterprise", "REGISTRATION_MODE": "invite",
                       "TENANCY_MODE": "single", "DELONIX_MIGRATE": "1"},
    }[nome]
    for k, v in esperado.items():
        if val(k) != v:
            erros.append(f"[{nome}] {k}={val(k)!r}, esperado {v!r}")

    if nome == "saas":
        job = um(docs, "Job", "delonix-server-migrate")
        if not job:
            erros.append("[saas] falta o Job delonix-server-migrate")
        else:
            jc = job["spec"]["template"]["spec"]["containers"][0]
            if jc.get("args") != ["migrate"]:
                erros.append(f"[saas] Job migrate com args {jc.get('args')!r}, esperado ['migrate']")
            if jc["image"] != c["image"]:
                erros.append(f"[saas] Job migra com {jc['image']} e o Deployment arranca com {c['image']}")
        cm = um(docs, "ConfigMap", "delonix-config")
        if not (cm and (cm.get("data") or {}).get("REDIS_URL")):
            erros.append("[saas] REDIS_URL vazio ou ausente no delonix-config — obrigatório em SaaS (ADR-0005 §2)")
        hpa = um(docs, "HorizontalPodAutoscaler", "delonix-server")
        with open(os.environ["HPA_SRC"]) as f:
            fonte = [d for d in yaml.safe_load_all(f) if d][0]
        if not hpa:
            erros.append("[saas] falta o HPA delonix-server")
        elif hpa["spec"] != fonte["spec"]:
            erros.append("[saas] o HPA do overlay derivou de deploy/k8s/21-server-hpa.yaml — alinhar a cópia")

    if nome == "enterprise":
        if dep["spec"].get("replicas") != 1:
            erros.append(f"[enterprise] replicas={dep['spec'].get('replicas')}, esperado 1")
        if um(docs, "Job", "delonix-server-migrate"):
            erros.append("[enterprise] não há Job de migração: migra no arranque (DELONIX_MIGRATE=1)")

for e in erros:
    print("✗ " + e)
sys.exit(1 if erros else 0)
PYEOF

[ "$fail" = 0 ] && echo "✓ manifestos k8s renderizam; portas internas fora do ingress; ADR-0001 intacto nos overlays"
exit $fail
