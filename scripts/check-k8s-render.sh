#!/usr/bin/env bash
# ============================================================
#  Fitness function: os manifestos Kubernetes RENDERIZAM, e o que sai deles
#  respeita as fronteiras do ADR-0001 e do ADR-0006 §3/§4.
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
#   6. enterprise: edição/registo/tenancy, DELONIX_MIGRATE=1, uma réplica;
#   7. em TODOS: todo o Deployment com mais de uma réplica (ou com um HPA a
#      mirá-lo) espalha-se por nó, com ScheduleAnyway. Réplicas todas no mesmo
#      nó ficam `Available` num único ponto de falha, sem dar erro nenhum.
#  10. o `make prod` não aplica os valores COZIDOS nos manifestos — o VIP do
#      kind (`KIND_TURN_VIP`), o domínio .local (`LAB_DOMAIN`) e o contacto do
#      ACME (`PLACEHOLDER_ACME_EMAIL`): todo o manifesto que ele manda ao
#      cluster e que leve um deles passa por um `sed` que o substitui, o alvo
#      depende do `prod-preflight`, e o preflight é CORRIDO para provar que
#      recusa as omissões e aceita valores reais.
#   9. o coturn: o relay TEM caminho (a rede do nó, ou o hairpin autorizado com
#      `--allowed-peer-ip` == `--external-ip`), a gama de relay fica fora dos
#      NodePort e dos portos efémeros quando está na rede do nó, e a
#      `--total-quota` não promete mais alocações do que a gama tem portas. São
#      as mesmas três recusas que o `_helpers.tpl` faz no chart.
#   8. os manifestos OPT-IN de deploy/k8s (os que a kustomization NÃO inclui, e
#      que se aplicam com `kubectl apply -f`) têm recursos, largam as
#      capacidades, não permitem escalada e correm non-root. Nenhum render os
#      cobria, e foi por aí que entraram as lacunas que isto fecha.
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

# ---- 8. os manifestos OPT-IN, que nenhum render cobre ----------------------
#  A kustomization de deploy/k8s inclui 12 dos 17 ficheiros. Os outros aplicam-se
#  à mão (`kubectl apply -f`) e por isso NUNCA passavam por portão nenhum — é
#  assim que o 52-data-plain.yaml andou sem `securityContext` e o 50-data.yaml
#  sem largar capacidades, sem nada acusar.
#
#  Quatro invariantes, e DE PROPÓSITO não exige `livenessProbe`: numa base de
#  dados uma sonda de liveness transforma uma consulta lenta num ciclo de
#  reinícios, e a readiness já a tira do Service. Onde a liveness faz falta é
#  num worker que se pendura — e isso precisa de código no worker, não de YAML.
python3 scripts/k8s-optin-higiene.py deploy/k8s || fail=1

OUT="$OUT" HPA_SRC=deploy/k8s/21-server-hpa.yaml python3 - <<'PYEOF' || fail=1
import os, re, sys, yaml

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
                             "porta interna (8181/9180) exposta por ingress (ADR-0006 §3)")

    # 7. tudo o que tem mais de uma réplica (ou um HPA a mirá-lo) espalha-se
    #    por nó. Sem isto, as 3 réplicas podem aterrar todas no mesmo nó e a
    #    perda desse nó leva o serviço inteiro — e nada dava erro: o Deployment
    #    fica `Available`, com 3 pods, num único ponto de falha.
    #    `ScheduleAnyway`, não `DoNotSchedule`: no laboratório de UM nó (kind) é
    #    um no-op, e o `DoNotSchedule` deixava pods `Pending` sem explicação.
    #    Onde conta é no artefacto de PRODUTO: os overlays saas/enterprise que
    #    um cliente aplica no SEU cluster (ADR-0006 §2).
    alvos_de_hpa = {
        d["spec"]["scaleTargetRef"]["name"]
        for d in docs
        if d.get("kind") == "HorizontalPodAutoscaler"
    }
    for d in (x for x in docs if x.get("kind") == "Deployment"):
        quem = d["metadata"]["name"]
        n = (d.get("spec") or {}).get("replicas", 1)
        tem_hpa = quem in alvos_de_hpa
        if n <= 1 and not tem_hpa:
            continue
        porque = f"{n} réplicas" if n > 1 else "um HPA a mirá-lo"
        tsc = (d["spec"]["template"]["spec"].get("topologySpreadConstraints") or [])
        chaves = {c.get("topologyKey") for c in tsc}
        if "kubernetes.io/hostname" not in chaves:
            erros.append(f"[{nome}] o Deployment {quem} tem {porque} e não se espalha por nó "
                         "(topologySpreadConstraints com topologyKey kubernetes.io/hostname) — "
                         "as réplicas podem aterrar todas no mesmo nó e ficar `Available` "
                         "num único ponto de falha")
            continue
        duros = [c["topologyKey"] for c in tsc if c.get("whenUnsatisfiable") == "DoNotSchedule"]
        if duros:
            erros.append(f"[{nome}] o Deployment {quem} usa DoNotSchedule em {duros} — num cluster "
                         "de um nó (o laboratório) isso deixa pods `Pending` sem explicação; "
                         "usa ScheduleAnyway, como o chart de produção")

    # 9. O RELAY DO COTURN TEM DE TER CAMINHO — as três recusas que o
    #    `_helpers.tpl` já faz no chart, agora também nos manifestos crus.
    #
    #    O TURN tem duas pernas. O cliente fala com a porta de CONTROLO (3478) e
    #    recebe um endereço de RELAY numa porta da gama min-max. Um Service do
    #    Kubernetes só programa regras para as portas que DECLARA, e declarar
    #    centenas de portas num Service não é caminho. Há exactamente DUAS
    #    formas de dar caminho ao relay, e uma delas tem de estar presente:
    #
    #      (a) a REDE DO NÓ (`hostNetwork`), que é o que produção usa desde
    #          2026-10-11 (chart, release `meet`: hostNetwork=true, 20000-20300);
    #      (b) o HAIRPIN autorizado — `--allowed-peer-ip` igual ao
    #          `--external-ip`. Em relay-only dos DOIS lados o «peer» de cada
    #          alocação é o próprio endereço que o coturn anuncia, logo o
    #          relay-a-relay nunca sai do pod e o cliente só precisa da 3478.
    #          É a topologia MEDIDA do laboratório kind (R4: 0% de perda com
    #          dois browsers, 2026-07-11) — e é por isso que esta guarda a
    #          aceita em vez de exigir `hostNetwork` em todo o sítio.
    #
    #    Sem (a) nem (b): o Allocate chega, o coturn anuncia um relay, e os
    #    pacotes de media não têm regra nenhuma por onde entrar. O sintoma é o
    #    pior que há — sinalização a funcionar, sala a abrir, participantes na
    #    lista, e ECRÃ PRETO.
    EFEMEROS = (32768, 60999)   # /proc/sys/net/ipv4/ip_local_port_range, lido nos workers a 2026-10-11
    NODEPORT = (30000, 32767)   # a gama de NodePort do Kubernetes

    def sobrepoe(a, b):
        return a[0] <= b[1] and b[0] <= a[1]

    def flags_do_turnserver(container):
        """Os `--flag=valor` da linha de comando do turnserver, sem as aspas."""
        texto = " ".join(container.get("args") or [])
        return {m.group(1): m.group(2).strip('"')
                for m in re.finditer(r"--([a-z][a-z-]*)=(\S+)", texto)}

    for dep in (d for d in docs
                if d.get("kind") == "Deployment" and d["metadata"]["name"] == "coturn"):
        pspec = dep["spec"]["template"]["spec"]
        cs = [x for x in pspec.get("containers", []) if x["name"] == "coturn"]
        if not cs:
            erros.append(f"[{nome}] o Deployment coturn não tem container `coturn`")
            continue
        f = flags_do_turnserver(cs[0])
        na_rede_do_no = bool(pspec.get("hostNetwork"))
        hairpin = ("allowed-peer-ip" in f
                   and f.get("allowed-peer-ip") == f.get("external-ip"))

        # --- recusa 1: «relay sem rede do nó» -------------------------------
        if not na_rede_do_no and not hairpin:
            erros.append(
                f"[{nome}] coturn: o relay não tem caminho — sem `hostNetwork` e sem hairpin "
                "autorizado (`--allowed-peer-ip` igual a `--external-ip`). O Service só expõe a "
                "porta de controlo e a gama de relay (min-max) não tem regra nenhuma: o Allocate "
                "chega, a media morre, e o sintoma é ecrã preto com a sinalização toda a "
                "funcionar. Dá-lhe a rede do nó (como produção) ou autoriza o hairpin (como o "
                "laboratório kind, R4). Declarar centenas de portas num Service não é caminho")
        #  O hairpin só cobre o relay-a-relay. Se a media NÃO for relay-only
        #  dos dois lados, um cliente atrás de NAT difícil recebe um endereço
        #  de relay que, sem a rede do nó, continua sem regra por onde entrar —
        #  e volta o ecrã preto, agora só para esse cliente. Por isso a via (b)
        #  exige o `FORCE_TURN_RELAY=1` que o ConfigMap da base já leva.
        if not na_rede_do_no and hairpin:
            cm = um(docs, "ConfigMap", "delonix-config")
            if (cm or {}).get("data", {}).get("FORCE_TURN_RELAY") != "1":
                erros.append(
                    f"[{nome}] coturn: o relay vive do hairpin (sem `hostNetwork`) mas o "
                    "FORCE_TURN_RELAY do delonix-config não é \"1\" — o hairpin só cobre o "
                    "relay-a-relay. Sem relay-only dos dois lados, um cliente atrás de NAT "
                    "difícil recebe um endereço de relay sem caminho, e fica com ecrã preto "
                    "só ele. Ou liga o relay-only, ou dá ao coturn a rede do nó")
        if na_rede_do_no and pspec.get("dnsPolicy") != "ClusterFirstWithHostNet":
            erros.append(
                f"[{nome}] coturn: `hostNetwork` sem `dnsPolicy: ClusterFirstWithHostNet` — na rede "
                "do nó o pod herda o resolv.conf do nó e deixa de resolver nomes do cluster")
        if na_rede_do_no:
            svc = um(docs, "Service", "coturn")
            if svc and svc["spec"].get("type") == "LoadBalancer":
                erros.append(
                    f"[{nome}] coturn: na rede do nó, um Service LoadBalancer gasta um endereço do "
                    "MetalLB para nada — o relay já entra pelo endereço do nó. Usa ClusterIP")

        # --- recusa 2: «gama nos portos efémeros» ---------------------------
        #  Só conta na rede do nó: em netns de POD o espaço de portas é privado
        #  do pod e não disputa nada (é a mesma condição do `_helpers.tpl`).
        if "min-port" in f and "max-port" in f:
            gama = (int(f["min-port"]), int(f["max-port"]))
            if na_rede_do_no and (sobrepoe(gama, NODEPORT) or sobrepoe(gama, EFEMEROS)):
                erros.append(
                    f"[{nome}] coturn: a gama de relay {gama[0]}-{gama[1]} entra na dos NodePort "
                    f"({NODEPORT[0]}-{NODEPORT[1]}) e/ou nos portos efémeros do kernel "
                    f"({EFEMEROS[0]}-{EFEMEROS[1]}, lidos nos workers a 2026-10-11). Na rede do nó "
                    "isso disputa portas com o que o nó já abre, e a colisão aparece como uma "
                    "alocação que falha de vez em quando — nunca como um erro no arranque")

            # --- recusa 3: «quota acima das portas» -------------------------
            #  Sem condição de topologia: cada alocação UDP gasta UMA porta.
            portas = gama[1] - gama[0] + 1
            if "total-quota" in f and int(f["total-quota"]) > portas:
                erros.append(
                    f"[{nome}] coturn: --total-quota={f['total-quota']} com {portas} portas de relay "
                    f"({gama[0]}-{gama[1]}): cada alocação UDP gasta uma, logo a quota promete mais "
                    "do que a gama dá. O limite real apareceria como uma alocação recusada sem razão "
                    "visível, não como quota esgotada — alinha os dois")

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

    # 3b. o resgate do QR do Linphone não fica no registo de acessos (R278): o
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
            erros.append("[saas] REDIS_URL vazio ou ausente no delonix-config — obrigatório em SaaS (ADR-0006 §2)")
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

# ---------------------------------------------------------------------------
#  As imagens da plataforma: nem `latest`, nem `bitnami/` sem legacy.
#
#  Medido a 2026-10-07: o `bitnami/*` público deixou de servir tags
#  versionadas (404) e as oficiais do MinIO deixaram de ser puxáveis
#  anonimamente (401). A única via gratuita com tags versionadas é
#  `bitnamilegacy`. E `latest` é mutável — num Redis que guarda presença e
#  sessões, um upgrade não anunciado num reinício de pod tira pessoas de salas.
#
#  Isto é estático e não vai à rede: guarda a ESCOLHA, não a existência.
# ---------------------------------------------------------------------------
img_fail=0
for f in deploy/k8s/plataforma/*-values.yaml; do
  [ -e "$f" ] || continue
  while IFS= read -r linha; do
    case "$linha" in
      *"repository: bitnami/"*)
        echo "✗ $f: «$linha» — o bitnami/* público não serve tags versionadas; usa bitnamilegacy/"; img_fail=1 ;;
    esac
  done < <(/usr/bin/grep -nE '^\s*repository: bitnami/' "$f" 2>/dev/null)
  if /usr/bin/grep -qE '^\s*tag: *"?latest"?' "$f" 2>/dev/null; then
    echo "✗ $f: tag «latest» — é mutável e um upgrade entra num reinício de pod sem aviso"; img_fail=1
  fi
done
if [ "$img_fail" = 0 ]; then
  echo "✓ imagens da plataforma: sem latest e sem bitnami/* público (só bitnamilegacy, com tag fixa)"
else
  fail=1
fi

# ---------------------------------------------------------------------------
#  O `make prod` não pode aplicar valores do laboratório.
#
#  Os manifestos de deploy/k8s levam TRÊS valores cozidos: o VIP MetalLB da
#  rede docker do kind (`KIND_TURN_VIP`), o domínio `.local` (`LAB_DOMAIN`) e o
#  contacto do ACME (`PLACEHOLDER_ACME_EMAIL`). Os dois primeiros são os CERTOS
#  no `make stage`; o terceiro não é de ninguém. O `make prod` aplica os MESMOS
#  ficheiros, e durante muito tempo aplicava-os com os valores lá dentro:
#
#    * com o VIP do kind, o relay de media fica sem caminho — ecrã preto com a
#      sinalização toda a funcionar, o pior sintoma que há;
#    * com o domínio .local, o Ingress responde por um nome que ninguém resolve
#      e o cert-manager pede um certificado que nunca é emitido;
#    * com o contacto do ACME, a conta Let's Encrypt fica registada noutro nome
#      e os avisos de validade vão para a caixa errada — dá-se por isso no dia
#      em que o certificado expira.
#
#  Três exigências. Primeiro, estáticas: o alvo `prod` depende do
#  `prod-preflight`, e todo o manifesto que ele aplique e que contenha um valor
#  de laboratório é substituído por `sed` na própria linha. Depois, uma de
#  COMPORTAMENTO: corre o `prod-preflight` a sério e exige que ele recuse as
#  omissões e aceite valores reais — uma recusa afirmada num grep apodrece, uma
#  recusa corrida não.
# ---------------------------------------------------------------------------
mkvar() { /usr/bin/grep -E "^$1 *:?= *" Makefile | head -1 | sed 's/.*= *//'; }
KIND_TURN_VIP=$(mkvar KIND_TURN_VIP)
LAB_DOMAIN=$(mkvar LAB_DOMAIN)
PLACEHOLDER_ACME_EMAIL=$(mkvar PLACEHOLDER_ACME_EMAIL)
prod_fail=0

for v in KIND_TURN_VIP LAB_DOMAIN PLACEHOLDER_ACME_EMAIL; do
  eval "valor=\$$v"
  if [ -z "$valor" ]; then
    echo "✗ Makefile: falta o $v — é dele que este portão tira o valor cozido"
    prod_fail=1
  fi
done

if ! /usr/bin/grep -qE '^prod: .*prod-preflight' Makefile; then
  echo "✗ Makefile: o alvo prod não depende do prod-preflight — a recusa tem de correr ANTES do primeiro apply"
  prod_fail=1
fi

if [ "$prod_fail" = 0 ]; then
  receita=$(awk '/^prod: /{f=1} f&&/^\t/{print} f&&!/^\t/&&!/^prod: /{exit}' Makefile)
  while IFS= read -r linha; do
    case "$linha" in *deploy/k8s/*) ;; *) continue ;; esac
    for m in $(printf '%s\n' "$linha" | /usr/bin/grep -oE 'deploy/k8s/[A-Za-z0-9_.-]+\.yaml'); do
      [ -f "$m" ] || continue
      for v in KIND_TURN_VIP LAB_DOMAIN PLACEHOLDER_ACME_EMAIL; do
        eval "valor=\$$v"
        /usr/bin/grep -qF "$valor" "$m" || continue
        #  Vale o literal ou a variável: o Makefile usa $(KIND_TURN_VIP)/$(LAB_DOMAIN).
        substitui=0
        case "$linha" in *sed*"$valor"*|*sed*"$v"*) substitui=1 ;; esac
        if [ "$substitui" = 0 ]; then
          echo "✗ Makefile (prod): «$m» leva o valor cozido $v=$valor e é aplicado sem o substituir."
          echo "    Em produção esse valor não serve. Aplica-o com"
          echo "    sed \"s/\$($v)/<o valor real>/g\" $m | kubectl apply -f -"
          prod_fail=1
        fi
      done
    done
  done <<EOF
$receita
EOF
fi

#  A prova de comportamento: o preflight não toca em nada (printf + exit), por
#  isso corre-se aqui sem risco. IP e domínio de exemplo, nunca aplicados.
if [ "$prod_fail" = 0 ]; then
  pf() { make --no-print-directory prod-preflight "$@" >/dev/null 2>&1; }
  #  Os três reais, que servem de base: nenhum é aplicado, o preflight só imprime.
  OK_IP=198.51.100.10; OK_DOM=exemplo.ao; OK_MAIL=ops@exemplo.ao
  if pf; then
    echo "✗ make prod-preflight PASSA sem argumento nenhum — tinha de recusar"; prod_fail=1
  fi
  if pf TURN_PUBLIC_IP="$OK_IP"; then
    echo "✗ make prod-preflight PASSA com o DOMAIN no laboratório ($LAB_DOMAIN) — tinha de recusar"; prod_fail=1
  fi
  if pf DOMAIN="$OK_DOM" ACME_EMAIL="$OK_MAIL"; then
    echo "✗ make prod-preflight PASSA sem TURN_PUBLIC_IP — tinha de recusar"; prod_fail=1
  fi
  if pf TURN_PUBLIC_IP="$KIND_TURN_VIP" DOMAIN="$OK_DOM" ACME_EMAIL="$OK_MAIL"; then
    echo "✗ make prod-preflight PASSA com o VIP do kind ($KIND_TURN_VIP) — tinha de recusar"; prod_fail=1
  fi
  if pf TURN_PUBLIC_IP="$OK_IP" DOMAIN="$LAB_DOMAIN" ACME_EMAIL="$OK_MAIL"; then
    echo "✗ make prod-preflight PASSA com o domínio do laboratório — tinha de recusar"; prod_fail=1
  fi
  if pf TURN_PUBLIC_IP="$OK_IP" DOMAIN="$OK_DOM"; then
    echo "✗ make prod-preflight PASSA sem ACME_EMAIL — tinha de recusar"; prod_fail=1
  fi
  if pf TURN_PUBLIC_IP="$OK_IP" DOMAIN="$OK_DOM" ACME_EMAIL="$PLACEHOLDER_ACME_EMAIL"; then
    echo "✗ make prod-preflight PASSA com o email do manifesto ($PLACEHOLDER_ACME_EMAIL) — tinha de recusar"; prod_fail=1
  fi
  if pf TURN_PUBLIC_IP="$OK_IP" DOMAIN="$OK_DOM" ACME_EMAIL=naoeemail; then
    echo "✗ make prod-preflight PASSA com um ACME_EMAIL que não é email — tinha de recusar"; prod_fail=1
  fi
  if ! pf TURN_PUBLIC_IP="$OK_IP" DOMAIN="$OK_DOM" ACME_EMAIL="$OK_MAIL"; then
    echo "✗ make prod-preflight RECUSA valores reais — a válvula ficou fechada de vez"; prod_fail=1
  fi
fi

if [ "$prod_fail" = 0 ]; then
  echo "✓ make prod: depende do preflight, substitui o VIP, o domínio e o email do ACME, e o preflight recusa as omissões (corrido)"
else
  fail=1
fi

[ "$fail" = 0 ] && echo "✓ manifestos k8s renderizam; portas internas fora do ingress; ADR-0001 intacto nos overlays"
exit $fail
