#!/usr/bin/env bash
# ============================================================
#  Fitness function: o /ws NÃO escreve o token no access log.
#
#  PORQUE EXISTE. O token da sala — e o de FONTE de um estúdio, que vive
#  12 h (`SOURCE_TOKEN_TTL_SECS`) — viaja no `?token=` do `/ws`. Um
#  `access_log` nessa location guarda a credencial em claro num ficheiro:
#  quem leia os logs do bordo publica media naquela sala até o token
#  expirar, e uma fonte está isenta do tecto de sockets e da quota da
#  organização.
#
#  A regra já era conhecida da casa — o `deploy/nginx-delonix.conf` e o
#  template do ansible desligam-no com a nota «os tokens em ?token= NÃO
#  devem ser registados», e o próprio `edge.conf` desliga-o seis linhas
#  abaixo para o QR do Linphone (R278). Faltava nos dois ficheiros que
#  servem o compose e o k8s, que são precisamente os que vão a produção.
#  Medido a 2026-10-16, na revisão do #356.
#
#  Verifica (estático): em cada nginx do repo que tenha uma location do
#  `/ws`, essa location contém `access_log off`.
#
#  Uso:  bash scripts/check-ws-sem-access-log.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0

# Os nginx do repo. Um ficheiro novo que sirva o `/ws` tem de entrar aqui —
# e o portão avisa se a lista encolher sem razão.
FICHEIROS=(
  deploy/compose/edge.conf
  deploy/k8s/nginx.conf
  deploy/nginx-delonix.conf
  deploy/ansible/roles/single_host/templates/nginx-delonix.conf.j2
)

vistos=0
for f in "${FICHEIROS[@]}"; do
  if [ ! -f "$f" ]; then
    echo "✗ ws/access_log: $f não existe — a lista do portão está velha"
    fail=1
    continue
  fi
  # A location do `/ws` (com ou sem `$`, com ou sem `|rtc`): do cabeçalho
  # até à chave que o fecha, na primeira coluna do bloco.
  bloco=$(awk '
    /location[[:space:]]*~[[:space:]]*\^\/\(ws/ { dentro=1 }
    dentro { print }
    dentro && /^[[:space:]]*\}[[:space:]]*$/ { exit }
  ' "$f")
  if [ -z "$bloco" ]; then
    # Sem location do /ws não há nada a guardar neste ficheiro.
    continue
  fi
  vistos=$((vistos + 1))
  if ! grep -qE '^[[:space:]]*access_log[[:space:]]+off[[:space:]]*;' <<<"$bloco"; then
    echo "✗ ws/access_log: $f registra o /ws — o token do ?token= fica em claro no log"
    fail=1
  fi
done

# Controlo da própria medição: se nenhum ficheiro tivesse a location, o
# portão passava a verde sem ler nada — foi o que aconteceu ao
# `check-capability-claims.sh` com um glob errado (V2 da onda 0).
if [ "$vistos" -eq 0 ]; then
  echo "✗ ws/access_log: nenhuma location do /ws encontrada — o portão não mediu nada"
  fail=1
fi

if [ "$fail" -eq 0 ]; then
  echo "✓ /ws sem access_log: $vistos ficheiro(s) de nginx — o token do ?token= não vai a disco"
fi
exit "$fail"
