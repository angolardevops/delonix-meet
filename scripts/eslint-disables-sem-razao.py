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

«Razão escrita» = há um comentário nas TRÊS linhas acima do disable. O padrão
real do repo é um comentário de duas ou três linhas a explicar a decisão, às
vezes com a linha de código que ele explica no meio (o `ui/search/ListSearch` e
o `ui/search/useResourceSearch` fazem isso) — uma janela de uma linha dava
falso positivo em ambos, e foram falsos positivos MEUS, não código a corrigir.

Três e não cinco: com cinco, aceitava prosa de OUTRO assunto como razão.

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
            # Sobe no máximo TRÊS linhas. A janela foi MEDIDA, não escolhida:
            # com cinco, o contador aceitava prosa de outro assunto como razão
            # (no `pages/RecordingPlayer` apanhava um comentário sobre
            # capítulos a -5, e no `pages/Studio` a cauda de outra frase a -4).
            # Com três, as 22 razões aceites são todas sobre as dependências do
            # efeito — verificadas uma a uma.
            tem_razao = False
            for j in range(i - 1, max(i - 4, -1), -1):
                linha_acima = linhas[j].strip()
                if linha_acima.startswith(('//', '*', '/*')):
                    tem_razao = True
                    break
                if not linha_acima:
                    continue
            if not tem_razao:
                n += 1
print(n)
