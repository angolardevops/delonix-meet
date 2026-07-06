#!/usr/bin/env bash
# Recompila o frontend e publica-o para o Nginx servir. Correr após mudanças no web/.
set -euo pipefail
export PATH="/home/walter/.nvm/versions/node/v25.0.0/bin:$PATH"
cd "$(dirname "$0")/../web"
echo "→ a compilar…"
npm run build
echo "→ a publicar em /var/www/delonix…"
sudo rsync -a --delete dist/ /var/www/delonix/
sudo chown -R www-data:www-data /var/www/delonix
echo "✓ publicado. (Nginx serve já os novos ficheiros; recarrega o browser.)"
