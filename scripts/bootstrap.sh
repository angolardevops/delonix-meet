#!/usr/bin/env bash
# ============================================================
#  make bootstrap — prepara uma máquina acabada de clonar.
#
#  Idempotente: corre-se as vezes que for preciso; só faz o que falta.
#    1. confere as ferramentas (e diz como instalar as que faltam);
#    2. instala o `helm` DENTRO do projecto (.tools/bin), com o checksum
#       publicado conferido — nada vai para o sistema;
#    3. dependências do frontend (npm ci) e do backend (cargo fetch);
#    4. `.env` com segredos ALEATÓRIOS (nunca os de exemplo);
#    5. certificado TLS de meet.ngolacloud.local para o compose e o cluster.
#
#  Não mexe em /etc/hosts nem em nada que peça sudo: no fim diz o que falta
#  fazer à mão.
# ============================================================
set -euo pipefail
cd "$(dirname "$0")/.."

MEET_HOST=${MEET_HOST:-meet.ngolacloud.local}
HELM_VERSION=${HELM_VERSION:-v3.16.4}
TOOLS_BIN="$PWD/.tools/bin"
export PATH="$TOOLS_BIN:$PATH"

c=$'\033[1;36m'; g=$'\033[1;32m'; y=$'\033[1;33m'; r=$'\033[1;31m'; z=$'\033[0m'
faltam=0

tem() { command -v "$1" >/dev/null 2>&1; }
ok()  { printf "  %s✓%s %s\n" "$g" "$z" "$1"; }
avisa() { printf "  %s!%s %s\n" "$y" "$z" "$1"; }
falta() { printf "  %s✗%s %s\n" "$r" "$z" "$1"; faltam=$((faltam + 1)); }

printf "%s▶ ferramentas%s\n" "$c" "$z"
for t in git make openssl curl tar; do
  if tem "$t"; then ok "$t"; else falta "$t — instala pelo gestor de pacotes do sistema"; fi
done
if tem cargo; then ok "cargo ($(cargo --version | cut -d' ' -f2))"; else falta "cargo — https://rustup.rs"; fi
if tem node && tem npm; then ok "node $(node --version) / npm $(npm --version)"; else falta "node + npm (>= 22) — https://github.com/nvm-sh/nvm"; fi
if tem delonix; then
  ok "delonix ($(delonix --version | head -1 | cut -d' ' -f2)) — build, compose e cluster"
elif tem docker; then
  avisa "delonix ausente — uso o docker para build e compose; o «make cluster» precisa do delonix"
else
  falta "delonix (ou docker) — sem motor de contentores não há build, compose nem cluster"
fi
if tem kubectl; then ok "kubectl"; else falta "kubectl — necessário para o «make cluster»"; fi
if tem mkcert; then ok "mkcert (certificado confiado pelo browser)"; else avisa "mkcert ausente — o certificado será self-signed e o browser vai avisar"; fi

# ---- helm: dentro do projecto, com o checksum publicado conferido ----
printf "%s▶ helm %s%s\n" "$c" "$HELM_VERSION" "$z"
if tem helm; then
  ok "helm ($(helm version --short 2>/dev/null))"
else
  so=$(uname -s | tr '[:upper:]' '[:lower:]')
  case "$(uname -m)" in
    x86_64) arq=amd64 ;;
    aarch64 | arm64) arq=arm64 ;;
    *) arq="" ;;
  esac
  if [ -z "$arq" ]; then
    falta "helm — arquitectura $(uname -m) não prevista; instala à mão: https://helm.sh/docs/intro/install/"
  else
    pacote="helm-${HELM_VERSION}-${so}-${arq}.tar.gz"
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    if curl -fsSL "https://get.helm.sh/${pacote}" -o "$tmp/$pacote" &&
      curl -fsSL "https://get.helm.sh/${pacote}.sha256sum" -o "$tmp/$pacote.sha256sum" &&
      (cd "$tmp" && sha256sum -c "$pacote.sha256sum" >/dev/null); then
      mkdir -p "$TOOLS_BIN"
      tar -xzf "$tmp/$pacote" -C "$tmp"
      install -m 0755 "$tmp/${so}-${arq}/helm" "$TOOLS_BIN/helm"
      ok "helm instalado em .tools/bin (checksum conferido)"
    else
      falta "helm — não consegui descarregar ou o checksum não bate; instala à mão: https://helm.sh/docs/intro/install/"
    fi
  fi
fi

if [ "$faltam" -gt 0 ]; then
  printf "\n%s✗ faltam %s ferramenta(s) — instala-as e volta a correr «make bootstrap».%s\n" "$r" "$faltam" "$z"
  exit 1
fi

# ---- dependências ----
printf "%s▶ dependências do frontend (npm ci)%s\n" "$c" "$z"
if [ -d web/node_modules ] && [ web/node_modules/.package-lock.json -nt web/package-lock.json ]; then
  ok "web/node_modules em dia"
else
  (cd web && npm ci --no-audit --no-fund >/dev/null)
  ok "web/node_modules instalado"
fi
printf "%s▶ dependências do backend (cargo fetch)%s\n" "$c" "$z"
(cd server && cargo fetch --locked >/dev/null 2>&1) && ok "crates descarregados" || avisa "cargo fetch falhou (sem rede?) — o primeiro build descarrega"

# ---- .env com segredos aleatórios ----
printf "%s▶ .env%s\n" "$c" "$z"
segredo() { openssl rand -hex "$1"; }
if [ ! -f .env ]; then
  cp deploy/compose/env.example .env
  chmod 600 .env
  ok ".env criado a partir de deploy/compose/env.example"
fi
# MEET_ADMIN_PASSWORD: a conta de validação que o `make seed` cria.
# VOICE_ADMIN_PASSWORD: as interfaces de administração do Kamailio e do PBX.
# POSTGRES_REPLICATION_PASSWORD: só o `make prod` (Postgres com réplicas) o usa.
for par in POSTGRES_PASSWORD:24 JWT_SECRET:32 TURN_SECRET:24 PROVISIONING_SECRET:24 VOICE_INTERNAL_SECRET:32 TELEPHONY_ESL_PASSWORD:32 \
  MEET_ADMIN_PASSWORD:12 VOICE_ADMIN_PASSWORD:12 VOICE_CENTRAL_PASSWORD:16 POSTGRES_REPLICATION_PASSWORD:24; do
  nome=${par%%:*}
  bytes=${par##*:}
  if grep -qE "^${nome}=.+" .env; then
    continue
  fi
  valor=$(segredo "$bytes")
  if grep -qE "^${nome}=" .env; then
    sed -i "s|^${nome}=.*|${nome}=${valor}|" .env
  else
    printf '%s=%s\n' "$nome" "$valor" >>.env
  fi
  ok "$nome gerado"
done
# DATA_ENCRYPTION_KEYS: a chave que cifra os segredos guardados na base. Não é
# hexadecimal como os outros — o formato é `kid:base64` de 32 bytes — e o
# servidor recusa arrancar sem ela fora do modo de desenvolvimento. NUNCA se
# regenera: perder a chave é perder todos os segredos que ela cifrou.
if ! grep -qE "^DATA_ENCRYPTION_KEYS=.+" .env; then
  chave="lab:$(openssl rand -base64 32)"
  if grep -qE "^DATA_ENCRYPTION_KEYS=" .env; then
    sed -i "s|^DATA_ENCRYPTION_KEYS=.*|DATA_ENCRYPTION_KEYS=${chave}|" .env
  else
    printf 'DATA_ENCRYPTION_KEYS=%s\n' "$chave" >>.env
  fi
  ok "DATA_ENCRYPTION_KEYS gerado"
fi
# O que se DERIVA dos segredos: o URL da base de dados do compose e a
# configuração do relay. Reescrevem-se sempre, para nunca ficarem desencontrados.
valor_de() { sed -n "s/^$1=//p" .env | head -1; }
db="postgres://delonix:$(valor_de POSTGRES_PASSWORD)@delonix-postgres:5432/delonix_meet"
if grep -qE "^DATABASE_URL=" .env; then
  sed -i "s|^DATABASE_URL=.*|DATABASE_URL=${db}|" .env
else
  printf 'DATABASE_URL=%s\n' "$db" >>.env
fi
GEN=deploy/compose/generated
mkdir -p "$GEN/voice-tls"
cat >"$GEN/turnserver.conf" <<CONF
# Gerado por «make bootstrap» — NÃO versionar (tem o segredo do relay).
listening-port=3478
realm=${MEET_HOST}
use-auth-secret
static-auth-secret=$(valor_de TURN_SECRET)
no-tls
no-dtls
no-cli
min-port=49300
max-port=49340
fingerprint
log-file=stdout
CONF
# Interface de gestão do Kamailio: basic auth na borda (utilizador «admin»).
printf 'admin:%s\n' "$(openssl passwd -apr1 "$(valor_de VOICE_ADMIN_PASSWORD)")" >"$GEN/voice.htpasswd"
# Interface HTTP do PBX de cliente (Asterisk): ARI com o mesmo acesso.
cat >"$GEN/pbx-http.conf" <<CONF
; Gerado por «make bootstrap».
[general]
enabled = yes
bindaddr = 0.0.0.0
bindport = 8088
enablestatic = no
CONF
cat >"$GEN/pbx-ari.conf" <<CONF
; Gerado por «make bootstrap» — NÃO versionar (tem a password).
[general]
enabled = yes
pretty = yes

[admin]
type = user
read_only = no
password_format = plain
password = $(valor_de VOICE_ADMIN_PASSWORD)
CONF
# A conta com que o PBX de laboratório entra no bordo como CENTRAL da
# organização (ADR-0016): a mesma que o `make seed` grava no «Registo SIP».
{
  printf '; Gerado por «make bootstrap» a partir de voice/pbx-cliente/central.conf.tmpl — NÃO versionar (tem a password).\n'
  sed -e '/^;/d' -e "s/__BORDO__/delonix-kamailio/" -e "s/__PASSWORD__/$(valor_de VOICE_CENTRAL_PASSWORD)/" \
    voice/pbx-cliente/central.conf.tmpl
} >"$GEN/pbx-central.conf"
# Certificado self-signed do bordo SIP (Kamailio) no compose. O nome vai também
# no SAN: é por ele que a central confere o certificado. Um certificado de um
# bootstrap anterior, sem SAN, é substituído (é de laboratório e self-signed).
san=$(openssl x509 -in "$GEN/voice-tls/tls.crt" -noout -ext subjectAltName 2>/dev/null) || san=
if ! grep -q "DNS:delonix-kamailio" <<<"$san"; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 825 -subj "/CN=delonix-kamailio" \
    -addext "subjectAltName=DNS:delonix-kamailio" \
    -keyout "$GEN/voice-tls/tls.key" -out "$GEN/voice-tls/tls.crt" 2>/dev/null
fi
# Lidos dentro de contentores por outros utilizadores: têm de ser legíveis.
# A pasta não sai da máquina (gitignore) e os segredos são os deste laboratório.
chmod -R a+rX "$GEN"
ok ".env e deploy/compose/generated/ prontos (fora do git)"

# ---- certificado de meet.ngolacloud.local ----
printf "%s▶ certificado TLS de %s%s\n" "$c" "$MEET_HOST" "$z"
mkdir -p deploy/certs
crt="deploy/certs/${MEET_HOST}.crt"
key="deploy/certs/${MEET_HOST}.key"
if [ -f "$crt" ] && [ -f "$key" ]; then
  ok "já existe"
elif tem mkcert; then
  mkcert -cert-file "$crt" -key-file "$key" "$MEET_HOST" >/dev/null 2>&1
  chmod 600 "$key"
  ok "gerado com mkcert (corre «mkcert -install» uma vez para o browser confiar)"
else
  openssl req -x509 -newkey rsa:2048 -nodes -days 825 \
    -keyout "$key" -out "$crt" -subj "/CN=${MEET_HOST}" \
    -addext "subjectAltName=DNS:${MEET_HOST}" 2>/dev/null
  chmod 600 "$key"
  avisa "self-signed (sem mkcert) — o browser vai avisar"
fi

printf "\n%s✓ máquina pronta.%s\n" "$g" "$z"
printf "  A seguir:  %smake dev%s (desenvolver)  ·  %smake build%s (imagens)  ·  %smake cluster%s (stack completo)\n" "$y" "$z" "$y" "$z" "$y" "$z"
if ! grep -qE "[[:space:]]${MEET_HOST}([[:space:]]|\$)" /etc/hosts 2>/dev/null; then
  printf "  %sFalta um passo manual%s (pede sudo, por isso não o faço):\n" "$y" "$z"
  printf "    echo '127.0.0.1 %s' | sudo tee -a /etc/hosts\n" "$MEET_HOST"
  printf "    (serve o compose em :8443 e o cluster em :443 — os dois publicam em 127.0.0.1)\n"
fi
