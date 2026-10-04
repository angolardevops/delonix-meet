#!/usr/bin/env bash
# Publica a borda do compose num túnel Pinggy, com um URL NOVO a cada execução,
# para o QR do Linphone se poder ler num telemóvel fora da rede local.
#
# Porquê: o QR (R278) é um URL https que o TELEMÓVEL abre. O nome do laboratório
# (`meet.ngolacloud.local`) é mDNS e o certificado do bootstrap é autoassinado:
# o telefone não resolve um nem confia no outro. O Pinggy dá um nome público e
# um certificado válido. O URL do QR sai da PRIMEIRA origem de `CORS_ORIGINS`,
# por isso este script escreve uma sobreposição que a põe à cabeça (a segunda
# mantém o browser desta máquina) — o `delonix compose` não interpola variáveis,
# como se explica em `compose-lan.sh`, e o URL só se conhece depois de o túnel
# estar de pé.
#
# A técnica é a do túnel do stage do `delonix-deploy` (um simples `ssh -R` com
# `u:Host` e `x:localservertls`). NÃO é esse túnel: aquele expõe o stage do
# cluster de produção e usa tokens que não vivem aqui. Este usa o Pinggy sem
# conta — dura 60 minutos e muda de endereço de cada vez (o que se quer num
# laboratório) — e aponta só à borda do compose local.
#
# O QUE ISTO PUBLICA: a borda inteira (consola, API, login) fica na Internet
# durante o túnel. Não há filtro por caminho: o Pinggy não o faz. Laboratório
# apenas, e `make tunnel-stop` quando acabar. Não leva `b:` (autenticação
# básica) de propósito: o Linphone não a envia ao descarregar o QR.
#
# O QUE ISTO NÃO RESOLVE: o registo SIP e o áudio. O proxy é UDP (5070) e o
# Pinggy não tunela UDP — o QR descarrega, mas o telefone só se regista se
# alcançar o 5070 por outro caminho (a rede local, com `make compose-up LAN_IP=…`).
#
# Uso:  bash scripts/compose-tunnel.sh up|down|status
set -euo pipefail
cd "$(dirname "$0")/.."

MEET_HOST=${MEET_HOST:-meet.ngolacloud.local}
EDGE_PORT=${EDGE_PORT:-8443}
TUNNEL_SERVER=${TUNNEL_SERVER:-a.pinggy.io}
WAIT_SECS=${TUNNEL_WAIT:-30}
DIR=deploy/compose/generated/tunnel
OVERRIDE=deploy/compose/generated/tunnel.yaml
PID_FILE=$DIR/ssh.pid
URL_FILE=$DIR/url
LOG=$DIR/ssh.log

alive() { [ -s "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; }

stop_tunnel() {
  if alive; then
    kill "$(cat "$PID_FILE")" 2>/dev/null || true
    # Dá um instante para o ssh fechar a ligação antes de se apagar o resto.
    for _ in 1 2 3 4 5; do alive || break; sleep 1; done
    alive && kill -9 "$(cat "$PID_FILE")" 2>/dev/null || true
  fi
  rm -f "$PID_FILE" "$URL_FILE" "$OVERRIDE"
}

case "${1:-}" in
  up)
    command -v ssh >/dev/null || { echo "✗ falta o cliente ssh" >&2; exit 1; }
    if alive; then
      echo "✗ já há um túnel de pé ($(cat "$URL_FILE" 2>/dev/null || echo '?')); «make tunnel-stop» primeiro" >&2
      exit 1
    fi
    # A borda tem de estar a ouvir, senão o túnel abre e devolve 502 a quem o usa.
    if ! (exec 3<>"/dev/tcp/127.0.0.1/$EDGE_PORT") 2>/dev/null; then
      echo "✗ nada a ouvir em 127.0.0.1:$EDGE_PORT — corre «make compose-up» primeiro" >&2
      exit 1
    fi
    mkdir -p "$DIR"
    : >"$LOG"
    # O reencaminhamento remoto (-R, porta 0) é o destino da borda; x:localservertls fala TLS com ela, com SNI
    # igual ao nome que o nginx espera; u:Host reescreve o cabeçalho Host (o
    # `server_name` do nginx escolhe o site por ele); x:https só aceita https.
    # `StrictHostKeyChecking=accept-new` com um ficheiro de chaves próprio: a
    # primeira vez fixa a chave do Pinggy, as seguintes recusam se ela mudar.
    nohup ssh -p 443 \
      -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$DIR/known_hosts" \
      -o ServerAliveInterval=30 -o ServerAliveCountMax=3 -o ExitOnForwardFailure=yes \
      -R "0:127.0.0.1:$EDGE_PORT" "$TUNNEL_SERVER" \
      "x:localservertls:$MEET_HOST" "u:Host:$MEET_HOST" "x:https" \
      >"$LOG" 2>&1 </dev/null &
    echo $! >"$PID_FILE"
    # O Pinggy escreve o URL no terminal, com cores; tira-se o ruído e apanha-se o 1.º https.
    url=
    for _ in $(seq 1 "$WAIT_SECS"); do
      alive || break
      url=$(sed 's/\x1b\[[0-9;?]*[A-Za-z]//g' "$LOG" | tr -d '\r' \
            | grep -oE 'https://[A-Za-z0-9._-]*pinggy[A-Za-z0-9._-]*' | head -1 || true)
      [ -n "$url" ] && break
      sleep 1
    done
    if [ -z "$url" ]; then
      echo "✗ o túnel não deu um URL em ${WAIT_SECS}s — fim do registo:" >&2
      tail -5 "$LOG" >&2 || true
      stop_tunnel
      exit 1
    fi
    echo "$url" >"$URL_FILE"
    cat >"$OVERRIDE" <<YAML
# Gerado por scripts/compose-tunnel.sh — NÃO versionar (o URL muda de cada vez).
# O QR do Linphone sai da PRIMEIRA origem: o telemóvel só alcança o túnel. A
# segunda mantém a consola desta máquina, com o nome do laboratório.
services:
  server:
    environment:
      CORS_ORIGINS: ${url},https://${MEET_HOST}:${EDGE_PORT}
YAML
    echo "$url"
    ;;
  down)
    stop_tunnel
    ;;
  status)
    if alive; then echo "túnel de pé: $(cat "$URL_FILE" 2>/dev/null || echo '?') (pid $(cat "$PID_FILE"))"; else echo "sem túnel"; exit 1; fi
    ;;
  *)
    echo "uso: $0 up|down|status" >&2
    exit 2
    ;;
esac
