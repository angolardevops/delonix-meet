#!/usr/bin/env bash
# ============================================================
#  Fitness function: o chart Helm (deploy/helm/delonix-meet) renderiza, recusa
#  o que deve recusar, e o que sai dele respeita as mesmas fronteiras que os
#  manifestos de deploy/k8s (ADR-0001, ADR-0006 §3).
#
#  Porquê: o `check-k8s-render.sh` lê o kustomize; o chart é OUTRO caminho
#  para o mesmo cluster, e uma regra que só um deles cumpre não é uma regra.
#
#  Verifica:
#   1. a configuração de voz do chart (files/voice/) são LIGAÇÕES para voice/
#      — não cópias — e o pacote (`helm package`) leva o conteúdo igual ao do
#      repo (falha se divergir);
#   2. `helm lint` com os três ficheiros de valores;
#   3. as RECUSAS: sem valores, e em produção sem Secret/tag/IP do relay, o
#      `helm template` falha com a mensagem certa; `latest` é recusada em
#      produção; o PBX de laboratório e o Postgres do chart também;
#   4. os três perfis renderizam (produção, produção com voz, laboratório);
#   5. em todos: nenhum Ingress chega ao Service interno nem às portas
#      8181/9180; o /ws vai para o Service dedicado com
#      upstream-hash-by:$arg_room; ninguém liga DELONIX_ALLOW_INSECURE;
#      FORCE_TURN_RELAY=1; readiness em /ready; drain maior que o do servidor;
#   6. produção: NENHUM Secret renderizado, nenhuma imagem sem tag ou `latest`;
#   7. laboratório: os segredos gerados são aleatórios (dois renders diferem);
#   8. nenhum segredo de desenvolvimento conhecido (deploy/k8s/01-config.yaml,
#      server/src/config.rs) aparece no chart nem no que ele renderiza.
#
#  Opcional (DRYRUN=1): `kubectl apply --dry-run=server` do perfil de
#  laboratório no namespace $HELM_DRYRUN_NS (por omissão `meet-helm`, que tem
#  de existir). Precisa de um cluster acessível.
#
#  Uso:  bash scripts/check-helm.sh        (HELM=/caminho/helm para escolher o binário)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

CHART=deploy/helm/delonix-meet
HELM=${HELM:-helm}
if ! command -v "$HELM" >/dev/null 2>&1; then
  echo "✗ helm não encontrado (HELM=$HELM) — sem ele não há render; este portão não passa em silêncio"
  exit 2
fi

OUT=$(mktemp -d "${TMPDIR:-/tmp}/check-helm.XXXXXX")
trap 'rm -rf "$OUT"' EXIT
fail=0
erro() { echo "✗ $1"; fail=1; }

# ---- 1. a voz do chart é a voz do repo --------------------------------
n=0
while IFS= read -r f; do
  n=$((n + 1))
  if [ ! -L "$f" ]; then
    erro "$f é uma CÓPIA — tem de ser uma ligação simbólica para voice/ (uma cópia diverge)"
    continue
  fi
  alvo=$(realpath -e "$f" 2>/dev/null) || { erro "$f aponta para um ficheiro que não existe"; continue; }
  case "$alvo" in
    "$PWD"/voice/*) ;;
    *) erro "$f aponta para fora de voice/ ($alvo)" ;;
  esac
done < <(find "$CHART/files/voice" -mindepth 1 ! -type d | sort)
[ "$n" -gt 0 ] || erro "$CHART/files/voice está vazio"

if "$HELM" package "$CHART" -d "$OUT/pkg" >"$OUT/pkg.log" 2>&1; then
  tar -xmzf "$OUT"/pkg/delonix-meet-*.tgz -C "$OUT/pkg"
  while IFS= read -r f; do
    rel=${f#"$CHART"/}
    cmp -s "$f" "$OUT/pkg/delonix-meet/$rel" || erro "pacote: $rel difere do ficheiro em voice/"
  done < <(find "$CHART/files/voice" -mindepth 1 ! -type d | sort)
  [ -e "$OUT/pkg/delonix-meet/ci" ] && erro "pacote: leva a pasta ci/ (entradas de prova)"
  [ "$fail" = 0 ] && echo "  ✓ voz: $n ligações para voice/, e o pacote leva o mesmo conteúdo"
else
  erro "helm package falhou"; sed 's/^/    /' "$OUT/pkg.log" | grep -v 'found symbolic link' | head -10
fi

# ---- 2. lint ----------------------------------------------------------
PROD=(-f "$CHART/values-production.yaml" -f "$CHART/ci/production-inputs.yaml")
VOZ=("${PROD[@]}" -f "$CHART/ci/production-voice.yaml")
LOCAL=(-f "$CHART/values-local.yaml")

lint() { # nome, args…
  local nome=$1; shift
  if "$HELM" lint "$CHART" "$@" >"$OUT/lint-$nome.log" 2>&1; then
    echo "  ✓ lint ($nome)"
  else
    erro "helm lint ($nome)"; grep -v 'found symbolic link' "$OUT/lint-$nome.log" | sed 's/^/    /' | head -20
  fi
}
lint values
lint production "${PROD[@]}"
lint local "${LOCAL[@]}"

# ---- 3. recusas -------------------------------------------------------
recusa() { # descrição, texto esperado, args…
  local desc=$1 esperado=$2; shift 2
  if "$HELM" template meet "$CHART" -n delonix-meet "$@" >"$OUT/recusa.out" 2>"$OUT/recusa.err"; then
    erro "devia ter sido recusado e renderizou: $desc"
  elif ! grep -qF -- "$esperado" "$OUT/recusa.err"; then
    erro "recusado, mas sem a mensagem «$esperado»: $desc"
    grep -v 'found symbolic link' "$OUT/recusa.err" | sed 's/^/    /' | head -8
  else
    echo "  ✓ recusa: $desc"
  fi
}
recusa "valores por omissão, sozinhos" "secrets.existingSecret"
recusa "produção sem Secret" "secrets.existingSecret: em produção" \
  "${PROD[@]}" --set secrets.existingSecret=
recusa "produção sem tag de imagem" "server.image.tag: falta a tag" \
  "${PROD[@]}" --set image.tag=
recusa "produção com latest" "server.image.tag=latest" \
  "${PROD[@]}" --set image.tag=latest
recusa "produção com segredos gerados" "secrets.create=true é só para laboratório" \
  "${PROD[@]}" --set secrets.create=true
recusa "produção sem IP público do relay" "coturn.externalIP" \
  "${PROD[@]}" --set coturn.externalIP=
recusa "produção sem TLS no ingress" "ingress.tls" \
  "${PROD[@]}" --set ingress.tls.clusterIssuer=
recusa "produção com o Postgres do chart" "postgresql.enabled" \
  "${PROD[@]}" --set postgresql.enabled=true
recusa "produção com o PBX de laboratório" "voice.labPbx.enabled" \
  "${VOZ[@]}" --set voice.labPbx.enabled=true
recusa "produção com voz e sem certificado do bordo" "voice.kamailio.tls.existingSecret" \
  "${VOZ[@]}" --set voice.kamailio.tls.existingSecret=
recusa "várias réplicas com gravações ReadWriteOnce" "recordings.accessMode=ReadWriteOnce" \
  "${PROD[@]}" --set recordings.accessMode=ReadWriteOnce
recusa "várias réplicas sem Redis" "REDIS_URL é obrigatório" \
  "${PROD[@]}" --set externalRedis.fromSecret=false
recusa "ponte telefone↔sala sem IPs do FreeSWITCH" "server.phoneBridge.freeswitchIPs" \
  "${LOCAL[@]}" --set server.phoneBridge.enabled=true

# ---- 4. render --------------------------------------------------------
render() { # nome, args…
  local nome=$1; shift
  if "$HELM" template meet "$CHART" -n delonix-meet "$@" >"$OUT/$nome.yaml" 2>"$OUT/$nome.err"; then
    echo "  ✓ render $nome ($(grep -c '^kind:' "$OUT/$nome.yaml") recursos)"
  else
    erro "render falhou: $nome"; grep -v 'found symbolic link' "$OUT/$nome.err" | sed 's/^/    /' | head -20
  fi
}
render production "${PROD[@]}"
render production-voz "${VOZ[@]}"
render local "${LOCAL[@]}"
render local-2 "${LOCAL[@]}"
[ "$fail" = 0 ] || exit 1

if [ "${DRYRUN:-0}" = "1" ]; then
  ns=${HELM_DRYRUN_NS:-meet-helm}
  if "$HELM" template meet "$CHART" -n "$ns" "${LOCAL[@]}" 2>/dev/null |
    kubectl apply --dry-run=server -n "$ns" -f - >"$OUT/dry.log" 2>&1; then
    echo "  ✓ (dry-run server, ns $ns) local: $(wc -l <"$OUT/dry.log") recursos aceites pelo API server"
  else
    erro "(dry-run server, ns $ns) local"; sed 's/^/    /' "$OUT/dry.log" | grep -v 'created\|configured' | head -20
  fi
fi

# ---- 5–8. o que saiu --------------------------------------------------
OUT="$OUT" CHART="$CHART" python3 - <<'PYEOF' || fail=1
import base64, os, re, subprocess, sys, yaml

out, chart = os.environ["OUT"], os.environ["CHART"]
erros = []

def carregar(nome):
    with open(f"{out}/{nome}.yaml") as f:
        return [d for d in yaml.safe_load_all(f) if d]

def um(docs, kind, name):
    hits = [d for d in docs if d.get("kind") == kind and d["metadata"]["name"] == name]
    return hits[0] if hits else None

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

def pod_specs(docs):
    for d in docs:
        tpl = (d.get("spec") or {}).get("template")
        if isinstance(tpl, dict) and "spec" in tpl:
            yield d, tpl["spec"]

# Segredos de desenvolvimento conhecidos — lidos das fontes, não copiados para aqui.
conhecidos = set()
with open("deploy/k8s/01-config.yaml") as f:
    for d in yaml.safe_load_all(f):
        if d and d.get("kind") == "Secret":
            for k, v in (d.get("stringData") or {}).items():
                if k not in ("POSTGRES_USER", "POSTGRES_DB"):
                    conhecidos.add(str(v))
                    m = re.match(r"postgres://[^:]+:([^@]+)@", str(v))
                    if m:
                        conhecidos.add(m.group(1))
src = open("server/src/config.rs").read()
conhecidos |= set(re.findall(r'^const DEV_\w+: &str = "([^"]+)";', src, re.M))
bloco = re.search(r"BURNED_VOICE_SECRETS: &\[&str\] = &\[(.*?)\];", src, re.S)
conhecidos |= set(re.findall(r'^\s*"([^"]+)",', bloco.group(1), re.M)) if bloco else set()
conhecidos = {c for c in conhecidos if len(c) >= 12}
if len(conhecidos) < 5:
    erros.append(f"só encontrei {len(conhecidos)} segredos de desenvolvimento nas fontes — o portão deixou de os saber ler")

# 8a. nas fontes do chart
fontes = subprocess.run(["git", "ls-files", "-co", "--exclude-standard", chart],
                        capture_output=True, text=True).stdout.split()
for p in fontes:
    if os.path.islink(p) or not os.path.isfile(p):
        continue
    try:
        txt = open(p).read()
    except UnicodeDecodeError:
        continue
    for c in conhecidos:
        if c in txt:
            erros.append(f"{p}: contém um segredo de desenvolvimento conhecido ({c[:6]}…)")

for nome in ("production", "production-voz", "local"):
    docs = carregar(nome)
    bruto = open(f"{out}/{nome}.yaml").read()

    # 5. ingress e portas internas
    for ing in (d for d in docs if d.get("kind") == "Ingress"):
        for path, b in backends(ing):
            svc, port = svc_ref(b)
            if svc == INTERNAL_SVC or port in INTERNAL_PORTS:
                erros.append(f"[{nome}] Ingress {ing['metadata']['name']} {path} → {svc}:{port} — "
                             "porta interna exposta por ingress (ADR-0006 §3)")
    for svc in (d for d in docs if d.get("kind") == "Service"):
        n = svc["metadata"]["name"]
        portas = {p["port"] for p in svc["spec"]["ports"]}
        if n in ("delonix-server", "delonix-server-ws") and portas & {8181, 9180}:
            erros.append(f"[{nome}] o Service público {n} não pode levar 8181/9180")
        if n == INTERNAL_SVC and svc["spec"].get("type", "ClusterIP") != "ClusterIP":
            erros.append(f"[{nome}] {INTERNAL_SVC} tem de ser ClusterIP")
        if portas & {8181, 9180} and n != INTERNAL_SVC:
            erros.append(f"[{nome}] o Service {n} expõe uma porta interna")

    # 5. afinidade por sala (ADR-0001)
    ws_ok, api_svcs = False, set()
    for ing in (d for d in docs if d.get("kind") == "Ingress"):
        ann = ing["metadata"].get("annotations") or {}
        for path, b in backends(ing):
            svc, _ = svc_ref(b)
            if path == "/ws":
                ws_ok = (svc == "delonix-server-ws"
                         and ann.get("nginx.ingress.kubernetes.io/upstream-hash-by") == "$arg_room")
            else:
                api_svcs.add(svc)
    if not ws_ok:
        erros.append(f"[{nome}] /ws não vai para delonix-server-ws com upstream-hash-by:$arg_room (ADR-0001, R3)")
    if "delonix-server-ws" in api_svcs:
        erros.append(f"[{nome}] o Service do /ws é partilhado com outro caminho — o ingress-nginx descarta o hash")
    if not um(docs, "Service", "delonix-server-ws"):
        erros.append(f"[{nome}] falta o Service delonix-server-ws (ADR-0001)")

    # 5. nunca o modo inseguro; media relay-only
    if "DELONIX_ALLOW_INSECURE" in bruto:
        erros.append(f"[{nome}] o render menciona DELONIX_ALLOW_INSECURE")
    cm = um(docs, "ConfigMap", "delonix-config")
    if (cm.get("data") or {}).get("FORCE_TURN_RELAY") != "1":
        erros.append(f"[{nome}] FORCE_TURN_RELAY tem de ser 1 em Kubernetes (R4)")
    if not (cm.get("data") or {}).get("TURN_HOST") and nome != "local":
        erros.append(f"[{nome}] TURN_HOST vazio")

    # 5. drain e sondas do servidor
    dep = um(docs, "Deployment", "delonix-server")
    ps = dep["spec"]["template"]["spec"]
    c = [x for x in ps["containers"] if x["name"] == "server"][0]
    if ps.get("terminationGracePeriodSeconds", 30) <= 52:
        erros.append(f"[{nome}] terminationGracePeriodSeconds tem de ser > 52 (DRAIN_GRACE_SECS 40 + DRAIN_READINESS_SECS 12)")
    rp = c.get("readinessProbe", {})
    if rp.get("httpGet", {}).get("path") != "/ready" or rp.get("failureThreshold") != 1:
        erros.append(f"[{nome}] readiness do servidor tem de ser /ready com failureThreshold 1")
    if c.get("livenessProbe", {}).get("httpGet", {}).get("path") != "/health":
        erros.append(f"[{nome}] liveness do servidor tem de ser /health")
    sc = c.get("securityContext", {})
    if not (ps.get("securityContext", {}).get("runAsNonRoot") and sc.get("readOnlyRootFilesystem")
            and sc.get("allowPrivilegeEscalation") is False and sc.get("capabilities", {}).get("drop") == ["ALL"]):
        erros.append(f"[{nome}] o servidor tem de correr non-root, rootfs só de leitura, sem escalada, drop ALL")
    if any("secretRef" in e for e in c.get("envFrom", [])):
        erros.append(f"[{nome}] o servidor lê o Secret por chave (secretKeyRef), não por envFrom")

    # todos os contentores: recursos e capacidades largadas
    for d, spec in pod_specs(docs):
        for ct in spec.get("containers", []) + spec.get("initContainers", []):
            quem = f"{d['kind']}/{d['metadata']['name']}:{ct['name']}"
            res = ct.get("resources") or {}
            if not res.get("requests") or not res.get("limits"):
                erros.append(f"[{nome}] {quem} sem requests/limits")
            if "ALL" not in ((ct.get("securityContext") or {}).get("capabilities") or {}).get("drop", []):
                erros.append(f"[{nome}] {quem} não larga as capacidades (drop ALL)")

    segredos = [d for d in docs if d.get("kind") == "Secret"]
    if nome.startswith("production"):
        # 6. produção: nenhum Secret, nenhuma imagem solta
        for s in segredos:
            erros.append(f"[{nome}] o chart renderizou o Secret {s['metadata']['name']} — em produção os segredos vêm de fora")
        for d, spec in pod_specs(docs):
            for ct in spec.get("containers", []) + spec.get("initContainers", []):
                img = ct["image"]
                tag = img.rsplit(":", 1)[1] if ":" in img.rsplit("/", 1)[-1] else ""
                if "@sha256:" not in img and tag in ("", "latest"):
                    erros.append(f"[{nome}] {d['kind']}/{d['metadata']['name']}: imagem «{img}» sem tag imutável")
        if not um(docs, "HorizontalPodAutoscaler", "delonix-server"):
            erros.append(f"[{nome}] falta o HPA do servidor")
        if not um(docs, "PodDisruptionBudget", "delonix-server"):
            erros.append(f"[{nome}] falta o PDB do servidor")
        np = um(docs, "NetworkPolicy", "delonix-server")
        if not np:
            erros.append(f"[{nome}] falta a NetworkPolicy do servidor")
        else:
            for regra in np["spec"]["ingress"]:
                portas = {p["port"] for p in regra.get("ports", [])}
                if portas & {8181, 9180} and not regra.get("from"):
                    erros.append(f"[{nome}] NetworkPolicy: 8181/9180 abertas a qualquer origem")
        job = um(docs, "Job", "delonix-server-migrate")
        if not job:
            erros.append(f"[{nome}] falta o Job de migração")
        elif job["spec"]["template"]["spec"]["containers"][0]["image"] != c["image"]:
            erros.append(f"[{nome}] o Job migra com uma imagem e o Deployment arranca com outra")
        pvc = um(docs, "PersistentVolumeClaim", "delonix-recordings")
        if pvc and pvc["spec"]["accessModes"] == ["ReadWriteOnce"]:
            erros.append(f"[{nome}] gravações ReadWriteOnce com várias réplicas")

    # 8b. nenhum segredo conhecido no render (em claro ou em base64)
    valores = []
    for s in segredos:
        for k, v in (s.get("data") or {}).items():
            try:
                valores.append((s["metadata"]["name"], k, base64.b64decode(v).decode("utf-8", "replace")))
            except Exception:
                erros.append(f"[{nome}] Secret {s['metadata']['name']}.{k} não é base64")
        for k, v in (s.get("stringData") or {}).items():
            valores.append((s["metadata"]["name"], k, str(v)))
    for conhecido in conhecidos:
        if conhecido in bruto or any(conhecido in v for _, _, v in valores):
            erros.append(f"[{nome}] o render contém um segredo de desenvolvimento conhecido ({conhecido[:6]}…)")

    if nome == "production-voz":
        kcm = um(docs, "ConfigMap", "kamailio-cfg")
        with open("voice/kamailio/kamailio.cfg") as f:
            if kcm["data"]["kamailio.cfg"].strip() != f.read().strip():
                erros.append("[production-voz] o kamailio.cfg renderizado difere de voice/kamailio/kamailio.cfg")
        if "198.51.100.20 32 0 pbx-cliente-prova" not in kcm["data"]["address"]:
            erros.append("[production-voz] a allowlist do bordo (address) não leva o tronco dos valores")
        fs = um(docs, "Service", "freeswitch")
        if fs["spec"].get("clusterIP") != "None" or not fs["spec"].get("publishNotReadyAddresses"):
            erros.append("[production-voz] o Service do FreeSWITCH tem de ser headless com publishNotReadyAddresses")
        kam = um(docs, "Deployment", "kamailio")["spec"]["template"]["spec"]
        if kam["containers"][0].get("command") != ["kamailio"]:
            erros.append("[production-voz] o Kamailio precisa de command: [kamailio]")
        if not kam.get("initContainers"):
            erros.append("[production-voz] falta a espera pelo FreeSWITCH antes do Kamailio")
        if um(docs, "Deployment", "pbx-cliente"):
            erros.append("[production-voz] o PBX de laboratório não entra em produção")
        fsd = um(docs, "Deployment", "freeswitch")["spec"]["template"]["spec"]["containers"][0]
        if fsd.get("command") != ["/bin/sh", "/entrypoint/freeswitch-entrypoint.sh"]:
            erros.append("[production-voz] o FreeSWITCH tem de arrancar pelo entrypoint que endurece a vanilla")

# 7. laboratório: aleatórios a sério
def dados(nome):
    return {(s["metadata"]["name"], k): v for s in carregar(nome) if s.get("kind") == "Secret"
            for k, v in (s.get("data") or {}).items()}
a, b = dados("local"), dados("local-2")
if not a:
    erros.append("[local] esperava o Secret gerado")
for chave in a:
    if a[chave] == b.get(chave):
        erros.append(f"[local] {chave[0]}.{chave[1]} é igual em dois renders — é um literal, não um aleatório")

for e in erros:
    print("✗ " + e)
sys.exit(1 if erros else 0)
PYEOF

[ "$fail" = 0 ] && echo "✓ chart helm: lint, recusas e render; portas internas fora do ingress; afinidade por sala; sem segredos literais; sem latest em produção"
exit $fail
