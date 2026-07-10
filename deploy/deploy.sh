#!/usr/bin/env bash
# ============================================================
#  Delonix Meet — deploy de PRODUÇÃO (passo-a-passo idempotente)
#
#  Uso:   bash deploy/deploy.sh [--skip-tests] [--skip-build]
#
#  O que faz, por ordem:
#    0) pré-voo   — valida ferramentas, ficheiro de segredos e que
#                   os segredos NÃO são os defaults de dev
#    1) infra     — garante Postgres/Redis/coturn a correr (healthy)
#    2) backend   — cargo test + cargo build --release
#    3) migrações — corridas pelo servidor ao arrancar (verificadas)
#    4) frontend  — assets de IA (modelo RVM + runtime ONNX), build e
#                   publicação para o Nginx (/var/www/delonix)
#    5) serviços  — reinicia delonix-server (systemd)
#    6) smoke     — health + cabeçalhos de segurança
#
#  Ver deploy/DEPLOYMENT.md para o guia completo (TLS, nginx, coturn).
# ============================================================
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENV_FILE="${DELONIX_ENV_FILE:-/etc/delonix/delonix.env}"
WEB_ROOT="${DELONIX_WEB_ROOT:-/var/www/delonix}"
NODE_BIN="${DELONIX_NODE_BIN:-/home/walter/.nvm/versions/node/v25.0.0/bin}"
BASE_URL="${DELONIX_BASE_URL:-http://127.0.0.1:8180}"
SKIP_TESTS=0; SKIP_BUILD=0
for a in "$@"; do case "$a" in --skip-tests) SKIP_TESTS=1;; --skip-build) SKIP_BUILD=1;; esac; done

c()  { printf '\033[1;36m▶ %s\033[0m\n' "$*"; }        # passo
ok() { printf '\033[1;32m  ✓ %s\033[0m\n' "$*"; }
warn(){ printf '\033[1;33m  ! %s\033[0m\n' "$*"; }
die(){ printf '\033[1;31m  ✗ %s\033[0m\n' "$*" >&2; exit 1; }

# Assets de IA do efeito de fundo (matting RVM) — self-hosted e FORA do git.
# Corre DEPOIS do `npm ci` e ANTES do `npm run build` (o Vite copia public/).
# Não-fatal: se faltar, o efeito cai para o MediaPipe (cabelo menos bom).
RVM_MODEL_URL="${RVM_MODEL_URL:-https://github.com/PeterL1n/RobustVideoMatting/releases/download/v1.0.0/rvm_mobilenetv3_fp32.onnx}"
# SHA-256 fixado do rvm_mobilenetv3_fp32.onnx — protege contra corrupção e
# adulteração no trânsito (supply-chain). Override por env em atualizações.
RVM_MODEL_SHA256="${RVM_MODEL_SHA256:-88d4531297118f595bf2fd60f6f566aec2e559393802d1f436c380f0cbbd2828}"

# 0 se o ficheiro $1 tiver o SHA-256 $2. Sem sha256sum, avisa e não bloqueia.
sha256_ok() {
  command -v sha256sum >/dev/null || { warn "sha256sum indisponível — a saltar verificação de integridade"; return 0; }
  [ "$(sha256sum "$1" | awk '{print $1}')" = "$2" ]
}

ensure_matting_assets() {
  local web="$ROOT/web"
  # DOIS runtimes ONNX distintos (versões diferentes, dirs SEPARADOS):
  #  - /ort      → Whisper (transcrição), via @xenova/transformers (onnxruntime 1.14)
  #  - /ort-rvm  → matting RVM do efeito de fundo (onnxruntime-web 1.19)
  # Misturá-los faz o wasm não carregar. Ambos self-hosted, fora do git.
  local tf_src="$web/node_modules/@xenova/transformers/node_modules/onnxruntime-web/dist"
  local rvm_src="$web/node_modules/onnxruntime-web/dist"
  local model="$web/public/models/rvm/rvm_mobilenetv3_fp32.onnx"

  # 1a) Whisper: os 4 wasm da versão que a transformers.js espeta.
  if [ -d "$tf_src" ]; then
    mkdir -p "$web/public/ort"
    cp -f "$tf_src"/ort-wasm*.wasm "$web/public/ort/" 2>/dev/null || true
    ok "runtime ONNX do Whisper sincronizado em public/ort"
  else
    warn "onnxruntime-web (Whisper) não encontrado — transcrição pode falhar"
  fi

  # 1b) RVM: wasm (+ .jsep para WebGPU) do onnxruntime-web de topo.
  if [ -d "$rvm_src" ]; then
    mkdir -p "$web/public/ort-rvm"
    for f in ort-wasm-simd-threaded.wasm ort-wasm-simd-threaded.mjs \
             ort-wasm-simd-threaded.jsep.wasm ort-wasm-simd-threaded.jsep.mjs; do
      [ -f "$rvm_src/$f" ] && cp -f "$rvm_src/$f" "$web/public/ort-rvm/$f"
    done
    ok "runtime ONNX do RVM sincronizado em public/ort-rvm"
  else
    warn "onnxruntime-web não instalado — matting RVM indisponível (fallback MediaPipe)"
  fi

  # 2) Modelo RVM (~15MB) — presença + INTEGRIDADE (SHA-256 fixado).
  if [ -f "$model" ] && sha256_ok "$model" "$RVM_MODEL_SHA256"; then
    ok "modelo RVM presente e íntegro ($(du -h "$model" | cut -f1))"
  else
    [ -f "$model" ] && warn "modelo RVM em falta/corrompido — a (re)descarregar"
    mkdir -p "$(dirname "$model")"
    printf '  ↓ a descarregar modelo RVM (~15MB)…\n'
    if curl -fsSL -o "$model" "$RVM_MODEL_URL" && sha256_ok "$model" "$RVM_MODEL_SHA256"; then
      ok "modelo RVM descarregado e verificado (SHA-256)"
    else
      rm -f "$model"
      warn "modelo RVM inválido (download falhou ou SHA-256 não confere) — fallback MediaPipe"
    fi
  fi
}

# ---- 0) Pré-voo ----------------------------------------------------------
c "0/6  Pré-voo"
for t in cargo docker rsync curl; do command -v "$t" >/dev/null || die "falta a ferramenta: $t"; done
[ -d "$NODE_BIN" ] && export PATH="$NODE_BIN:$PATH"
command -v npm >/dev/null || die "npm não está no PATH (ajusta DELONIX_NODE_BIN)"

[ -f "$ENV_FILE" ] || die "segredos em falta: $ENV_FILE (ver deploy/delonix.env.example)"
# shellcheck disable=SC1090
set -a; source "$ENV_FILE"; set +a
[ -n "${JWT_SECRET:-}" ]  || die "JWT_SECRET não definido em $ENV_FILE"
[ "${#JWT_SECRET}" -ge 32 ] || die "JWT_SECRET < 32 bytes"
[ "${JWT_SECRET}"  != "dev-only-secret-change-in-production" ] || die "JWT_SECRET ainda é o default de dev"
[ "${TURN_SECRET:-}" != "delonix_turn_dev_secret" ] || die "TURN_SECRET ainda é o default de dev"
case "${DATABASE_URL:-}" in *delonix_dev*) die "DATABASE_URL ainda usa a password de dev";; esac
[ "${DELONIX_ALLOW_INSECURE:-}" != "1" ] || die "DELONIX_ALLOW_INSECURE=1 em produção — remover de $ENV_FILE"
ok "segredos presentes e não-default"

# ---- 1) Infraestrutura ---------------------------------------------------
c "1/6  Infraestrutura (Postgres/Redis/coturn)"
if systemctl --user is-enabled delonix-infra >/dev/null 2>&1; then
  systemctl --user start delonix-infra && ok "delonix-infra a correr (systemd)"
else
  ( cd "$ROOT" && docker compose up -d --wait ) && ok "docker compose up (healthy)"
fi

# ---- 2) Backend ----------------------------------------------------------
if [ "$SKIP_BUILD" = 0 ]; then
  c "2/6  Backend (test + build release)"
  [ "$SKIP_TESTS" = 1 ] || ( cd "$ROOT/server" && cargo test --release >/dev/null && ok "testes passaram" )
  ( cd "$ROOT/server" && cargo build --release ) && ok "binário compilado (target/release)"
else
  c "2/6  Backend — ignorado (--skip-build)"
fi

# ---- 3) Migrações --------------------------------------------------------
c "3/6  Migrações"
echo "  (corridas automaticamente pelo servidor ao arrancar; ver passo 5)"
ok "$(ls "$ROOT"/server/migrations/*.sql | wc -l | tr -d ' ') ficheiros de migração presentes"

# ---- 4) Frontend ---------------------------------------------------------
if [ "$SKIP_BUILD" = 0 ]; then
  c "4/6  Frontend (build + publicar)"
  ( cd "$ROOT/web" && npm ci --silent )
  ensure_matting_assets   # modelo RVM + runtime ONNX (self-hosted, antes do build)
  bash "$ROOT/deploy/fetch-whisper.sh"   # modelo Whisper-tiny (transcrição local, self-hosted)
  ( cd "$ROOT/web" && npm run build >/dev/null )
  sudo rsync -a --delete "$ROOT/web/dist/" "$WEB_ROOT/"
  sudo chown -R www-data:www-data "$WEB_ROOT"
  ok "SPA publicada em $WEB_ROOT"
else
  c "4/6  Frontend — ignorado (--skip-build)"
fi

# ---- 5) Serviços ---------------------------------------------------------
c "5/6  Reiniciar o servidor"
systemctl --user restart delonix-server
for i in $(seq 1 15); do
  sleep 1
  [ "$(curl -s -o /dev/null -w '%{http_code}' "$BASE_URL/health" || true)" = "200" ] && break
  [ "$i" = 15 ] && die "o servidor não respondeu 200 em /health (ver: journalctl --user -u delonix-server -e)"
done
ok "delonix-server ativo (health 200)"

# ---- 6) Smoke tests ------------------------------------------------------
c "6/6  Smoke tests"
H="$(curl -s -D - -o /dev/null "$BASE_URL/health")"
for want in "x-frame-options" "x-content-type-options" "referrer-policy"; do
  echo "$H" | grep -qi "$want" && ok "cabeçalho $want presente" || die "falta cabeçalho $want"
done
printf '\033[1;32m\n✔ Deploy concluído.\033[0m Recarrega o browser. Valida TLS/nginx em deploy/DEPLOYMENT.md.\n'
