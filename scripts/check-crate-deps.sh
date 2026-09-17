#!/usr/bin/env bash
# ============================================================
#  Fitness function: a regra da dependência entre crates (ADR-0006 §1).
#
#  Porquê: o domínio que conhece o sqlx ou o axum deixa de ser testável sem
#  base nem servidor, e a regra de negócio volta a escorregar para o handler —
#  que é o problema de partida (auditoria 2026-09-16 §2.3). A disciplina não
#  chega: um `cargo add` num crate de domínio passa na revisão sem ninguém
#  reparar. Este portão lê os Cargo.toml e falha.
#
#  Duas regras:
#   1. PROIBIDAS — cada crate tem uma lista de dependências externas que não
#      pode ter (IO no núcleo, webrtc fora da media, …).
#   2. SÓ PARA BAIXO — um crate `delonix-meet-*` só depende de crates de
#      camada inferior. O monólito `delonix-server` (em transição) pode
#      depender de todos; nenhum crate pode depender dele.
#
#  Um crate novo sem linha na tabela FALHA: não entra nada sem regra escrita.
#
#  Uso:  bash scripts/check-crate-deps.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
exec python3 - <<'PYEOF'
import glob, re, sys, tomllib

IO = {'sqlx', 'axum', 'tonic', 'reqwest', 'redis', 'webrtc', 'tokio-tungstenite', 'hyper', 'tower', 'tower-http', 'openidconnect'}

# nome -> (camada, dependências externas proibidas)
# Camada: um crate só pode depender de crates `delonix-meet-*` de camada MENOR.
REGRAS = {
    'delonix-meet-core':         (0, IO | {'tokio'}),
    'delonix-meet-protocol':     (1, IO - {'tonic'}),
    'delonix-meet-domain':       (2, IO),
    'delonix-meet-store':        (3, IO - {'sqlx'}),
    'delonix-meet-integrations': (3, IO - {'reqwest', 'openidconnect'}),
    'delonix-meet-media':        (3, {'sqlx', 'axum', 'reqwest', 'redis', 'tower-http'}),
    'delonix-meet-realtime':     (4, {'sqlx', 'axum', 'reqwest', 'tower-http'}),
    'delonix-meet-api':          (5, set()),
    'delonix-meet-server':       (6, set()),
}

falha = False
vistos = 0
for path in sorted(glob.glob('server/crates/*/Cargo.toml')):
    toml = tomllib.load(open(path, 'rb'))
    nome = toml['package']['name']
    vistos += 1
    if nome not in REGRAS:
        print(f'✗ crate-deps: {nome} ({path}) não tem regra em scripts/check-crate-deps.sh — escreve a camada e as proibidas')
        falha = True
        continue
    camada, proibidas = REGRAS[nome]
    deps = {}
    for sec in ('dependencies', 'build-dependencies'):
        deps.update(toml.get(sec, {}))
    for dep in deps:
        if dep == 'delonix-server':
            print(f'✗ crate-deps: {nome} depende do monólito delonix-server — o código sai dele, não volta')
            falha = True
        elif dep.startswith('delonix-meet-'):
            if dep not in REGRAS:
                print(f'✗ crate-deps: {nome} depende de {dep}, que não tem regra')
                falha = True
            elif REGRAS[dep][0] >= camada:
                print(f'✗ crate-deps: {nome} (camada {camada}) depende de {dep} (camada {REGRAS[dep][0]}) — só se depende para baixo')
                falha = True
        elif dep in proibidas:
            print(f'✗ crate-deps: {nome} depende de {dep}, proibido nesta camada (ADR-0006 §1)')
            falha = True

if vistos == 0:
    print('✗ crate-deps: nenhum crate em server/crates — o portão ficou cego')
    sys.exit(1)
if falha:
    sys.exit(1)
print(f'✓ crate-deps: {vistos} crate(s) respeitam a regra da dependência')
PYEOF
