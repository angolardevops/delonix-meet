#!/usr/bin/env bash
# Arranca o backend Delonix em modo DEV.
#
# Desde o endurecimento de segurança, o servidor faz fail-closed: em produção
# exige JWT_SECRET / TURN_SECRET / DATABASE_URL fortes. Em desenvolvimento,
# DELONIX_ALLOW_INSECURE=1 permite os defaults de dev. NUNCA usar esta flag
# em produção — lá, define segredos reais no ambiente/systemd.
set -euo pipefail
cd "$(dirname "$0")/../server"
export DELONIX_ALLOW_INSECURE=1
exec ./target/release/delonix-server
