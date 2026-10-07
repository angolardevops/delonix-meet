#!/usr/bin/env bash
# ============================================================
#  Fitness function: a observabilidade de produção aponta para coisas REAIS.
#
#  PORQUE EXISTE. Nada do que está aqui dava erro quando estava errado — dava
#  ZERO. Um alerta sobre uma métrica inexistente nunca dispara; um
#  ServiceMonitor com o selector errado não tem alvos; uma NetworkPolicy que
#  abre o namespace errado fecha o scrape. Nos três casos o painel fica verde
#  com o produto em baixo, e foi exactamente o que aconteceu a 2026-10-06:
#  o selector usava etiquetas que o chart não põe, e o endpoint apontava para a
#  porta pública quando o /metrics vive no listener interno.
#
#  O que se mede:
#   1. cada `delonix_*` citado nas regras existe como `# TYPE` no metrics.rs;
#   2. o ServiceMonitor selecciona etiquetas que o chart RENDERIZA, e a porta
#      que ele pede existe no Service seleccionado (precisa de helm);
#   3. o namespace que a NetworkPolicy abre ao Prometheus é o namespace onde a
#      observabilidade se instala.
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

OBS=deploy/k8s/observabilidade
CHART=deploy/helm/delonix-meet
VP=$CHART/values-production.yaml
r=$'\e[31m'; g=$'\e[32m'; y=$'\e[33m'; z=$'\e[0m'
falhas=0
fail() { echo "${r}✗ observabilidade: $1${z}"; falhas=$((falhas + 1)); }

# ---- 1. métricas citadas existem -------------------------------------------
citadas=$(/usr/bin/grep -ohE 'delonix_[a-z0-9_]+' "$OBS"/*.yaml | sort -u)
n=0
for m in $citadas; do
  n=$((n + 1))
  /usr/bin/grep -qE "^ *# TYPE $m " server/src/metrics.rs \
    || fail "a regra cita «$m», que o server/src/metrics.rs não expõe"
done
[ "$n" -gt 0 ] || fail "nenhuma métrica citada nas regras — o ficheiro está vazio?"

# ---- 3. o namespace do scrape bate com o da instalação ----------------------
ns_instala=$(/usr/bin/grep -oP '^NS=\K\S+' "$OBS/instalar.sh" | head -1)
ns_policy=$(python3 - "$VP" <<'PY'
import re, sys
s = open(sys.argv[1]).read()
m = re.search(r'^networkPolicy:(?:\n(?:[ \t].*)?)*', s, re.M)
bloco = m.group(0) if m else ''
mm = re.search(r'^\s+metricsNamespace:\s*"?([\w-]+)"?', bloco, re.M)
print(mm.group(1) if mm else '')
PY
)
if [ -z "$ns_instala" ]; then
  fail "não encontrei NS= no $OBS/instalar.sh"
elif [ "$ns_instala" != "$ns_policy" ]; then
  fail "a observabilidade instala-se em «$ns_instala» mas a NetworkPolicy abre o :8181 a «${ns_policy:-<nada>}» — o scrape fica fechado"
fi

# ---- 2. o ServiceMonitor casa com o que o chart renderiza -------------------
HELM=${HELM:-helm}
command -v "$HELM" >/dev/null 2>&1 || HELM=.tools/bin/helm
if ! command -v "$HELM" >/dev/null 2>&1 && [ ! -x "$HELM" ]; then
  echo "  ${y}· helm ausente — o cruzamento ServiceMonitor↔Service não foi medido${z}"
else
  rendered=$("$HELM" template m "$CHART" -f "$VP" \
    --set secrets.existingSecret=s --set coturn.externalIP=1.2.3.4 \
    --set image.tag=portao 2>/dev/null)
  if [ -z "$rendered" ]; then
    fail "o chart não renderizou — sem render não há cruzamento"
  else
    # Via ficheiro e não por pipe: o heredoc do script OCUPA o stdin, e um
    # `printf | python3 - <<EOF` entrega o heredoc e descarta o pipe (foi o que
    # deu «o render não trouxe Service nenhum» com o render à frente dos olhos).
    tmp=$(mktemp "${TMPDIR:-/tmp}/delonix-obs.XXXXXX")
    trap 'rm -f "$tmp"' EXIT
    printf '%s' "$rendered" > "$tmp"
    python3 - "$OBS/servicemonitor.yaml" "$tmp" <<'PYEOF'
import re, sys

# Sem PyYAML de propósito: o CI não instala nada para os portões, e um portão
# que depende de um pacote que pode não estar é um portão que salta em silêncio.
# O que isto faz é ler blocos pela INDENTAÇÃO, que é o que faltava à primeira
# versão — uma regex sobre «linhas indentadas» apanhava o resto do ficheiro e
# punha `path` e `interval` dentro do selector.

def linhas(txt):
    for l in txt.split('\n'):
        if not l.strip() or l.lstrip().startswith('#'):
            continue
        yield len(l) - len(l.lstrip()), l.strip()

def sub_bloco(txt, chave):
    """As linhas sob `chave:`, só as MAIS indentadas do que ela."""
    out, nivel = [], None
    for ind, t in linhas(txt):
        if nivel is None:
            if t == f'{chave}:' or t.startswith(f'{chave}:') and not t[len(chave) + 1:].strip():
                nivel = ind
            continue
        if ind <= nivel:
            break
        out.append((ind, t))
    return out

def mapa(bloco):
    if not bloco:
        return {}
    base = bloco[0][0]
    return dict(
        (k.strip(), v.strip().strip('"\''))
        for ind, t in bloco if ind == base and ':' in t
        for k, v in [t.split(':', 1)] if v.strip()
    )

sm = open(sys.argv[1]).read()
sel = mapa(sub_bloco(sm, 'matchLabels'))
m = re.search(r'^\s*- port:\s*(\S+)', sm, re.M)
porta = m.group(1).strip('"\'') if m else None
if not sel or not porta:
    print("✗ observabilidade: não consegui ler o selector ou a porta do ServiceMonitor")
    raise SystemExit(1)

servicos = []
for doc in open(sys.argv[2]).read().split('\n---\n'):
    if not re.search(r'^kind: Service$', doc, re.M):
        continue
    nome = re.search(r'^\s*name:\s*(\S+)', doc, re.M)
    labels = mapa(sub_bloco(doc, 'labels'))
    portas = re.findall(r'name:\s*([\w-]+)', '\n'.join(t for _, t in sub_bloco(doc, 'ports')))
    servicos.append((nome.group(1) if nome else '?', labels, portas))

if not servicos:
    print("✗ observabilidade: o render não trouxe Service nenhum")
    raise SystemExit(1)

casam = [x for x in servicos if all(x[1].get(k) == v for k, v in sel.items())]
if not casam:
    print(f"✗ observabilidade: o selector {sel} não casa com Service nenhum do chart.")
    print("     Etiquetas que existem:")
    for nome, labels, _ in servicos:
        print(f"       {nome}: {labels}")
    raise SystemExit(1)
if len(casam) > 1:
    print(f"✗ observabilidade: o selector casa com {len(casam)} Services "
          f"({', '.join(x[0] for x in casam)});")
    print("     os que não servem /metrics ficam como alvos em baixo para sempre.")
    raise SystemExit(1)
nome, _, portas = casam[0]
if porta not in portas:
    print(f"✗ observabilidade: o ServiceMonitor pede a porta «{porta}» e o Service «{nome}» tem {portas}")
    raise SystemExit(1)
print(f"  ✓ ServiceMonitor → Service «{nome}», porta «{porta}»")
PYEOF
    [ $? -eq 0 ] || falhas=$((falhas + 1))
  fi
fi

if [ "$falhas" -gt 0 ]; then
  echo "${r}✗ observabilidade: $falhas problema(s)${z}"
  exit 1
fi
echo "${g}✓ observabilidade: $n métricas citadas existem; o scrape chega ao Service certo; o namespace do Prometheus bate com o da política${z}"
