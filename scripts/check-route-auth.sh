#!/usr/bin/env bash
# ============================================================
#  Fitness function: TODA a rota tem autenticação, ou está na lista de
#  públicas com uma razão escrita.
#
#  Existe por causa de um caso concreto: `/api/mls/*` esteve registado com
#  handlers SEM autenticação nenhuma, a responder 201/200/202 com
#  `"status": "delivered"` a qualquer pessoa. Ninguém reparou porque não há
#  middleware de autenticação global — cada handler declara a sua, e um handler
#  que se esqueça fica simplesmente aberto. Foi encontrado por acaso; este
#  portão faz com que não possa voltar por acaso.
#
#  Uso:  bash scripts/check-route-auth.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
exec python3 - <<'PYEOF'
import re, os, sys, glob

MAIN = 'server/src/main.rs'
PUBLICAS = 'scripts/rotas-publicas.txt'

# Autenticação por EXTRACTOR na assinatura.
EXTRACTORES = ('AuthUser', 'ApiKey', 'OdooTokenAuth')
# Autenticação por GUARDA chamada no corpo. Nem tudo pode ser um extractor: a
# API interna da voz autentica por segredo partilhado em header, e isso lê-se
# do `HeaderMap`, não de um tipo.
GUARDAS = ('check_media_secret', 'check_provisioning_secret')

main = open(MAIN).read()

def rotas_de(src, prefixo=''):
    """Todas as rotas declaradas em `src`, com o prefixo de aninhamento."""
    saida = []
    for m in re.finditer(r'\.route\(\s*"([^"]+)"\s*,(.*?)\)\s*(?=[,.\n])', src, re.S):
        saida.append((prefixo + m.group(1), m.group(2)))
    return saida

# Routers ANINHADOS. Sem isto o portão é cego exactamente onde o buraco esteve:
# `/api/mls` foi registado com `.nest("/api/mls", mls::router())`, e a primeira
# versão deste portão aprovou a sua reintrodução sem uma queixa. Um portão que
# não apanha o caso que o originou não é um portão — é decoração.
# Os routers locais (`let auth_routes = Router::new()...`) são declarados no
# próprio main.rs e depois aninhados com prefixo. Retiram-se do texto ANTES de
# varrer, senão as mesmas rotas apareciam duas vezes: uma sem prefixo (errada) e
# outra com (certa).
locais = {}
resto = main
for m in re.finditer(r'let ([a-z_0-9]+) = Router::new\(\)(.*?);\n', main, re.S):
    locais[m.group(1)] = m.group(2)
    resto = resto.replace(m.group(0), f'let {m.group(1)} = Router::new();\n')

pares = rotas_de(resto)
for m in re.finditer(r'\.nest\(\s*"([^"]+)"\s*,\s*([a-z_0-9]+)::router\(\)', main):
    prefixo, mod = m.group(1), m.group(2)
    f = f'server/src/{mod}.rs'
    if not os.path.exists(f):
        print(f'✗ .nest("{prefixo}", {mod}::router()) — módulo não encontrado; o portão não o consegue verificar')
        sys.exit(1)
    pares += rotas_de(open(f).read(), prefixo)
# Routers aninhados construídos em variáveis locais (ex.: `auth_routes`).
for m in re.finditer(r'\.nest\(\s*"([^"]+)"\s*,\s*([a-z_0-9]+)\s*\)', resto):
    prefixo, var = m.group(1), m.group(2)
    if var in locais:
        pares += rotas_de(locais[var], prefixo)
    else:
        print(f'✗ .nest("{prefixo}", {var}) — router desconhecido; o portão não o consegue verificar')
        sys.exit(1)

rotas = []
for caminho, corpo in pares:
    nomeados = [((h.group(2) or '').rstrip(':'), h.group(3))
                for h in re.finditer(r'\b(get|post|put|patch|delete)\(\s*(?:axum::routing::\w+\()?\s*([a-z_0-9]+::)?([a-z_0-9]+)', corpo)]
    if nomeados:
        for mod, h in nomeados:
            rotas.append((caminho, mod, h))
    else:
        # Closure inline: não há corpo nomeado para inspeccionar.
        rotas.append((caminho, '', '<closure>'))

handlers = {}
for f in glob.glob('server/src/*.rs'):
    mod = os.path.basename(f)[:-3]
    src = open(f).read()
    for m in re.finditer(r'(?:pub )?(?:async )?fn ([a-z_0-9]+)\s*\(([^{]*?)\)\s*(?:->[^{]*?)?\{', src, re.S):
        i = m.end()
        handlers.setdefault((mod, m.group(1)), (m.group(2), src[i:i+4000]))

publicas = {}
if os.path.exists(PUBLICAS):
    for linha in open(PUBLICAS):
        if linha.lstrip().startswith('#') or not linha.strip():
            continue
        partes = linha.split(None, 1)
        publicas[partes[0]] = partes[1].strip() if len(partes) > 1 else ''

falhas, invisiveis, usadas = [], [], set()
for caminho, mod, h in sorted(set(rotas)):
    if h == '<closure>':
        (usadas.add(caminho) if caminho in publicas
         else invisiveis.append(f'{caminho} → handler inline (closure)'))
        continue
    par = handlers.get((mod or 'main', h))
    if par is None:
        cand = [v for (m2, h2), v in handlers.items() if h2 == h]
        par = cand[0] if len(cand) == 1 else None
    if par is None:
        invisiveis.append(f'{caminho} → {mod}::{h} (assinatura não encontrada)')
        continue
    args, corpo = par
    autenticado = any(e in args for e in EXTRACTORES) or any(g in corpo for g in GUARDAS)
    if autenticado:
        if caminho in publicas:
            falhas.append(f'{caminho} está em {PUBLICAS} mas JÁ tem autenticação — tira-a de lá')
        continue
    if caminho in publicas:
        usadas.add(caminho)
        continue
    falhas.append(f'{caminho} ({mod}::{h}) SEM autenticação e não declarada pública')

orfas = set(publicas) - usadas
if invisiveis:
    print('✗ rotas que o portão NÃO CONSEGUE verificar — e uma rota que o portão não vê')
    print(f'  é uma rota sem portão. Declara-a em {PUBLICAS} ou dá-lhe um handler nomeado:')
    for s in invisiveis:
        print(f'     {s}')
if falhas:
    print('✗ autorização de rotas:')
    for f in falhas:
        print(f'     {f}')
if orfas:
    print(f'✗ {PUBLICAS} declara rotas que já não existem: {", ".join(sorted(orfas))}')

if falhas or invisiveis or orfas:
    sys.exit(1)
print(f'✓ autorização: {len(set(c for c, _, _ in rotas))} rotas — todas autenticadas ou declaradas públicas com razão')
PYEOF
