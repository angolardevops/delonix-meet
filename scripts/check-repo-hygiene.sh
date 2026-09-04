#!/usr/bin/env bash
# ============================================================
#  Fitness function de higiene do repositório.
#
#  Existe porque já aconteceu: uma chave privada TLS e um `.pyc` estavam
#  seguidos no git. Nenhum dos dois foi posto lá de má-fé — foram um `git add`
#  distraído num directório que ainda não estava no `.gitignore`. Um portão
#  automático apanha isso no minuto seguinte, em vez de meses depois.
#
#  Os pontos 1–5 verificam o que está SEGUIDO no índice (o que sai num clone
#  hoje). Ficheiros locais não seguidos são problema de ninguém.
#
#  O ponto 6 verifica o HISTÓRICO, e existe porque os pontos 1–5 não bastam:
#  `git rm --cached` tira do índice e NÃO tira do histórico. A chave que já lá
#  está continua alcançável em todos os commits que a levaram — e este
#  repositório é público. Sem esta verificação, uma fuga desaparece do portão
#  no instante em que alguém a «resolve» com um `git rm`, que é exactamente o
#  que aconteceu em 3b80b8a.
#
#  Uso:  bash scripts/check-repo-hygiene.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0

# 1) Nenhum material de chave privada seguido — de dev também não. Uma chave
#    num repositório é uma chave comprometida: qualquer clone a tem.
keys=$(git ls-files -- '*.key' '*.pem' '*.p12' '*.pfx' '*.jks' 'id_rsa*' 'id_ed25519*' 2>/dev/null)
if [ -n "$keys" ]; then
  echo "✗ higiene: material de chave privada SEGUIDO no git:"
  echo "$keys" | sed 's/^/     /'
  echo "     Gera-o localmente (ver 'make certs') e põe o directório no .gitignore."
  fail=1
fi

# 2) Cabeçalho PEM de chave privada dentro de QUALQUER ficheiro seguido —
#    apanha a chave colada num .yaml, .env.example ou README, que a extensão
#    do ponto 1 não vê.
#
#    Duas precisões que a primeira versão não tinha e que a faziam acusar-se a
#    si própria: o padrão é ancorado ao INÍCIO DA LINHA (é assim que um PEM a
#    sério começa; uma menção em prosa não), e este ficheiro fica de fora da
#    busca, porque contém o padrão por definição. Um portão que dispara sobre
#    si mesmo não é rigor — é ruído, e ruído acaba desligado.
pem_hits=$(git grep -lI -e '^-----BEGIN [A-Z ]*PRIVATE KEY-----' \
             -- . ':(exclude)scripts/check-repo-hygiene.sh' 2>/dev/null || true)
if [ -n "$pem_hits" ]; then
  echo "✗ higiene: cabeçalho de CHAVE PRIVADA dentro de ficheiros seguidos:"
  echo "$pem_hits" | sed 's/^/     /'
  fail=1
fi

# 3) Artefactos compilados / gerados não se versionam.
arts=$(git ls-files -- '*.pyc' '*.pyo' '*.class' '*.o' '*.so' '*.rlib' '__pycache__/*' 'web/dist/*' 'server/target/*' 2>/dev/null)
if [ -n "$arts" ]; then
  echo "✗ higiene: artefactos compilados SEGUIDOS no git:"
  echo "$arts" | sed 's/^/     /'
  fail=1
fi

# 4) Dumps de base de dados — dados de utilizador, nunca no repo.
dumps=$(git ls-files -- 'backups/*' '*.dump' 2>/dev/null)
if [ -n "$dumps" ]; then
  echo "✗ higiene: dumps de base de dados SEGUIDOS no git:"
  echo "$dumps" | sed 's/^/     /'
  fail=1
fi

# 5) As migrações têm de ser uma sequência SEM buracos nem números repetidos.
#    Um número repetido em duas branches faz o `sqlx migrate` divergir entre
#    ambientes — falha tardia, no deploy, e não no CI.
nums=$(ls server/migrations/*.sql 2>/dev/null | sed -E 's/.*\/([0-9]{4})_.*/\1/' | sort)
dups=$(echo "$nums" | uniq -d)
if [ -n "$dups" ]; then
  echo "✗ higiene: número de migração REPETIDO: $(echo "$dups" | tr '\n' ' ')"
  fail=1
fi
prev=0
for n in $nums; do
  cur=$((10#$n))
  if [ "$cur" -ne $((prev + 1)) ]; then
    echo "✗ higiene: buraco na sequência de migrações — depois de $(printf '%04d' $prev) vem $n"
    fail=1
  fi
  prev=$cur
done

# 5. Catálogo de regressões: números ÚNICOS e referências que existem.
#    Mesma razão das migrações, e a mesma causa: duas branches acrescentam ao
#    fim do ficheiro e o git funde sem conflito, porque as linhas não colidem.
#    O resultado são dois R49 a falar de coisas diferentes — e um `Ver R49` no
#    HARNESS.md que passa a apontar para as duas. Aconteceu; ver R59.
CAT=docs/reference/regressions.md
if [ -f "$CAT" ]; then
  rnums=$(grep -oE '^### R[0-9]+' "$CAT" | grep -oE '[0-9]+' | sort -n)
  rdups=$(echo "$rnums" | uniq -d)
  if [ -n "$rdups" ]; then
    echo "✗ higiene: número de regressão REPETIDO no catálogo: $(echo "$rdups" | sed 's/^/R/' | tr '\n' ' ')"
    fail=1
  fi
  # Uma referência `R<n>` fora do catálogo tem de ter entrada no catálogo.
  refs=$(grep -rhoE '\bR[0-9]{1,3}\b' --include='*.rs' --include='*.tsx' --include='*.ts' \
           --include='*.mjs' --include='*.md' --include='*.sql' --include='*.sh' \
           --exclude-dir=node_modules --exclude-dir=target . 2>/dev/null \
         | grep -oE '[0-9]+' | sort -n -u)
  for r in $refs; do
    echo "$rnums" | grep -qx "$r" || {
      echo "✗ higiene: referência a R$r sem entrada no catálogo (renumeração perdida?)"
      fail=1
    }
  done
fi

# 6. Todo o teste ponta-a-ponta ou corre no CI, ou tem razão escrita.
#    Um teste que existe e nunca corre não é portão nenhum — o `estudio.mjs`
#    esteve comprometido sem correr uma única vez e os seis jobs davam verde
#    na mesma (R72). Compara-se o inventário do que EXISTE com o do que CORRE.
#
#    Duas saídas válidas, e são deliberadamente diferentes:
#      - `scripts/e2e-fora-do-ci.txt` — para um TESTE que não corre, com a razão.
#        Mesmo padrão do `rotas-publicas.txt`.
#      - `MÓDULO DE APOIO` no cabeçalho — para o que NÃO é um teste. Fica no
#        próprio ficheiro porque é lá que quem o abre a lê, e porque uma lista
#        de nomes de módulos de apoio só cresce e ninguém a poda.
WF=.github/workflows/ci.yml
EXC=scripts/e2e-fora-do-ci.txt
if [ -f "$WF" ] && [ -d web/e2e ]; then
  for f in web/e2e/*.mjs; do
    n=$(basename "$f")
    # Módulos de apoio, não testes — declarados no próprio cabeçalho.
    case "$n" in pg.mjs|harness*.mjs) continue;; esac
    head -20 "$f" | grep -q "MÓDULO DE APOIO" && continue
    grep -q "web/e2e/$n" "$WF" && continue
    grep -qE "^$n . .+" "$EXC" 2>/dev/null && continue
    echo "✗ higiene: $n não corre no CI nem tem razão em $EXC"
    fail=1
  done
fi

# 6. Material de chave privada no HISTÓRICO, não só no índice.
#
#    O ponto 1 vê o que sai num clone HOJE. Este vê o que sai num `git log`, que
#    é o que um atacante lê. As duas chaves de dev que já cá estiveram saíram do
#    índice em 3b80b8a e continuam alcançáveis em quatro commits — o ponto 1
#    ficou verde no minuto seguinte e a exposição não mudou nada.
#
#    Cada caminho encontrado tem de estar em scripts/leaked-keys-accepted.txt,
#    com a razão e a data escritas. É o mesmo padrão do rustsec-accepted.txt: o
#    portão não impede a decisão, impede a decisão SILENCIOSA.
#
#    Limite honesto desta verificação: procura por CAMINHO, não por conteúdo.
#    Uma chave colada dentro de um `notas.txt` do histórico não é apanhada aqui
#    — para o índice, é o ponto 2 que a apanha; para o histórico, seria preciso
#    ler todos os blobs, e isso não cabe num portão de CI. Preferimos dizê-lo a
#    dar uma garantia que não temos.
LEDGER=scripts/leaked-keys-accepted.txt
hist_keys=$(git rev-list --objects --all 2>/dev/null \
            | sed 's/^[0-9a-f]\{40,\} //' \
            | grep -iE '(^|/)(id_rsa|id_ed25519|id_ecdsa)[^/]*$|\.(key|pem|p12|pfx|jks)$' \
            | sort -u || true)
for p in $hist_keys; do
  # `-F -x` porque o ledger guarda caminhos literais, um por linha; sem isto um
  # `.` do caminho passava a curinga e um caminho novo podia casar com a linha
  # de outro.
  if ! grep -v '^#' "$LEDGER" 2>/dev/null | grep -qFx "$p"; then
    echo "✗ higiene: CHAVE PRIVADA no histórico do git, sem decisão escrita: $p"
    echo '     Um "git rm" NÃO a remove do histórico. Trata-a como comprometida:'
    echo "     roda-a, e depois ou reescreves o histórico (force-push, parte todos"
    echo "     os clones) ou acrescentas uma linha a $LEDGER com a razão."
    fail=1
  fi
done

[ "$fail" = 0 ] && echo "✓ higiene do repositório: sem chaves, artefactos ou dumps seguidos; migrações e regressões sem duplicados; fugas de chave no histórico todas com decisão escrita"
exit $fail
