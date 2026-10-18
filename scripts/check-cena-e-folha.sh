#!/usr/bin/env bash
# ============================================================
#  Fitness function: `web/src/cena/` é FOLHA — não importa de `room/` nem de
#  `studio/`.
#
#  PORQUE EXISTE. A moldura de transmissão é desenhada pelos DOIS compositores:
#  o da Sala (`web/src/room/compositor.ts`) e o do Estúdio
#  (`web/src/studio/compositor.ts`). Para não escrever a cena duas vezes, ela
#  vive num módulo próprio que ambos importam.
#
#  O risco concreto, e não é hipotético: o `Rect` nascia no
#  `room/compositor.ts`, e o caminho preguiçoso para o reutilizar na cena era
#  importá-lo de lá. Nesse instante ficava `room → cena → room` — um ciclo. O
#  TypeScript compila ciclos de TIPO sem se queixar, e o Vite resolve muitos
#  ciclos de valor em silêncio até ao dia em que um `import` passa a ser
#  avaliado antes do módulo estar pronto e alguém recebe `undefined` no sítio
#  de uma função. É um defeito que não dá vermelho em lado nenhum até dar.
#
#  Medido a 2026-10-18: a aresta `room → studio` JÁ EXISTE
#  (`room/useMulticam.ts` importa `../studio/directo` e
#  `../studio/destinosGuardados`). Por isso este portão NÃO proíbe essa — seria
#  vermelho à nascença e obrigava a um refactor fora de âmbito. Proíbe a única
#  que fecha o ciclo: a cena a importar de volta.
#
#  CONTROLO POSITIVO, e não é decoração: um `grep` que não encontra nada é
#  indistinguível de um `grep` que procurou no sítio errado — uma pasta
#  renomeada, uma extensão nova, um `cd` falhado.
#
#  Mas o controlo NÃO pode ser «a cena tem de ter imports»: um módulo folha
#  genuinamente puro pode não ter nenhum, e o `cena/moldura.ts` não tem. Essa
#  primeira versão punha o portão vermelho precisamente por o código estar
#  certo. O controlo é, em vez disso, uma AMOSTRA CONHECIDA: o mesmo extractor
#  corre contra o `room/useMulticam.ts`, que SABEMOS importar de `studio/`, e
#  tem de o apanhar lá. Se não apanhar, é o extractor que está partido, e o
#  silêncio do lado da cena não vale nada.
#
#  Uso:  bash scripts/check-cena-e-folha.sh   (exit 1 se a cena deixar de ser folha)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

CENA=${CENA_DIR:-web/src/cena}
r=$'\e[31m'; g=$'\e[32m'; z=$'\e[0m'

if [ ! -d "$CENA" ]; then
  echo "${r}✗ cena: a pasta $CENA não existe — o portão não mediu nada${z}"
  echo "   (isto é falha, não verde: se a cena mudou de sítio, este portão tem de a seguir)"
  exit 1
fi

ficheiros=$(find "$CENA" -type f \( -name '*.ts' -o -name '*.tsx' \) ! -name '*.test.ts' ! -name '*.test.tsx' 2>/dev/null | sort)
if [ -z "$ficheiros" ]; then
  echo "${r}✗ cena: não encontrei ficheiros de código em $CENA — o portão não mediu nada${z}"
  exit 1
fi

# As origens de todos os `import …` / `export … from …` de um conjunto de
# ficheiros, uma por linha.
origens() {
  grep -hoE "(import|export)[^\"']*(from[[:space:]]*)?['\"][^\"']+['\"]" "$@" 2>/dev/null \
    | grep -oE "['\"][^\"']+['\"]" | tr -d "\"'" | sort -u
}
# Uma origem aponta para room/ ou studio/?
proibidas() { grep -E '(^|/)(\.\./)*(room|studio)/' || true; }

# ---- controlo positivo: o extractor funciona ---------------------------------
# Amostra que SABEMOS violar a regra (o useMulticam importa de ../studio/).
# Se o extractor não a apanhar aqui, o silêncio do lado da cena não vale nada.
AMOSTRA=${AMOSTRA_CONHECIDA:-web/src/room/useMulticam.ts}
if [ ! -f "$AMOSTRA" ]; then
  echo "${r}✗ cena: a amostra de controlo $AMOSTRA não existe — não dá para provar o extractor${z}"
  exit 1
fi
if [ -z "$(origens "$AMOSTRA" | proibidas)" ]; then
  echo "${r}✗ cena: o extractor NÃO apanhou o import de studio/ em $AMOSTRA${z}"
  echo "   É o controlo positivo a falhar: o portão está a olhar para o sítio errado"
  echo "   ou a expressão partiu-se. Verde daqui seria mentira."
  exit 1
fi

# ---- a regra ----------------------------------------------------------------
# shellcheck disable=SC2086
todos=$(origens $ficheiros)
proibidos=$(printf '%s\n' "$todos" | proibidas)

if [ -n "$proibidos" ]; then
  echo "${r}✗ cena: $CENA importa de room/ ou studio/ — deixou de ser folha${z}"
  printf '     %s\n' $proibidos
  echo
  echo "   É assim que o ciclo se forma: os dois compositores importam a cena,"
  echo "   e a cena a importar de volta fecha room → cena → room. O TypeScript"
  echo "   compila-o calado; parte-se em runtime, um dia, com um undefined."
  echo "   O que fazer: mover para cena/ o que a cena precisa (foi o que se fez"
  echo "   ao Rect), e re-exportá-lo do lado de lá para não partir importadores."
  exit 1
fi

# `printf '%s\n' "$vazio" | wc -l` dá 1, não 0 — imprime uma linha em branco e
# conta-a. O portão anunciava «1 imports» num ficheiro sem import nenhum: uma
# mentira pequena, mas sobre a SUA PRÓPRIA medição, que é a classe de defeito
# que ele existe para apanhar. `grep -c .` conta linhas não-vazias.
quantos=$(printf '%s\n' "$todos" | grep -c . || true)
echo "${g}✓ cena: $CENA é folha — $quantos imports, nenhum de room/ ou studio/${z}"
exit 0
