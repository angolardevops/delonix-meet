#!/usr/bin/env bash
# Todo o teste que usa Playwright encontra os browsers que precisa.
#
# Porquê um portão: a mesma família de erro custou QUATRO correcções em dois
# dias, e as três primeiras pareceram certas porque todas dão o MESMO sintoma —
# `Executable doesn't exist`. Ver R97.
#
#   1. o passo corre antes do `playwright install` do job;
#   2. o passo tem `working-directory: web` e o comando já diz `web/e2e/`, e o
#      Node procura `web/web/e2e/`;
#   3. o `playwright install` corre na RAIZ, onde não há `node_modules`: o npx
#      descarrega a versão mais recente e instala os browsers DESSA, enquanto
#      os testes usam a do projecto. Foi esta a causa verdadeira, escondida
#      atrás das outras duas durante três correcções.
#
# O ficheiro é lido com um parser de YAML e não com `grep`. A primeira versão
# cortava os jobs com uma expressão regular e atribuía passos ao job errado —
# um portão que reporta o sítio errado é pior do que nenhum, porque manda
# procurar onde não está.
set -uo pipefail
cd "$(dirname "$0")/.."

python3 - <<'PY'
import sys, pathlib, yaml

wf = yaml.safe_load(pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8'))
e2e = pathlib.Path('web/e2e')
precisam = {p.name for p in e2e.glob('*.mjs') if '@playwright/test' in p.read_text(encoding='utf-8')}

falhou = False
for nome, job in (wf.get('jobs') or {}).items():
    job_em_web = (((job.get('defaults') or {}).get('run') or {}).get('working-directory')) == 'web'
    instalou = False
    for passo in job.get('steps') or []:
        run = passo.get('run') or ''
        wd = passo.get('working-directory')
        em_web = wd == 'web' or (wd is None and job_em_web)

        if 'playwright install' in run:
            if not em_web:
                print(f'✗ browser: job "{nome}": `playwright install` corre na RAIZ. '
                      f'Sem `node_modules` o npx baixa outra versão e os browsers não batem certo.')
                falhou = True
            instalou = True

        usados = [f for f in precisam if f'web/e2e/{f}' in run]
        if not usados:
            continue
        if wd == 'web':
            print(f'✗ browser: job "{nome}", passo "{passo.get("name", "?")}": comando diz '
                  f'`web/e2e/…` E tem `working-directory: web` — o caminho fica `web/web/e2e/`.')
            falhou = True
        if not instalou:
            print(f'✗ browser: job "{nome}", passo "{passo.get("name", "?")}": '
                  f'{", ".join(usados)} corre sem `playwright install` antes NESTE job.')
            falhou = True

sys.exit(1 if falhou else 0)
PY
code=$?
[ $code -eq 0 ] && echo "✓ browser: os testes com Playwright encontram os browsers que precisam"
exit $code
