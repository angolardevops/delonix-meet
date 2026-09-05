#!/usr/bin/env bash
# Todo o teste que usa Playwright corre DEPOIS de os browsers existirem.
#
# Porquê um portão para isto: o mesmo erro foi cometido DUAS VEZES em dois dias
# — o portão da barra no job do frontend, e o teste de reentrada no job do
# isolamento. Nos dois casos o passo ficou antes do `npx playwright install` e
# morreu com «Executable doesn't exist».
#
# O erro é fácil de repetir porque o `npm ci` dá a sensação de ter instalado
# tudo: traz a BIBLIOTECA do Playwright, e os browsers vêm de um comando à
# parte. E é caro porque o sintoma não aponta para a causa — parece um problema
# de ambiente, não uma linha fora de ordem.
#
# O que se verifica: em cada JOB, a primeira invocação de um teste que importe
# `@playwright/test` vem depois de um `playwright install` nesse mesmo job.
set -uo pipefail
cd "$(dirname "$0")/.."
WF=.github/workflows/ci.yml
[ -f "$WF" ] || { echo "✗ $WF não existe"; exit 1; }

python3 - "$WF" <<'PY'
import re, sys, pathlib
wf = pathlib.Path(sys.argv[1]).read_text(encoding='utf-8')

# Quais dos e2e precisam mesmo de browser: os que importam o Playwright.
precisam = {
    p.name for p in pathlib.Path('web/e2e').glob('*.mjs')
    if '@playwright/test' in p.read_text(encoding='utf-8')
}

# Partir por JOB. Um `playwright install` num job não vale para outro — foi
# exactamente essa a suposição que falhou da primeira vez.
jobs = re.split(r'\n  (?=[a-z][a-z0-9_-]*:\n)', wf)
falhou = False
for bloco in jobs:
    nome = (re.match(r'\s*([a-z][a-z0-9_-]*):', bloco) or [None, '?'])[1]
    linhas = bloco.split('\n')
    instalou_em = None
    for i, l in enumerate(linhas):
        if 'playwright install' in l and instalou_em is None:
            instalou_em = i
        for m in re.finditer(r'web/e2e/([a-z0-9-]+\.mjs)', l):
            f = m.group(1)
            if f not in precisam:
                continue
            if instalou_em is None:
                print(f'✗ browser: no job "{nome}", {f} corre sem `playwright install` antes')
                falhou = True
sys.exit(1 if falhou else 0)
PY
code=$?
[ $code -eq 0 ] && echo "✓ browser: todo o teste com Playwright corre depois de os browsers existirem"
exit $code
