#!/usr/bin/env bash
# Gera o ficheiro de sobreposição do compose que expõe os RAMAIS à rede local.
#
# O compose.yaml não interpola variáveis (o `delonix compose` não o faz), e o
# IP da máquina na rede local não se pode versionar: este script escreve-o num
# ficheiro gerado (deploy/compose/generated/lan.yaml), que o `make compose-up
# LAN_IP=…` junta ao compose.yaml.
#
# Só o perfil dos ramais (5070) e uma gama pequena de portas de áudio. É um
# laboratório: os ramais autenticam por SIP Digest, mas a sinalização vai em
# claro e as chaves SRTP (SDES) vão nela — não é para uma rede em que não se confia.
set -euo pipefail
: "${LAN_IP:?uso: LAN_IP=<ip desta máquina na rede local> $0}"
[[ "$LAN_IP" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "LAN_IP não é um IPv4: $LAN_IP" >&2; exit 1; }

# ---- Certificado para a rede local -----------------------------------------
# O QR do Linphone (R278) é um URL https que o TELEMÓVEL abre: o nome do
# laboratório não resolve lá e o certificado auto-assinado do bootstrap só
# cobre esse nome. Aqui nasce uma raiz de laboratório (a instalar UMA vez no
# telemóvel) e um certificado da borda que cobre o nome e este IP. A raiz só
# se refaz se faltar; a folha refaz-se quando o IP muda.
cd "$(dirname "$0")/.."
MEET_HOST=${MEET_HOST:-meet.ngolacloud.local}
TLS=deploy/compose/generated/lan-tls
mkdir -p "$TLS"
if [ ! -s "$TLS/ca.crt" ] || [ ! -s "$TLS/ca.key" ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 825 \
    -subj "/O=Delonix Meet (laboratorio)/CN=Raiz de laboratorio do Delonix Meet" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" \
    -keyout "$TLS/ca.key" -out "$TLS/ca.crt" 2>/dev/null
  rm -f "$TLS/tls.crt"
fi
cobre=$(openssl x509 -in "$TLS/tls.crt" -noout -checkip "$LAN_IP" 2>/dev/null) || cobre=
if ! grep -q 'does match' <<<"$cobre"; then
  openssl req -newkey rsa:2048 -nodes -subj "/CN=${MEET_HOST}" \
    -keyout "$TLS/tls.key" -out "$TLS/tls.csr" 2>/dev/null
  printf 'subjectAltName=DNS:%s,IP:%s\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' \
    "$MEET_HOST" "$LAN_IP" >"$TLS/tls.ext"
  openssl x509 -req -in "$TLS/tls.csr" -CA "$TLS/ca.crt" -CAkey "$TLS/ca.key" -CAcreateserial \
    -days 397 -extfile "$TLS/tls.ext" -out "$TLS/tls.crt" 2>/dev/null
  rm -f "$TLS/tls.csr" "$TLS/tls.ext"
fi
# A chave da raiz fica só para o dono. A da borda tem de ser lida pelo nginx do
# contentor (outro uid, rootless), como as restantes de deploy/compose/generated.
chmod 600 "$TLS/ca.key"; chmod 644 "$TLS/tls.key" "$TLS"/*.crt
# Opcional (ADR-0023, S-01): com PUSH_LAB_URL=http://<ip>:<porta>/push o servidor entrega os pedidos de
# «acordar» de um aparelho `lab` a esse URL (a prova scripts/ramais-push-real-prova.py é quem o serve), o
# FreeSWITCH passa a segurar 20 s a chamada para um ramal sem registo, e o host do URL entra na isenção da
# guarda de saída (só ele). Sem a variável nada disto existe.
PUSH_SERVER_ENV=""; PUSH_FS_ENV=""; PUSH_HOSTS=""
if [ -n "${PUSH_LAB_URL:-}" ]; then
  [[ "$PUSH_LAB_URL" =~ ^http://([0-9.]+):[0-9]+/[A-Za-z0-9/_-]*$ ]] || { echo "PUSH_LAB_URL tem de ser http://<ip>:<porta>/<caminho>: $PUSH_LAB_URL" >&2; exit 1; }
  PUSH_SERVER_ENV+=$'\n      PUSH_LAB_URL: '"${PUSH_LAB_URL}"
  PUSH_HOSTS="${BASH_REMATCH[1]}"
  PUSH_FS_ENV=$'\n      DELONIX_PUSH_WAIT_SECS: "20"'
fi
# Opcional (ADR-0023): com PUSH_DELONIX_URL=http://<ip>:<porta> e PUSH_DELONIX_KEY=dpk_… o servidor cunha os aparelhos
# `delonix` nesse delonix-push e acorda-os por lá; o FreeSWITCH passa a segurar 20 s as chamadas. A chave é um segredo:
# vai para o ficheiro gerado (que não se versiona) e nunca para o log.
if [ -n "${PUSH_DELONIX_URL:-}" ] || [ -n "${PUSH_DELONIX_KEY:-}" ]; then
  [[ "${PUSH_DELONIX_URL:-}" =~ ^http://([0-9.]+):[0-9]+$ ]] || { echo "PUSH_DELONIX_URL tem de ser http://<ip>:<porta>: ${PUSH_DELONIX_URL:-}" >&2; exit 1; }
  DELONIX_PUSH_HOST="${BASH_REMATCH[1]}"
  [[ "${PUSH_DELONIX_KEY:-}" =~ ^dpk_[0-9a-f]{64}$ ]] || { echo "PUSH_DELONIX_KEY tem de ser a chave dpk_… do projecto" >&2; exit 1; }
  PUSH_SERVER_ENV+=$'\n      PUSH_DELONIX_URL: '"${PUSH_DELONIX_URL}"$'\n      PUSH_DELONIX_KEY: '"${PUSH_DELONIX_KEY}"
  PUSH_HOSTS="${PUSH_HOSTS:+$PUSH_HOSTS,}${DELONIX_PUSH_HOST}"
  PUSH_FS_ENV=$'\n      DELONIX_PUSH_WAIT_SECS: "20"'
fi
[ -n "$PUSH_HOSTS" ] && PUSH_SERVER_ENV+=$'\n      OUTBOUND_ALLOW_HOSTS: '"${PUSH_HOSTS}"
# Opcional: LAB_DB_NAME=<nome> aponta o servidor para OUTRA base do mesmo Postgres (criada à parte), em vez de
# `delonix_meet`. Serve para provar uma branch cujas migrações não encaixam na base que o laboratório já tem, sem a apagar.
# A URL vem do `.env` (só muda o nome da base) e vai para o ficheiro gerado, que não se versiona.
if [ -n "${LAB_DB_NAME:-}" ]; then
  [[ "$LAB_DB_NAME" =~ ^[a-z_][a-z0-9_]{0,40}$ ]] || { echo "LAB_DB_NAME inválido: $LAB_DB_NAME" >&2; exit 1; }
  DB_URL=$(sed -n 's/^DATABASE_URL=//p' .env | head -1)
  [ -n "$DB_URL" ] || { echo "sem DATABASE_URL no .env" >&2; exit 1; }
  PUSH_SERVER_ENV+=$'\n      DATABASE_URL: '"${DB_URL%/*}/${LAB_DB_NAME}"
fi
cat <<YAML
# Gerado por scripts/compose-lan.sh — NÃO versionar (tem o IP desta máquina).
services:
  # A consola mostra o endereço onde o softphone se liga: na rede local é este
  # IP — o nome do compose.yaml só resolve na própria máquina.
  # O QR do Linphone sai da PRIMEIRA origem: na rede local tem de ser este IP,
  # que é o que o telemóvel alcança. A segunda mantém o browser desta máquina.
  server:
    environment:
      VOICE_RAMAIS_PUBLIC_HOST: ${LAN_IP}
      # O QR do Linphone manda o telefone registar por TLS, na porta TLS do perfil dos ramais.
      VOICE_RAMAIS_PUBLIC_PORT: "5071"
      VOICE_RAMAIS_PUBLIC_TRANSPORT: tls
      CORS_ORIGINS: https://${LAN_IP}:8443,https://${MEET_HOST}:8443${PUSH_SERVER_ENV}
  # A borda fica também na rede local, com o certificado que cobre este IP. Em
  # 8080 serve a raiz de laboratório, para o telemóvel a ir buscar e instalar.
  edge:
    ports:
      - "${LAN_IP}:8443:8443"
      - "${LAN_IP}:8080:8080"
    volumes:
      - ./deploy/compose/generated/lan-tls/tls.crt:/etc/nginx/tls/tls.crt:ro
      - ./deploy/compose/generated/lan-tls/tls.key:/etc/nginx/tls/tls.key:ro
      - ./deploy/compose/generated/lan-tls/ca.crt:/etc/nginx/lab-ca.crt:ro
  freeswitch:
    environment:
      DELONIX_EXTERNAL_IP: ${LAN_IP}
      # TLS nos ramais (entrypoint, passo 9b), com o MESMO certificado da borda: cobre este IP.
      # O 5070 (UDP/TCP em claro) fica de reserva para o softphone de linha de comandos das provas.
      DELONIX_RAMAIS_TLS_PORT: "5071"
      DELONIX_RTP_MIN: "20000"
      DELONIX_RTP_MAX: "20100"${PUSH_FS_ENV}
    volumes:
      - ./deploy/compose/generated/lan-tls/tls.crt:/tls-ramais/tls.crt:ro
      - ./deploy/compose/generated/lan-tls/tls.key:/tls-ramais/tls.key:ro
    ports:
      - "${LAN_IP}:5070:5070/udp"
      - "${LAN_IP}:5070:5070/tcp"
      - "${LAN_IP}:5071:5071/tcp"
      - "${LAN_IP}:20000-20100:20000-20100/udp"
YAML
