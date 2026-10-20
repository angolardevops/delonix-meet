#!/usr/bin/env bash
# Envio e limpeza da consolidação de 2026-10-20 — ver docs/sprint-consolidado-2026-10-20.md
#
# PARA O DONO CORRER. O harness do Claude recusa `git push` incondicionalmente,
# por isso estes comandos ficam aqui em vez de serem executados por ele.
#
#   ./scripts/consolidado-2026-10-20-enviar.sh enviar      # empurra ramos + develop
#   ./scripts/consolidado-2026-10-20-enviar.sh confirmar   # lê os SHA do remoto
#   ./scripts/consolidado-2026-10-20-enviar.sh limpar      # worktrees + ramos locais
#
# A ordem importa: `limpar` RECUSA-SE a correr enquanto `confirmar` não achar
# todos os ramos no remoto, porque sem push eles são a única cópia de 21 commits.

set -euo pipefail

REMOTO=delonix-meet-origin
RAIZ=/home/walter/workspaces/ngolacloud/delonix-meet

VIVOS=(
  integra/cena-de-marca
  integra/tecto-nao-falha-aberto
  integra/gravacao-iso
  integra/legendas-soberanas
  integra/gravacao-iso-registo-kind
)

FUNDIDOS=(
  integra/paridade-medida
  integra/dial-in-e2ee
  integra/divida-do-356
  integra/portao-nao-mente
)

WORKTREES=(
  /home/walter/workspaces/ngolacloud-wt/meet-cena
  /home/walter/workspaces/ngolacloud-wt/meet-paridade
  /home/walter/workspaces/ngolacloud-wt/meet-tecto
)

cd "$RAIZ"

enviar() {
  for b in "${VIVOS[@]}"; do
    echo "=== $b"
    git push "$REMOTO" "$b:$b"
  done
  echo "=== develop"
  git push "$REMOTO" develop:develop
}

# Devolve 0 só se TODOS os ramos (e a develop) existirem no remoto.
confirmar() {
  local falta=0
  for b in "${VIVOS[@]}" develop; do
    local sha
    sha=$(git ls-remote --heads "$REMOTO" "$b" | cut -f1)
    if [ -z "$sha" ]; then
      printf '%-42s AUSENTE\n' "$b"
      falta=1
    else
      printf '%-42s %s\n' "$b" "${sha:0:9}"
    fi
  done
  return "$falta"
}

limpar() {
  echo "A confirmar o remoto antes de apagar seja o que for…"
  if ! confirmar; then
    echo
    echo "RECUSADO: há ramos que não chegaram ao remoto (ver AUSENTE acima)." >&2
    echo "Corre '$0 enviar' primeiro. Nada foi apagado." >&2
    exit 1
  fi

  echo
  echo "Todos no remoto. A limpar."
  for w in "${WORKTREES[@]}"; do
    [ -d "$w" ] && git worktree remove "$w" && echo "worktree removido: $w"
  done
  git branch -D "${FUNDIDOS[@]}" 2>/dev/null || true
  git branch -D "${VIVOS[@]}"
  echo
  echo "Feito. Sobram:"
  git branch
}

case "${1:-}" in
  enviar)    enviar ;;
  confirmar) confirmar ;;
  limpar)    limpar ;;
  *) echo "uso: $0 {enviar|confirmar|limpar}" >&2; exit 2 ;;
esac
