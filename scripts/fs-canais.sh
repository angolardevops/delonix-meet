#!/usr/bin/env bash
# FreeSWITCH real da prova da ponte telefone↔sala (frente D, ADR-0010).
# up: copia a base segura, aplica voice/freeswitch/canais-prova, arranca `fs-canais`.
# down: pára e remove APENAS o contentor `fs-canais`.
set -euo pipefail
cd "$(dirname "$0")/.."
NAME=fs-canais
DIR=.fs-canais
BASE=${FS_BASE_CONF:-../../freeswitch-build/conf}
case "${1:-}" in
  up)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    rm -rf "$DIR"; mkdir -p "$DIR/recordings"; chmod 777 "$DIR/recordings"
    # `tls/` é do root na base e não faz falta (a prova não usa TLS).
    mkdir -p "$DIR/conf"
    tar -C "$BASE" --exclude=./tls -cf - . | tar -C "$DIR/conf" -xf -
    rm -f "$DIR/conf/sip_profiles/internal.xml" "$DIR/conf/sip_profiles/external.xml"
    cp -r voice/freeswitch/canais-prova/sip_profiles/. "$DIR/conf/sip_profiles/"
    cp -r voice/freeswitch/canais-prova/dialplan/. "$DIR/conf/dialplan/"
    PW=$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')
    echo -n "$PW" > "$DIR/esl-password.txt"; chmod 600 "$DIR/esl-password.txt"
    sed -i -E "s#(name=\"listen-port\" value=)\"[0-9]+\"#\1\"8221\"#; s#(name=\"password\" value=)\"[^\"]*\"#\1\"$PW\"#" \
      "$DIR/conf/autoload_configs/event_socket.conf.xml"
    sed -i -E 's#<!-- <param name="rtp-start-port" value="16384"/> -->#<param name="rtp-start-port" value="32800"/>#; s#<!-- <param name="rtp-end-port" value="32768"/> -->#<param name="rtp-end-port" value="33000"/>#' \
      "$DIR/conf/autoload_configs/switch.conf.xml"
    docker run -d --name "$NAME" --network host \
      -v "$PWD/$DIR/conf:/usr/local/freeswitch/etc/freeswitch" \
      -v "$PWD/$DIR/recordings:/usr/local/freeswitch/recordings" \
      delonix-dev/freeswitch:1.11.3 freeswitch -nonat -nf -nc >/dev/null
    for _ in $(seq 1 60); do
      if docker exec "$NAME" fs_cli -P 8221 -p "$PW" -x "sofia status" 2>/dev/null | grep -q "carrier"; then
        echo "fs-canais pronto (ESL 127.0.0.1:8221)"; exit 0
      fi
      sleep 1
    done
    echo "fs-canais não ficou pronto"; docker logs --tail 40 "$NAME"; exit 1
    ;;
  down)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    ;;
  *) echo "uso: $0 up|down"; exit 2 ;;
esac
