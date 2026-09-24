#!/usr/bin/env bash
# ============================================================
#  Fitness function: o contrato OpenAPI (ADR-0006 §3).
#
#  1. SPEC GERADO = SPEC COMMITADO. `docs/reference/openapi/{bff,v1}.json` é o
#     que o cliente TypeScript do web e o SDK consomem. Um handler que muda de
#     forma sem o spec mudar no mesmo commit é uma quebra silenciosa para quem
#     gera código a partir dele.
#  2. CATRACA DA COBERTURA. Cada operação (método + caminho) montada no router
#     tem de estar no spec da sua superfície. Hoje há dívida herdada; o número
#     de operações SEM documentação não pode subir, e desce com BLESS.
#     Fora da conta, com razão: WebSockets (/ws, /rtc, directo — o contrato é o
#     `protocol`), sondas e métricas (texto), a API máquina-a-máquina do IVR (o
#     contrato é o .proto) e os próprios /openapi.json.
#
#  Uso:  bash scripts/check-openapi.sh
#        BLESS=1 bash scripts/check-openapi.sh   (regrava specs e baixa a catraca)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

OUT=docs/reference/openapi
mkdir -p "$OUT"
BIN_ARGS=(run --release --quiet --manifest-path server/Cargo.toml --bin delonix-server --)
mkdir -p "$HOME/.cache"
for sup in bff v1 operator integrations; do
  cargo "${BIN_ARGS[@]}" openapi "$sup" 2>/dev/null > "$HOME/.cache/delonix-openapi-$sup.json" \
    || { echo "✗ openapi: não consegui gerar o spec «$sup»"; exit 1; }
done

exec env BLESS="${BLESS:-0}" python3 - "$OUT" <<'PYEOF'
import json, os, re, sys, shutil

OUT = sys.argv[1]
BLESS = os.environ.get('BLESS') == '1'
BASELINE = 'scripts/openapi-baseline.txt'
gerado = {s: os.path.expanduser(f'~/.cache/delonix-openapi-{s}.json') for s in ('bff', 'v1', 'operator', 'integrations')}

falha = False
for sup, path in gerado.items():
    alvo = f'{OUT}/{sup}.json'
    novo = open(path, encoding='utf-8').read()
    velho = open(alvo, encoding='utf-8').read() if os.path.exists(alvo) else None
    if novo != velho:
        if BLESS:
            shutil.copyfile(path, alvo)
            print(f'✓ openapi: {alvo} regravado')
        else:
            print(f'✗ openapi: {alvo} difere do spec gerado — corre BLESS=1 bash scripts/check-openapi.sh e commita o diff')
            falha = True

# ---- cobertura: operações montadas vs documentadas ----
lib = open('server/src/lib.rs', encoding='utf-8').read()
EXCLUIDAS = {'/ws', '/rtc', '/api/rooms/{room_code}/live', '/health', '/ready', '/metrics',
             '/internal/v1/voice/ivr/validate', '/internal/v1/voice/ivr/cdr',
             # Callbacks `mod_xml_curl` do FreeSWITCH (ramais): máquina-a-máquina, por
             # segredo partilhado, respondem XML — o contrato é o do FreeSWITCH.
             '/api/voice/ivr/directory', '/api/voice/ivr/dialplan-did', '/api/voice/ivr/resolve-extension',
             '/api/openapi.json', '/api/v1/openapi.json', '/api/operator/v1/openapi.json',
             '/api/integrations/openapi.json'}

def chamadas_route(src):
    """(caminho, corpo) de cada `.route("…", corpo)`, com o corpo lido por
    parêntesis EQUILIBRADOS. Uma regex preguiçosa parava no primeiro `)` e
    perdia o `.patch(…)` encadeado em `get(a).patch(b)`."""
    for m in re.finditer(r'\.route\(\s*"([^"]+)"\s*,', src):
        i, prof = m.end(), 1
        while i < len(src) and prof:
            prof += {'(': 1, ')': -1}.get(src[i], 0)
            i += 1
        yield m.group(1), src[m.end():i - 1]

def operacoes(src, prefixo=''):
    ops = set()
    for caminho, corpo in chamadas_route(src):
        for verbo in re.findall(r'\b(get|post|put|patch|delete)\(', corpo):
            ops.add((verbo, prefixo + caminho))
    return ops

# Routers locais e aninhados (mesmo tratamento do check-route-auth.sh).
locais, resto = {}, lib
for m in re.finditer(r'let ([a-z_0-9]+) = Router::new\(\)(.*?);\n', lib, re.S):
    locais[m.group(1)] = m.group(2)
    resto = resto.replace(m.group(0), '')
ops = set()
ops |= operacoes(resto)
for m in re.finditer(r'\.nest\(\s*"([^"]+)"\s*,\s*([a-z_0-9]+)\s*\)', resto):
    if m.group(2) in locais:
        ops |= operacoes(locais[m.group(2)], m.group(1))

documentadas = set()
for sup, path in gerado.items():
    spec = json.load(open(path, encoding='utf-8'))
    for caminho, item in spec.get('paths', {}).items():
        for verbo in item:
            documentadas.add((verbo, caminho))

montadas = {(v, c) for v, c in ops if c not in EXCLUIDAS}
sem_doc = sorted(montadas - documentadas)
fantasma = sorted((v, c) for v, c in documentadas - montadas if c not in EXCLUIDAS)
if fantasma:
    print('✗ openapi: o spec documenta operações que o router NÃO monta (contrato falso):')
    for v, c in fantasma:
        print(f'     {v.upper()} {c}')
    falha = True

ref = None
if os.path.exists(BASELINE):
    for l in open(BASELINE):
        if l.startswith('rotas_sem_openapi='):
            ref = int(l.split('=')[1])
n = len(sem_doc)
if BLESS:
    if ref is not None and n > ref:
        print(f'✗ openapi: o BLESS só baixa — operações sem documentação subiram de {ref} para {n}')
        sys.exit(1)
    open(BASELINE, 'w').write('# Operações HTTP montadas sem OpenAPI (scripts/check-openapi.sh). Só desce.\n'
                              f'rotas_sem_openapi={n}\n')
    print(f'✓ openapi: catraca gravada ({n} sem documentação de {len(montadas)})')
elif ref is None:
    print(f'✗ openapi: sem referência em {BASELINE} (medido {n})'); falha = True
elif n > ref:
    print(f'✗ openapi: operações sem documentação subiram de {ref} para {n} — documenta a rota nova:')
    for v, c in sem_doc:
        print(f'     {v.upper()} {c}')
    falha = True
elif n < ref:
    print(f'✗ openapi: operações sem documentação desceram de {ref} para {n} — bom. BLESS=1 bash scripts/check-openapi.sh')
    falha = True

if falha:
    sys.exit(1)
print(f'✓ openapi: specs em dia; {len(montadas) - n}/{len(montadas)} operações documentadas')
PYEOF
