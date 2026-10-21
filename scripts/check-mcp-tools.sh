#!/usr/bin/env bash
# ============================================================
#  Fitness function: toda a tool MCP do catálogo está LIGADA, declarada e
#  EXERCITADA — e nenhuma aceita a organização do cliente.
#
#  Porquê, em três razões concretas (ADR-0027):
#
#  1. `tools/list` é o que um agente lê como CONTRATO. Uma tool anunciada que
#     responde «não implementada» é o relato desonesto que o
#     `check-capability-claims.sh` persegue — aqui com um leitor que não é uma
#     pessoa e não sabe desconfiar.
#  2. O isolamento desta superfície está em NÃO aceitar `org_id`: a organização
#     vem do token, que a trouxe do consentimento. Um `org_id` num esquema de
#     entrada transforma o isolamento numa escolha de quem chama, e é uma
#     linha de JSON que passa numa revisão sem ninguém reparar.
#  3. Uma tool sem prova não protege nada. Mesmo método do inventário de
#     testes ponta-a-ponta (R72) e da cobertura de isolamento (R95): compara-se
#     o que EXISTE com o que se TESTA, e a diferença é a lista.
#
#  Uso:  bash scripts/check-mcp-tools.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
exec python3 - <<'PYEOF'
import re, sys

CATALOGO = 'server/crates/delonix-meet-domain/src/integration/mcp.rs'
ADAPTADOR = 'server/src/mcp.rs'
PROVA = 'server/tests/mcp_tools.rs'

falha = False


def ler(p):
    try:
        return open(p, encoding='utf-8').read()
    except FileNotFoundError:
        print(f'✗ mcp: {p} não existe — o portão ficaria cego')
        sys.exit(1)


catalogo = ler(CATALOGO)
adaptador = ler(ADAPTADOR)
prova = ler(PROVA)

# Cada entrada `Tool { name: "x", … }` do catálogo, com o seu bloco.
entradas = re.findall(r'Tool \{(.*?)\n        \}', catalogo, re.S)
if not entradas:
    print(f'✗ mcp: não encontrei nenhuma `Tool {{ … }}` em {CATALOGO} — o portão ficou cego')
    sys.exit(1)

nomes = []
for bloco in entradas:
    m = re.search(r'name:\s*"([a-z0-9_]+)"', bloco)
    if not m:
        print(f'✗ mcp: uma entrada do catálogo não tem `name`: {bloco[:80]}…')
        falha = True
        continue
    nome = m.group(1)
    nomes.append(nome)
    # (a) escopo, resumo e esquema declarados.
    for campo in ('scope', 'summary', 'input_schema', 'read_only', 'capability'):
        if not re.search(rf'\b{campo}:', bloco):
            print(f'✗ mcp: a tool `{nome}` não declara `{campo}`')
            falha = True

# (b) a organização e a pessoa NUNCA vêm do cliente.
for proibido in ('org_id', 'user_id', 'organization_id', 'owner_id'):
    for m in re.finditer(rf'const ARGS[A-Z_]*: &str = r#"(.*?)"#', catalogo, re.S):
        if proibido in m.group(1):
            print(f'✗ mcp: um esquema de entrada declara `{proibido}` — a organização vem do token (ADR-0027 §4)')
            falha = True
    if re.search(rf'input_schema:.*{proibido}', catalogo):
        print(f'✗ mcp: um `input_schema` inline declara `{proibido}`')
        falha = True

# O adaptador SEM comentários: a verificação (e) procura SQL e prompts no
# código, e uma frase que os nomeie num comentário não é nenhum dos dois.
codigo = re.sub(r'^\s*//.*$', '', adaptador, flags=re.M)

# (c) ligada: o nome aparece como braço do `match tool.name` dentro de
# `executar` — e só desse `match`. Os braços de `dispatch` são métodos do
# JSON-RPC (`initialize`, `tools/list`), não tools.
m = re.search(r'async fn executar\(.*?\n\}\n', adaptador, re.S)
if not m:
    print(f'✗ mcp: não encontrei a `async fn executar` em {ADAPTADOR} — o portão ficou cego')
    sys.exit(1)
braços = set(re.findall(r'"([a-z0-9_]+)" =>', m.group(0)))
for nome in nomes:
    if nome not in braços:
        print(f'✗ mcp: a tool `{nome}` está no catálogo e NÃO está ligada em {ADAPTADOR}')
        falha = True
for b in sorted(braços - set(nomes)):
    print(f'✗ mcp: {ADAPTADOR} liga `{b}`, que não está no catálogo')
    falha = True

# (d) exercitada: o nome aparece na prova de integração.
for nome in nomes:
    if f'"{nome}"' not in prova:
        print(f'✗ mcp: a tool `{nome}` não é exercitada por {PROVA}')
        falha = True

# (f) a DESCOBERTA tem de ser encaminhada para a API em todos os sítios que
#     encaminham tráfego. Este é o defeito que fundiu sem ninguém ver: as rotas
#     `/.well-known/oauth-*` existiam no router, e nem o nginx, nem a HTTPRoute,
#     nem o Ingress, nem o dev server as levavam ao servidor — o documento caía
#     na regra do `/` e era servido pela WEB. Um cliente MCP recebia HTML e
#     nunca encontrava o servidor de autorização. Nada no render o dizia,
#     porque todos os objectos eram válidos.
PRE = '/.well-known/oauth-'
ENCAMINHADORES = [
    ('deploy/k8s/nginx.conf', 'o nginx da imagem web'),
    ('deploy/nginx-delonix.conf', 'o nginx do deploy clássico'),
    ('deploy/compose/edge.conf', 'o nginx do compose'),
    ('deploy/helm/delonix-meet/templates/gateway.yaml', 'a HTTPRoute do chart'),
    ('deploy/helm/delonix-meet/templates/ingress.yaml', 'o Ingress do chart'),
    ('web/vite.config.ts', 'o dev server'),
]
for caminho, o_que in ENCAMINHADORES:
    try:
        texto = open(caminho, encoding='utf-8').read()
    except FileNotFoundError:
        continue
    if PRE not in texto:
        print(f'✗ mcp: {o_que} ({caminho}) não encaminha `{PRE}*` para a API — '
              'a descoberta do OAuth cai na regra do `/` e é servida pela web')
        falha = True
# E o contrário: levar o `/.well-known/` INTEIRO roubaria o acme-challenge ao
# cert-manager e partia a emissão de certificados por HTTP-01.
import re
for caminho, o_que in ENCAMINHADORES:
    try:
        texto = open(caminho, encoding='utf-8').read()
    except FileNotFoundError:
        continue
    for linha in texto.splitlines():
        nua = linha.split('#')[0]
        if '/.well-known/' in nua and 'oauth-' not in nua:
            print(f'✗ mcp: {o_que} ({caminho}) encaminha `/.well-known/` inteiro: '
                  f'isso rouba o acme-challenge ao cert-manager — «{linha.strip()[:90]}»')
            falha = True

# (e) o adaptador é um ADAPTADOR: sem SQL e sem prompts.
for padrao, o_que in ((r'sqlx::query', 'uma consulta SQL'), (r'\bprompt\b', 'um prompt de LLM')):
    if re.search(padrao, codigo):
        print(f'✗ mcp: {ADAPTADOR} tem {o_que} — uma tool chama o handler do BFF, não a base nem o modelo')
        falha = True

if not falha:
    print(f'✓ mcp: {len(nomes)} tools — todas declaradas, ligadas e exercitadas; nenhuma aceita a '
          'organização do cliente; e a descoberta do OAuth é encaminhada nos seis sítios')
sys.exit(1 if falha else 0)
PYEOF
