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
#   7. produção: a quota do namespace cobre o pico COM ROLLOUT do chart MAIS a
#      plataforma que vive no mesmo namespace (Postgres e Redis, lidos de
#      deploy/k8s/plataforma) — requests, limits, pods e PVCs — e o `max` do
#      LimitRange não rejeita nenhum deles (ADR-0021, D1 de 2026-10-10);
#   7b. a prioridade de agendamento é opcional (ninguém FIXA uma classe) e, quando
#      dada, chega a TODOS os workloads — incluindo o Job de migração, que bloqueia
#      o release;
#   7c. os nomes de classe que os valores usam estão DECLARADOS em
#      deploy/k8s/plataforma/priorityclasses.yaml, e nenhuma classe é o default do
#      cluster (um globalDefault: true mudava a prioridade dos pods de outras
#      equipas);
#   8. laboratório: os segredos gerados são aleatórios (dois renders diferem);
#   9. nenhum segredo de desenvolvimento conhecido (deploy/k8s/01-config.yaml,
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
  if "$HELM" template meet "$CHART" -n ngolacloud-meet "$@" >"$OUT/recusa.out" 2>"$OUT/recusa.err"; then
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
recusa "ESL com o FreeSWITCH em hostNetwork e sem redes declaradas" "voice.freeswitch.eslCidrs: com server.telephony.eslAddr e o FreeSWITCH em hostNetwork" \
  "${VOZ[@]}" --set server.telephony.eslAddr=freeswitch.ngolacloud-meet.svc.cluster.local:8021 --set voice.freeswitch.hostNetwork=true
recusa "ESL em produção sem política de rede nem redes declaradas" "server.telephony.eslAddr em produção" \
  "${VOZ[@]}" --set server.telephony.eslAddr=freeswitch.ngolacloud-meet.svc.cluster.local:8021 --set networkPolicy.enabled=false

# ---- 4. render --------------------------------------------------------
render() { # nome, args…
  local nome=$1; shift
  if "$HELM" template meet "$CHART" -n ngolacloud-meet "$@" >"$OUT/$nome.yaml" 2>"$OUT/$nome.err"; then
    echo "  ✓ render $nome ($(grep -c '^kind:' "$OUT/$nome.yaml") recursos)"
  else
    erro "render falhou: $nome"; grep -v 'found symbolic link' "$OUT/$nome.err" | sed 's/^/    /' | head -20
  fi
}
render production "${PROD[@]}"
render production-voz "${VOZ[@]}"
render production-voz-esl "${VOZ[@]}" --set server.telephony.eslAddr=freeswitch.ngolacloud-meet.svc.cluster.local:8021
render production-voz-esl-cidrs "${VOZ[@]}" --set server.telephony.eslAddr=freeswitch.ngolacloud-meet.svc.cluster.local:8021 \
  --set 'voice.freeswitch.eslCidrs={10.244.0.0/16}'
render local "${LOCAL[@]}"
render local-2 "${LOCAL[@]}"
# Dois renders dedicados ao cruzamento 6d. Não se reaproveita o de produção:
# desde que o values-production.yaml passou a LIGAR as classes, ele já não
# serve de controlo negativo (o portão apanhou-me nisso mesmo).
#
#   sem-prioridade: todas as chaves VAZIAS → ninguém pode renderizar uma;
#   prioridade:     uma só chave, sem overrides → todos têm de a levar.
LIMPA=(--set priorityClassName= --set server.priorityClassName=
       --set coturn.priorityClassName= --set postgresql.priorityClassName=
       --set redis.priorityClassName= --set voice.freeswitch.priorityClassName=
       --set voice.kamailio.priorityClassName= --set voice.labPbx.priorityClassName=
       --set web.priorityClassName=)
render sem-prioridade "${VOZ[@]}" "${LIMPA[@]}"
render prioridade "${VOZ[@]}" "${LIMPA[@]}" --set priorityClassName=prova-prioridade

# --- Gateway API (ADR-0021) ------------------------------------------------
# O chart ganhou um segundo caminho de entrada. Estas recusas existem porque
# cada uma delas, se passasse, perdia algo em silêncio.
GW=("${PROD[@]}" --set ingress.enabled=false --set gateway.enabled=true
    --set gateway.className=delonix --set gateway.tls.clusterIssuer=delonix-letsencrypt
    --set gateway.acceptProvisioningInAccessLog=true)

recusa "ingress e gateway ao mesmo tempo" "são exclusivos" \
  "${PROD[@]}" --set gateway.enabled=true --set gateway.className=delonix
recusa "gateway em mode=own sem GatewayClass" "gateway.className" \
  "${PROD[@]}" --set ingress.enabled=false --set gateway.enabled=true
recusa "gateway em produção sem TLS" "gateway.tls:" \
  "${PROD[@]}" --set ingress.enabled=false --set gateway.enabled=true \
  --set gateway.className=delonix --set gateway.acceptProvisioningInAccessLog=true
recusa "gateway sem afinidade por sala em produção (R3)" "regressão R3" \
  "${GW[@]}" --set gateway.affinity.implementation=none
recusa "gateway sem reconhecer o bilhete no access log (R278)" "acceptProvisioningInAccessLog" \
  "${PROD[@]}" --set ingress.enabled=false --set gateway.enabled=true \
  --set gateway.className=delonix --set gateway.tls.clusterIssuer=x
recusa "nem ingress nem gateway em produção" "sem um deles o host não é servido" \
  "${PROD[@]}" --set ingress.enabled=false
recusa "gateway em mode=attach sem parentRef" "gateway.parentRef.name" \
  "${GW[@]}" --set gateway.mode=attach

render production-gateway "${GW[@]}"

# A invariante do ADR-0001 no caminho NOVO. No Ingress era a anotação
# `upstream-hash-by: $arg_room`; aqui é uma BackendTrafficPolicy com
# ConsistentHash por parâmetro de query. Se isto se perder, os pares de uma
# sala caem em pods diferentes e o SFU em memória parte-se — sem um erro.
python3 - "$OUT/production-gateway.yaml" <<'PYGW' || fail=1
import sys, yaml
docs = [d for d in yaml.safe_load_all(open(sys.argv[1], encoding='utf-8')) if d]
erros = []
pol = [d for d in docs if d.get('kind') == 'BackendTrafficPolicy']
rotas = {d['metadata']['name']: d for d in docs if d.get('kind') == 'HTTPRoute'}

if not pol:
    erros.append("R3: sem BackendTrafficPolicy — não há hash(room)→mesmo pod")
else:
    ch = (pol[0].get('spec', {}).get('loadBalancer') or {}).get('consistentHash') or {}
    if ch.get('type') != 'QueryParams':
        erros.append(f"R3: consistentHash.type={ch.get('type')}, esperado QueryParams")
    if [q.get('name') for q in ch.get('queryParams') or []] != ['room']:
        erros.append(f"R3: o hash não é por `room`: {ch.get('queryParams')}")
    alvos = [(t.get('kind'), t.get('name')) for t in pol[0]['spec'].get('targetRefs') or []]
    if ('HTTPRoute', 'delonix-ws') not in alvos:
        erros.append(f"R3: a política não se cola à rota do /ws: {alvos}")

ws = rotas.get('delonix-ws')
if not ws:
    erros.append("R3: falta a HTTPRoute dedicada ao /ws")
else:
    destinos = [b.get('name') for r in ws['spec']['rules'] for b in r.get('backendRefs') or []]
    if destinos != ['delonix-server-ws']:
        erros.append(f"R3: o /ws não usa o Service dedicado: {destinos}")

ger = rotas.get('delonix-meet')
if ger:
    caminhos = [m['path']['value'] for r in ger['spec']['rules'] for m in r.get('matches') or []]
    if '/ws' in caminhos:
        erros.append("R3: o /ws entrou na rota geral — a política de afinidade não se lhe aplica")
    if '/api/public/extension-provisioning' not in caminhos:
        erros.append("R278: falta o caminho do resgate do QR")
    # ADR-0006 §3: as portas internas nunca são servidas de fora
    portas = [b.get('port') for r in ger['spec']['rules'] for b in r.get('backendRefs') or []]
    for proibida in (8181, 9180):
        if proibida in portas:
            erros.append(f"ADR-0006 §3: a porta interna {proibida} está exposta na rota")

for e in erros:
    print("✗ " + e)
sys.exit(1 if erros else 0)
PYGW
[ "$fail" = 0 ] && echo "  ✓ Gateway API: afinidade por sala por ConsistentHash(room), /ws em Service dedicado, portas internas fora"

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

    # 5b. o resgate do QR do Linphone não fica no registo de acessos (R278): o
    #    caminho leva o bilhete, e só o Ingress dedicado, com o registo
    #    desligado, o pode servir.
    qr_ok = False
    for ing in (d for d in docs if d.get("kind") == "Ingress"):
        ann = (ing["metadata"].get("annotations") or {})
        for path, b in backends(ing):
            if path == "/api/public/extension-provisioning":
                svc, _ = svc_ref(b)
                qr_ok = (svc == "delonix-server"
                         and ann.get("nginx.ingress.kubernetes.io/enable-access-log") == "false")
    if not qr_ok:
        erros.append(f"[{nome}] /api/public/extension-provisioning não tem Ingress próprio com "
                     "enable-access-log:false — o bilhete do QR ficava no registo do ingress (R278)")

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
        # O arranque copia ficheiros de /meet com `set -e`: um que o ConfigMap
        # não traga deixa o pod em CrashLoop, e só se via no cluster (R291). O
        # compose e o cluster local têm a prova com chamadas; o chart tem esta.
        with open("voice/cluster/freeswitch-entrypoint.sh") as f:
            copiados = set(re.findall(r'"\$MEET/([^"]+)"', f.read()))
        meet = um(docs, "ConfigMap", "freeswitch-meet")
        em_falta = sorted(copiados - set((meet or {}).get("data") or {}))
        if not copiados:
            erros.append("[production-voz] não encontrei no freeswitch-entrypoint.sh os ficheiros que ele copia de /meet")
        elif em_falta:
            erros.append("[production-voz] o arranque copia de /meet ficheiros que o ConfigMap freeswitch-meet não traz: "
                         + ", ".join(em_falta))

# 6b. o ESL (R300): fechado por omissão; aberto, com a password do Secret e só aos pods do servidor
def env_do_fs(nome):
    c = um(carregar(nome), "Deployment", "freeswitch")["spec"]["template"]["spec"]["containers"][0]
    return {e["name"]: e for e in c.get("env") or []}
fechado, aberto = env_do_fs("production-voz"), env_do_fs("production-voz-esl")
if "TELEPHONY_ESL_PASSWORD" in fechado or "DELONIX_ESL_CIDRS" in fechado:
    erros.append("[production-voz] o FreeSWITCH recebe a configuração do ESL sem o servidor a ter pedido")
if um(carregar("production-voz"), "NetworkPolicy", "freeswitch-esl"):
    erros.append("[production-voz] há uma política de rede do ESL sem ESL")
if "DELONIX_ESL_CIDRS" in aberto:
    erros.append("[production-voz-esl] o FreeSWITCH recebe DELONIX_ESL_CIDRS sem `voice.freeswitch.eslCidrs`")
if env_do_fs("production-voz-esl-cidrs").get("DELONIX_ESL_CIDRS", {}).get("value") != "10.244.0.0/16":
    erros.append("[production-voz-esl-cidrs] o FreeSWITCH não recebe as redes de onde o servidor fala (DELONIX_ESL_CIDRS)")
if (aberto.get("TELEPHONY_ESL_PASSWORD", {}).get("valueFrom") or {}).get("secretKeyRef", {}).get("key") != "TELEPHONY_ESL_PASSWORD":
    erros.append("[production-voz-esl] a password do ESL do FreeSWITCH não vem do Secret")
pol = um(carregar("production-voz-esl"), "NetworkPolicy", "freeswitch-esl")
if not pol:
    erros.append("[production-voz-esl] falta a política de rede que fecha o 8021 ao servidor")
else:
    regras = pol["spec"]["ingress"]
    do_esl = [r for r in regras if any(p.get("port") == 8021 for p in r.get("ports") or [])]
    if len(do_esl) != 1 or not do_esl[0].get("from") or \
       do_esl[0]["from"][0].get("podSelector", {}).get("matchLabels", {}).get("app") != "delonix-server":
        erros.append("[production-voz-esl] o 8021 tem de estar aberto SÓ aos pods do servidor")
    abertas = [(p.get("protocol"), p.get("port"), p.get("endPort")) for r in regras if not r.get("from") for p in r.get("ports") or []]
    if any(a <= 8021 <= (b or a) for proto, a, b in abertas if proto == "TCP"):
        erros.append("[production-voz-esl] uma regra sem origem cobre o 8021: a política não fecha nada")

# 6c. a quota do namespace cobre o pico COM ROLLOUT (ADR-0021: cluster
#     partilhado). Recalcula a conta do topo de templates/quota.yaml a partir
#     do render — um comentário que ninguém recalcula não é uma garantia.
def _cpu(v):
    v = str(v)
    return float(v[:-1]) / 1000 if v.endswith("m") else float(v)

def _mem(v):
    v = str(v)
    for suf, mult in (("Ki", 2**10), ("Mi", 2**20), ("Gi", 2**30), ("Ti", 2**40),
                      ("K", 1e3), ("M", 1e6), ("G", 1e9)):
        if v.endswith(suf):
            return float(v[: -len(suf)]) * mult
    return float(v)

def _surge(d, n):
    """Pods a mais durante um rollout. `Recreate` não soma: derruba antes de subir."""
    est = (d.get("spec") or {}).get("strategy") or {}
    if d["kind"] != "Deployment" or est.get("type") == "Recreate":
        return 0
    ms = (est.get("rollingUpdate") or {}).get("maxSurge", "25%")
    if isinstance(ms, str) and ms.endswith("%"):
        import math
        return math.ceil(n * float(ms[:-1]) / 100)
    return int(ms)

docs = carregar("production-voz")
quota = um(docs, "ResourceQuota", "delonix-meet-quota")
faixa = um(docs, "LimitRange", "delonix-meet-limits")
if not quota:
    erros.append("[production-voz] falta o ResourceQuota do namespace — em cluster partilhado "
                 "um pico do Meet come o que é dos vizinhos (ADR-0021)")
if not faixa:
    erros.append("[production-voz] falta o LimitRange do namespace")
if quota and faixa:
    hpa = {d["spec"]["scaleTargetRef"]["name"]: d["spec"]["maxReplicas"]
           for d in docs if d.get("kind") == "HorizontalPodAutoscaler"}
    pico = {"cpu": 0.0, "mem": 0.0, "req_cpu": 0.0, "req_mem": 0.0, "pods": 0, "pvcs": 0}
    maior = {"cpu": 0.0, "mem": 0.0}
    for d, spec in pod_specs(docs):
        n = 1 if d["kind"] == "Job" else hpa.get(d["metadata"]["name"],
                                                 (d.get("spec") or {}).get("replicas", 1))
        if d["kind"] == "StatefulSet":
            pico["pvcs"] += len(d["spec"].get("volumeClaimTemplates") or []) * n
        n += _surge(d, n)
        pico["pods"] += n
        for ct in spec.get("containers", []) + spec.get("initContainers", []):
            res = ct.get("resources") or {}
            lm, rq = res.get("limits") or {}, res.get("requests") or {}
            c, m = _cpu(lm.get("cpu", 0)), _mem(lm.get("memory", 0))
            pico["cpu"] += c * n
            pico["mem"] += m * n
            pico["req_cpu"] += _cpu(rq.get("cpu", 0)) * n
            pico["req_mem"] += _mem(rq.get("memory", 0)) * n
            maior["cpu"], maior["mem"] = max(maior["cpu"], c), max(maior["mem"], m)
    pico["pvcs"] += sum(1 for d in docs if d.get("kind") == "PersistentVolumeClaim")

    # A PLATAFORMA no mesmo namespace (D1, 2026-10-10). O `instalar.sh` de
    # deploy/k8s/plataforma põe o Postgres (CNPG) e o Redis no namespace do
    # Meet, fora do chart — e o Kubernetes aplica a quota a TODOS os pods do
    # namespace. A quota do chart que só contava o chart deixava 8Gi de
    # requests para um Postgres que pede 12Gi sozinho: recusado no primeiro
    # `helm upgrade`. Lê-se dos próprios ficheiros, não de números copiados,
    # para que mudar a plataforma refaça esta conta.
    base = "deploy/k8s/plataforma"
    def _soma(res, n, quem):
        if not (res.get("requests") and res.get("limits")):
            erros.append(f"[plataforma] {quem} sem requests/limits — um preset do chart externo "
                         "não é medível aqui; declara-os")
            return
        pico["req_cpu"] += _cpu(res["requests"].get("cpu", 0)) * n
        pico["req_mem"] += _mem(res["requests"].get("memory", 0)) * n
        c, m = _cpu(res["limits"].get("cpu", 0)), _mem(res["limits"].get("memory", 0))
        pico["cpu"] += c * n
        pico["mem"] += m * n
        maior["cpu"], maior["mem"] = max(maior["cpu"], c), max(maior["mem"], m)
    pg = next(d for d in yaml.safe_load_all(open(f"{base}/cnpg-cluster.yaml"))
              if d and d.get("kind") == "Cluster")
    n = pg["spec"]["instances"]
    # Sem surge: o CNPG actualiza instância a instância, no lugar. O Job de
    # initdb/join corre ANTES da instância que cria, nunca ao lado de todas.
    _soma(pg["spec"].get("resources") or {}, n, "o Postgres (cnpg-cluster.yaml)")
    pico["pods"] += n
    pico["pvcs"] += n * (2 if pg["spec"].get("walStorage") else 1)
    rv = yaml.safe_load(open(f"{base}/redis-values.yaml"))
    if not (rv.get("architecture") == "replication" and (rv.get("sentinel") or {}).get("enabled")):
        erros.append("[plataforma] a conta do Redis assume replication + sentinel (um StatefulSet "
                     "de replica.replicaCount pods) — o redis-values.yaml mudou de forma")
    else:
        n = rv["replica"]["replicaCount"]
        _soma(rv["replica"].get("resources") or {}, n, "o Redis (replica)")
        _soma(rv["sentinel"].get("resources") or {}, n, "o sentinel do Redis")
        if (rv.get("metrics") or {}).get("enabled"):
            _soma(rv["metrics"].get("resources") or {}, n, "o exporter do Redis")
        pico["pods"] += n
        if (rv["replica"].get("persistence") or {}).get("enabled"):
            pico["pvcs"] += n

    dura = quota["spec"]["hard"]
    tecto = {"cpu": _cpu(dura["limits.cpu"]), "mem": _mem(dura["limits.memory"]),
             "req_cpu": _cpu(dura["requests.cpu"]), "req_mem": _mem(dura["requests.memory"]),
             "pods": float(dura["pods"]), "pvcs": float(dura["persistentvolumeclaims"])}
    for eixo, unidade, div in (("cpu", "CPU de limits", 1), ("mem", "Gi de limits", 2**30),
                               ("req_cpu", "CPU de requests", 1), ("req_mem", "Gi de requests", 2**30),
                               ("pods", "pods", 1), ("pvcs", "PVCs", 1)):
        if tecto[eixo] < pico[eixo]:
            erros.append(f"[production-voz] a quota ({tecto[eixo]/div:.2f} {unidade}) "
                         f"não cobre o pico COM ROLLOUT do chart + plataforma "
                         f"({pico[eixo]/div:.2f} {unidade}) — refaz a conta no topo de "
                         "templates/quota.yaml")
    if os.environ.get("QUOTA_CONTA"):
        print("  conta da quota (chart + plataforma): " + ", ".join(
            f"{k}={v/(2**30) if 'mem' in k else v:.2f}" for k, v in pico.items()))
    # O `max` por contentor não pode rejeitar os nossos próprios pods.
    lim = faixa["spec"]["limits"][0]
    for eixo, chave, unidade, div in (("cpu", "cpu", "CPU", 1), ("mem", "memory", "Gi", 2**30)):
        f = _cpu(lim["max"][chave]) if eixo == "cpu" else _mem(lim["max"][chave])
        if f < maior[eixo]:
            erros.append(f"[production-voz] o LimitRange max.{chave} ({f/div:.2f} {unidade}) é menor "
                         f"que o maior contentor do namespace ({maior[eixo]/div:.2f} {unidade}) — a "
                         "admissão rejeitava o nosso próprio pod (ou o Postgres da plataforma)")
    # O default/defaultRequest é o que torna a quota aplicável. Tem de estar lá
    # E ser uma quantidade > 0: um `cpu: ""` renderiza, mas o API server
    # recusa-o — e só se via no cluster.
    for campo in ("default", "defaultRequest"):
        bloco = lim.get(campo) or {}
        if not bloco:
            erros.append(f"[production-voz] o LimitRange sem {campo} não torna a quota aplicável: "
                         "um pod sem `resources` não consome requests nenhum")
            continue
        for chave, ler in (("cpu", _cpu), ("memory", _mem)):
            try:
                q = ler(bloco[chave])
            except (KeyError, ValueError):
                erros.append(f"[production-voz] LimitRange {campo}.{chave} = "
                             f"{bloco.get(chave, '<ausente>')!r} não é uma quantidade")
                continue
            if q <= 0:
                erros.append(f"[production-voz] LimitRange {campo}.{chave} é {q} — "
                             "um default de zero é o mesmo que não ter default")

# a quota é só de produção: num laboratório de um nó seria um tecto inventado
if um(carregar("local"), "ResourceQuota", "delonix-meet-quota"):
    erros.append("[local] o laboratório não leva quota — os números são a escala de produção")

# 6d. a prioridade de agendamento é uma MAÇANETA, e chega a todos
#     Num cluster partilhado (ADR-0021) o `priorityClassName` decide quem é
#     desalojado quando um nó aperta. O chart NÃO pode fixar um nome — um
#     `priorityClassName` que não exista faz o API server recusar o pod — logo
#     é opcional. Duas coisas têm de valer:
#
#       sem o valor: NINGUÉM renderiza uma prioridade (ninguém a fixou);
#       com o valor: TODOS a renderizam (ninguém se esqueceu de a ligar num
#                    workload novo — é esta a metade que apodrece sozinha).
sem = carregar("sem-prioridade")
fixos = [f"{d['kind']}/{d['metadata']['name']}"
         for d, spec in pod_specs(sem) if spec.get("priorityClassName")]
if fixos:
    erros.append("[sem-prioridade] com TODAS as chaves de prioridade vazias, estes workloads "
                 f"renderizam uma: {', '.join(fixos)} — o chart não pode FIXAR um nome de "
                 "classe, porque uma que não exista no cluster faz o API server recusar o pod")

com = carregar("prioridade")
faltam = [f"{d['kind']}/{d['metadata']['name']}"
          for d, spec in pod_specs(com) if spec.get("priorityClassName") != "prova-prioridade"]
if faltam:
    erros.append("[prioridade] com `priorityClassName=prova-prioridade` nos valores, estes "
                 f"workloads NÃO a levam: {', '.join(faltam)} — a maçaneta não chega a todos, "
                 "e num nó apertado eles são desalojados antes dos outros")
elif not list(pod_specs(com)):
    erros.append("[prioridade] o render não trouxe workload nenhum — o portão deixou de medir")

# 6e. os nomes de classe que os valores usam EXISTEM, e nenhuma classe é o
#     default do cluster.
#     Um `priorityClassName` que não exista faz o API server RECUSAR o pod: o
#     `helm upgrade` falha, não avisa. E uma PriorityClass com
#     `globalDefault: true` passa a ser a prioridade de TODOS os pods do
#     cluster que não declarem uma — incluindo os de outras equipas, noutros
#     namespaces. Num cluster partilhado (ADR-0021) é o mesmo dano que a quota
#     evita, pela porta oposta.
CLASSES = "deploy/k8s/plataforma/priorityclasses.yaml"
usados = set()
for nome in ("production", "production-voz"):
    for d, spec in pod_specs(carregar(nome)):
        if spec.get("priorityClassName"):
            usados.add(spec["priorityClassName"])

try:
    with open(CLASSES) as f:
        declaradas = {d["metadata"]["name"]: d for d in yaml.safe_load_all(f)
                      if d and d.get("kind") == "PriorityClass"}
except FileNotFoundError:
    declaradas = {}
    if usados:
        erros.append(f"os valores usam as classes {sorted(usados)} e o {CLASSES} não existe — "
                     "o API server recusaria os pods")

for nome_classe in sorted(usados - set(declaradas)):
    erros.append(f"os valores do chart usam a classe «{nome_classe}» e o {CLASSES} não a declara "
                 "— um priorityClassName que não exista faz o API server RECUSAR o pod, e o "
                 "`helm upgrade` falha")
for nome_classe, d in sorted(declaradas.items()):
    if d.get("globalDefault"):
        erros.append(f"{CLASSES}: a classe «{nome_classe}» tem globalDefault: true — passaria a "
                     "ser a prioridade de TODOS os pods do cluster que não declarem uma, "
                     "incluindo os de outras equipas (ADR-0021: o cluster é partilhado)")
if declaradas and not usados:
    erros.append(f"o {CLASSES} declara {sorted(declaradas)} e nenhum valor do chart as usa — "
                 "classes que ninguém pede não protegem nada")

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
