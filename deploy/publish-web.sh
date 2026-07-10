#!/usr/bin/env bash
# Recompila o frontend e publica-o para o Nginx servir. Correr após mudanças no web/.
set -euo pipefail
# Usa DELONIX_NODE_BIN se definido; caso contrário auto-detecta via nvm.
if [[ -n "${DELONIX_NODE_BIN:-}" ]]; then
  export PATH="${DELONIX_NODE_BIN}:${PATH}"
elif [[ -s "${HOME}/.nvm/nvm.sh" ]]; then
  # shellcheck disable=SC1091
  . "${HOME}/.nvm/nvm.sh"
fi
command -v npm >/dev/null || { echo "✗ npm não encontrado — define DELONIX_NODE_BIN ou instala via nvm" >&2; exit 1; }
cd "$(dirname "$0")/../web"
echo "→ a compilar…"
npm run build
echo "→ a publicar em /var/www/delonix…"
sudo rsync -a --delete dist/ /var/www/delonix/
sudo chown -R www-data:www-data /var/www/delonix
echo "✓ publicado. (Nginx serve já os novos ficheiros; recarrega o browser.)"
