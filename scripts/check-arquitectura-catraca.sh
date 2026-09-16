#!/usr/bin/env bash
# ============================================================
#  Fitness function: catraca da arquitectura do backend (ADR-0004 §5).
#
#  Porquê: a auditoria de 2026-09-16 mediu que as cópias de uma regra
#  DIVERGEM — e três das divergências eram falhas de segurança. A
#  verificação de pertença à org escrita à mão esquecia `archived_at`
#  (funcionário arquivado mantinha acesso); o «criar utilizador por email»
#  existia em 6 cópias e só 2 recusavam a conta de outra org. Nenhuma foi
#  escrita por descuido: cada cópia foi feita a partir de outra que, na
#  altura, ainda estava certa.
#
#  Corrigir tudo de uma vez é o refactor cego que a Regra 0 proíbe. A
#  catraca resolve o que interessa já: o número de cópias NÃO PODE SUBIR.
#  Código novo usa o helper que existe; a dívida herdada baixa quando for
#  tratada (ADR-0004 §6) — e baixar é a única forma de mexer na referência.
#
#  Como a catraca do clippy, também falha quando o número DESCE sem BLESS:
#  uma referência desactualizada deixa entrar uma cópia nova «de graça».
#
#  Limite honesto: conta PADRÕES, não semântica. Um helper com outro nome
#  que faça a mesma coisa escapa. É a revisão (agentes delonix-meet-*) que
#  apanha isso; a catraca apanha a cópia literal, que é a que aconteceu.
#
#  Uso:  bash scripts/check-arquitectura-catraca.sh
#        BLESS=1 bash scripts/check-arquitectura-catraca.sh   (grava a fasquia)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
exec python3 - <<'PYEOF'
import glob, os, re, sys

SRC = 'server/src'
BASELINE = 'scripts/arquitectura-baseline.txt'
BLESS = os.environ.get('BLESS') == '1'

def fontes(excluir=()):
    for path in sorted(glob.glob(f'{SRC}/*.rs')):
        if os.path.basename(path) in excluir:
            continue
        yield path, open(path, encoding='utf-8').read()

def sem_comentarios(texto):
    return '\n'.join(l for l in texto.splitlines() if not l.lstrip().startswith('//'))

def contar(regex, excluir=()):
    total, onde = 0, []
    for path, texto in fontes(excluir):
        for n, linha in enumerate(texto.splitlines(), 1):
            if linha.lstrip().startswith('//'):
                continue
            if re.search(regex, linha):
                total += 1
                onde.append(f'{path}:{n}')
    return total, onde

def v1_com_sessao():
    """Handlers montados em /api/v1 cuja assinatura extrai `AuthUser` (sessão)."""
    main = open(f'{SRC}/main.rs', encoding='utf-8').read()
    m = re.search(r'\.nest\(\s*"/api/v1"(.*?)\.layer\(\s*middleware::', main, re.S)
    if not m:
        print('✗ catraca: não encontrei o bloco .nest("/api/v1") em main.rs — o portão ficou cego')
        sys.exit(1)
    total, onde = 0, []
    for mod, fn in set(re.findall(r'\b(?:get|post|put|patch|delete)\(\s*(\w+)::(\w+)\s*\)', m.group(1))):
        path = f'{SRC}/{mod}.rs'
        if not os.path.exists(path):
            continue
        texto = open(path, encoding='utf-8').read()
        sig = re.search(rf'pub(?:\(crate\))?\s+async\s+fn\s+{fn}\s*\((.*?)\)\s*->', texto, re.S)
        if sig and 'AuthUser' in sig.group(1):
            total += 1
            onde.append(f'{path}  {mod}::{fn}')
    return total, sorted(onde)

# nome → (medida, regra do ADR-0004 §5 que a justifica)
MEDIDAS = {
    'pertenca_org_fora_de_org_rs': (
        lambda: contar(r'\borg_members\b', excluir=('org.rs',)),
        'regra 1 — pertença decide-se em org.rs (role_in_org/require_*_pub filtram archived_at)'),
    'authorization_lido_a_mao': (
        lambda: contar(r'strip_prefix\(\s*"Bearer '),
        'regra 2 — usar um extractor de auth.rs'),
    'clientes_reqwest': (
        lambda: contar(r'reqwest::Client::(builder|new)\('),
        'regra 3 — pedidos de saída pelo cliente partilhado, com guarda SSRF'),
    'primitivas_cripto_espalhadas': (
        lambda: contar(r'Sha256::digest|Argon2::default\(\)|\.fill_bytes\(|thread_rng\(\)\.fill\(',
                       excluir=('crypto.rs',)),
        'regra 4 — sha256/argon2/aleatoriedade num só módulo (crypto)'),
    'funcoes_sufixo_pub': (
        lambda: contar(r'\bfn\s+[a-z0-9_]+_pub\b'),
        'regra 5 — pub(crate) com o nome real'),
    'respostas_ok_true': (
        lambda: contar(r'"ok"\s*:\s*true'),
        'regra 6 — 201/204 ou o recurso, nunca {"ok": true}'),
    'rotas_v1_com_sessao': (
        v1_com_sessao,
        'regra 7 — /api/v1 autentica por chave, a sessão é da BFF'),
}

referencia = {}
if os.path.exists(BASELINE):
    for linha in open(BASELINE, encoding='utf-8'):
        linha = linha.split('#', 1)[0].strip()
        if '=' in linha:
            k, v = linha.split('=', 1)
            referencia[k.strip()] = int(v)

medido = {nome: f() for nome, (f, _) in MEDIDAS.items()}

if BLESS:
    # O BLESS só BAIXA. Se servisse para subir, a catraca era uma formalidade:
    # a cópia nova entrava com um comando a mais. Subir exige editar o ficheiro
    # à mão, num diff que a revisão vê.
    subidas = [f'{k}: {referencia[k]} → {v[0]}' for k, v in medido.items()
               if k in referencia and v[0] > referencia[k]]
    if subidas:
        print('✗ catraca: o BLESS só baixa a fasquia, e isto subiu:')
        for s in subidas:
            print(f'     {s}')
        sys.exit(1)
    with open(BASELINE, 'w', encoding='utf-8') as out:
        out.write('# Catraca da arquitectura (ADR-0004 §5). Gerado por\n')
        out.write('#   BLESS=1 bash scripts/check-arquitectura-catraca.sh\n')
        out.write('# Só se baixa. Subir um número é aceitar uma cópia nova — não se faz aqui.\n')
        for nome, (total, _) in medido.items():
            out.write(f'{nome}={total}\n')
    print(f'✓ catraca da arquitectura: referência gravada em {BASELINE}')
    sys.exit(0)

falha = False
for nome, (total, onde) in medido.items():
    regra = MEDIDAS[nome][1]
    if nome not in referencia:
        print(f'✗ catraca: «{nome}» não tem referência em {BASELINE} (medido {total})')
        falha = True
    elif total > referencia[nome]:
        print(f'✗ catraca: «{nome}» subiu de {referencia[nome]} para {total} — {regra}')
        for o in onde:
            print(f'     {o}')
        falha = True
    elif total < referencia[nome]:
        print(f'✗ catraca: «{nome}» desceu de {referencia[nome]} para {total} — bom.')
        print(f'     Grava a fasquia nova: BLESS=1 bash scripts/check-arquitectura-catraca.sh')
        falha = True

if falha:
    sys.exit(1)
print('✓ catraca da arquitectura: ' + ', '.join(f'{k}={v[0]}' for k, v in medido.items()))
PYEOF
