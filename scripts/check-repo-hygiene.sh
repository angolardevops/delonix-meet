#!/usr/bin/env bash
# ============================================================
#  Fitness function de higiene do repositório.
#
#  Existe porque já aconteceu: uma chave privada TLS e um `.pyc` estavam
#  seguidos no git. Nenhum dos dois foi posto lá de má-fé — foram um `git add`
#  distraído num directório que ainda não estava no `.gitignore`. Um portão
#  automático apanha isso no minuto seguinte, em vez de meses depois.
#
#  Verifica só o que está SEGUIDO no índice (o que sai num clone). Ficheiros
#  locais não seguidos são problema de ninguém.
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

[ "$fail" = 0 ] && echo "✓ higiene do repositório: sem chaves, artefactos ou dumps seguidos; migrações contíguas"
exit $fail
