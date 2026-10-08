#!/usr/bin/env python3
"""Conta os `eslint-disable` de regras de hooks que NÃO dizem porquê.

Usado por `scripts/check-frontend-lint.sh`. Até 2026-10-08 a catraca contava
TODOS, e isso metia no mesmo saco um disable pensado e um preguiçoso. Vários
são deliberados e correctos:

  - `components/AsyncSection` tem `[...deps, nonce]`, e o ESLint não consegue
    verificar um *spread* — o disable ali é estrutural, não esconde nada;
  - `ui/search/useSearch` exclui o `t` de propósito, porque incluí-lo refazia a
    pesquisa a cada mudança de língua.

Um disable COM a razão escrita imediatamente acima não é um problema escondido:
é uma decisão registada. Contar só os outros muda o incentivo — escrever porquê
passa a ser o caminho para a catraca descer, em vez de silenciar.

«Razão escrita» = há um comentário nas DUAS linhas anteriores. Duas e não uma
porque o padrão real do repo às vezes é «comentário, linha de código que ele
explica, disable» — o `ui/search/ListSearch` faz isso, e a primeira versão
deste contador não lhe via a razão que lá estava.

É deliberadamente simples: um portão que tentasse julgar a QUALIDADE da razão
seria um portão que ninguém percebe quando falha.
"""
import pathlib
import sys

raiz = pathlib.Path(sys.argv[1])
n = 0
for p in raiz.rglob('*'):
    if p.suffix not in ('.ts', '.tsx') or not p.is_file():
        continue
    linhas = p.read_text().split('\n')
    for i, linha in enumerate(linhas):
        if 'eslint-disable' in linha and 'react-hooks' in linha:
            antes = [linhas[j].strip() for j in (i - 2, i - 1) if j >= 0]
            if not any(a.startswith(('//', '*', '/*')) for a in antes):
                n += 1
print(n)
