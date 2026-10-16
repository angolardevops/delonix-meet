#!/usr/bin/env bash
# ============================================================
#  Fitness function: o /ws NÃO escreve o token no access log.
#
#  PORQUE EXISTE. O token da sala — e o de FONTE de um estúdio, que vive
#  12 h (`SOURCE_TOKEN_TTL_SECS`) — viaja no `?token=` do `/ws`. Um
#  `access_log` nesse caminho guarda a credencial em claro: quem leia os
#  logs do bordo publica media naquela sala até o token expirar, e uma
#  fonte está isenta do tecto de sockets e da quota da organização.
#
#  A regra já era da casa — o `deploy/nginx-delonix.conf` e o template do
#  ansible desligam-no com a nota «os tokens em ?token= NÃO devem ser
#  registados», e o `edge.conf` desliga-o seis linhas abaixo para o QR do
#  Linphone (R278). Faltava nos dois ficheiros que vão a produção, e —
#  pior — na camada por onde o `/ws` REALMENTE passa em K8s.
#
#  AS DUAS CAMADAS, e é isto que a primeira versão deste portão não via:
#   · nginx (compose e host): a `location` do `/ws` leva `access_log off`;
#   · INGRESS (k8s e Helm): o `/ws` vai Ingress → Service `delonix-server-ws`
#     DIRECTO, sem tocar no nginx do pod web. Ali o que desliga o log é a
#     anotação `nginx.ingress.kubernetes.io/enable-access-log: "false"`.
#     Medido a 2026-10-16: a primeira versão deste commit corrigiu o
#     `deploy/k8s/nginx.conf`, por onde o `/ws` nunca passa.
#
#  E O PORTÃO NÃO PODE PASSAR SEM MEDIR. A primeira versão casava a
#  `location` por `^/(ws` literal: com `location /ws {` o bloco saía vazio,
#  o ficheiro era saltado em silêncio e o portão dava verde com a
#  protecção retirada (medido). Agora: candidatos por padrão LARGO, uma
#  contagem ESPERADA por ficheiro, e chavetas contadas em vez de fechar no
#  primeiro `}`.
#
#  Uso:  bash scripts/check-ws-sem-access-log.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0

# Os nginx que servem o `/ws`, e quantas locations dele cada um tem. A
# contagem é parte do portão: um ficheiro que deixe de a cumprir falha, em
# vez de ser saltado em silêncio.
#
# Fora da lista de propósito: `deploy/nginx-dev.conf.template`, que não tem
# location do `/ws` (o `location /` apanha tudo) e registra por escolha.
NGINX="deploy/compose/edge.conf:1
deploy/k8s/nginx.conf:1
deploy/nginx-delonix.conf:1
deploy/ansible/roles/single_host/templates/nginx-delonix.conf.j2:1"

# Os manifests onde o `/ws` passa pelo INGRESS, e não por um nginx nosso.
INGRESS="deploy/k8s/04-ingress.yaml
deploy/helm/delonix-meet/templates/ingress.yaml"

# ---- camada 1: as locations dos nginx ----
while IFS=: read -r f esperadas; do
  [ -n "$f" ] || continue
  if [ ! -f "$f" ]; then
    echo "✗ ws/access_log: $f não existe — a lista do portão está velha"
    fail=1
    continue
  fi
  # Candidatos por padrão LARGO: qualquer `location` que mencione ws ou rtc,
  # em qualquer forma (`/ws`, `~ ^/(ws|rtc)`, `~* ^/(rtc|ws)$`, …). O veredicto
  # de cada bloco sai do PRÓPRIO awk: a primeira versão separava blocos com
  # `RS` e lia-os com `read`, que é linha a linha — cada linha sem
  # `access_log off` virava uma falha, e o portão cuspia dezenas delas.
  veredictos=$(awk '
    /^[[:space:]]*location[^{]*(\/|\()(ws|rtc)([|)\/ $]|$)/ && !dentro {
      dentro=1; prof=0; inicio=NR; tem=0
    }
    dentro {
      if ($0 ~ /^[[:space:]]*access_log[[:space:]]+off[[:space:]]*;/) tem=1
      n=gsub(/\{/,"{"); m=gsub(/\}/,"}"); prof += n - m
      if (prof <= 0) { print inicio (tem ? ":ok" : ":falta"); dentro=0 }
    }
  ' "$f")
  n=$(grep -c . <<<"$veredictos" || true)
  if [ "$n" -ne "$esperadas" ]; then
    echo "✗ ws/access_log: $f tem $n location(s) do /ws, esperavam-se $esperadas — o portão deixou de medir o que diz"
    fail=1
    continue
  fi
  while IFS=: read -r linha estado; do
    [ "$estado" = "falta" ] || continue
    echo "✗ ws/access_log: $f:$linha — a location do /ws registra; o token do ?token= fica em claro no log"
    fail=1
  done <<<"$veredictos"
done <<<"$NGINX"

# ---- camada 2: a anotação do Ingress ----
# Em K8s o `/ws` não passa por nenhum nginx nosso: o Ingress `delonix-ws`
# fala directo com o Service. É a anotação que desliga o log do ingress-nginx.
for f in $INGRESS; do
  if [ ! -f "$f" ]; then
    echo "✗ ws/access_log: $f não existe — a lista do portão está velha"
    fail=1
    continue
  fi
  bloco=$(awk '
    /^[[:space:]]*name:[[:space:]]*delonix-ws[[:space:]]*$/ { dentro=1 }
    dentro { print }
    dentro && /^spec:/ { exit }
  ' "$f")
  if [ -z "$bloco" ]; then
    echo "✗ ws/access_log: $f não tem o Ingress `delonix-ws` — a lista do portão está velha"
    fail=1
    continue
  fi
  if ! grep -qE 'enable-access-log:[[:space:]]*"?false"?' <<<"$bloco"; then
    echo "✗ ws/access_log: $f — o Ingress delonix-ws registra o /ws (falta enable-access-log: \"false\")"
    fail=1
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "✓ /ws sem access_log: 4 nginx + 2 ingress — o token do ?token= não vai a disco em nenhuma camada"
fi
exit "$fail"
