#!/usr/bin/env bash
# ============================================================
#  Fitness function: toda a reivindicação de trabalho passa pela peça comum.
#
#  PORQUE EXISTE: a 2026-10-07 havia SETE `FOR UPDATE SKIP LOCKED` à mão no
#  servidor, com a mesma semântica e SQL diferente, e a catraca da arquitectura
#  NÃO os via — ela conta oito padrões sintácticos, e sete consultas diferentes
#  não batem em nenhum. Pior: enquanto o trabalho de as juntar estava em curso,
#  entrou uma OITAVA (a composição, #268) sem ninguém ver.
#
#  A peça é `delonix_meet_core::jobs` (forma e aritmética),
#  `delonix_meet_store::jobs` (a única reivindicação) e `server/src/jobs.rs`
#  (o ciclo). Desenho: docs/desenho-2026-10-07-uma-fila-so.md.
#
#  Este portão conta os `FOR UPDATE … SKIP LOCKED` em `server/src/` que NÃO
#  estão no adaptador, e falha se passarem da fasquia. A fasquia é 1 e a
#  excepção é nomeada: `sms::agent_claim`, que junta duas tabelas e filtra pelo
#  gateway autenticado — outra forma, não outra cópia (ver o comentário lá).
#
#  Uso:  bash scripts/check-filas-reivindicacao.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

FASQUIA=1

# Só código: as linhas de comentário (`//`) falam do padrão sem o usar.
mapfile -t achados < <(
  /usr/bin/grep -rn "FOR UPDATE[^\"]*SKIP LOCKED" --include=*.rs server/src \
    | /usr/bin/grep -v "^[^:]*:[0-9]*: *//" || true
)

n=${#achados[@]}
if [ "$n" -gt "$FASQUIA" ]; then
  echo "✗ filas: $n reivindicações fora da peça comum (fasquia $FASQUIA)."
  echo "  Uma fila nova declara-se com delonix_meet_core::jobs::Queue e"
  echo "  reivindica com delonix_meet_store::jobs::claim — ver"
  echo "  docs/desenho-2026-10-07-uma-fila-so.md. Se for OUTRA FORMA e não"
  echo "  outra cópia, escreve porquê no código e sobe a fasquia aqui."
  for a in "${achados[@]}"; do echo "  $a"; done
  exit 1
fi
echo "✓ filas: $n reivindicação(ões) fora da peça comum, na fasquia ($FASQUIA)"
