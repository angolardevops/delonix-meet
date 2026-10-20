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

# Devolve 0 só se, para TODOS os ramos (e a develop), o SHA do remoto for
# IGUAL ao local. Existir não chega: a develop já existe no remoto e estava 25
# commits atrás — dá-la por confirmada só porque tem SHA era falhar ABERTO.
confirmar() {
  local falta=0
  for b in "${VIVOS[@]}" develop; do
    local local_sha remoto_sha
    local_sha=$(git rev-parse "$b")
    remoto_sha=$(git ls-remote --heads "$REMOTO" "$b" | cut -f1)
    if [ -z "$remoto_sha" ]; then
      printf '%-42s AUSENTE\n' "$b"
      falta=1
    elif [ "$remoto_sha" != "$local_sha" ]; then
      printf '%-42s DESACERTADO (remoto %s, local %s)\n' \
        "$b" "${remoto_sha:0:9}" "${local_sha:0:9}"
      falta=1
    else
      printf '%-42s %s ok\n' "$b" "${local_sha:0:9}"
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

# Prepara e VERIFICA a promoção da develop à main do Meet. Não a executa: o
# hook recusa `push` e operações de PR, e promover 764 commits é decisão de
# release. Imprime o que a promoção carrega e o comando que a abre.
#
# As duas referências são sobreponíveis para se poder ver o modo a RECUSAR
# contra referências forjadas, em vez de se alterar o script para o testar:
#   PROMOVER_DEV_REF=<ref> PROMOVER_MAIN_REF=<ref> ... promover
promover() {
  local dev_ref="${PROMOVER_DEV_REF:-$REMOTO/develop}"
  local main_ref="${PROMOVER_MAIN_REF:-$REMOTO/main}"

  # 1. Nada por enviar: a develop local tem de ser IGUAL à remota, senão a
  #    promoção deixava commits para trás sem ninguém dar por isso.
  local sha_local sha_remoto
  sha_local=$(git rev-parse develop)
  sha_remoto=$(git rev-parse "$dev_ref")
  if [ "$sha_local" != "$sha_remoto" ]; then
    echo "RECUSADO: a develop local e a remota divergem." >&2
    echo "  local  ${sha_local:0:9}" >&2
    echo "  remoto ${sha_remoto:0:9}  ($dev_ref)" >&2
    echo "Corre '$0 enviar' antes de promover." >&2
    return 1
  fi

  # 2. A main não pode ter trabalho que a develop não tenha — promover
  #    perdê-lo-ia. Merges não contam: são bolhas das promoções anteriores.
  local exclusivos
  exclusivos=$(git log --no-merges --oneline "$dev_ref..$main_ref")
  if [ -n "$exclusivos" ]; then
    echo "RECUSADO: a main tem trabalho que a develop não tem." >&2
    echo "$exclusivos" | sed 's/^/  /' >&2
    echo "Traz estes commits à develop antes de promover." >&2
    return 1
  fi

  # 3. Seguro. Dizer o que carrega, e parar.
  local n range topo
  n=$(git rev-list --count "$main_ref..$dev_ref")
  range=$(git ls-tree -r --name-only "$dev_ref" -- server/migrations \
          | sed 's@.*/@@' | sort | sed -n '1p;$p' | cut -d_ -f1 | paste -sd-)
  topo=$(git log -1 --format='%h %s' "$main_ref")

  echo "A promoção está SEGURA. Carrega:"
  echo "  commits          $n"
  echo "  migrações        $range"
  echo "  main actual      $topo"
  echo "  main passa a     $(git log -1 --format='%h %s' "$dev_ref")"
  echo
  echo "Neste repositório promove-se por PR (ver #176 e #200). Abre-a com:"
  echo
  echo "  gh pr create --repo angolardevops/delonix-meet \\"
  echo "    --base main --head develop \\"
  echo "    --title 'Promover a develop para a main ($(git log -1 --format=%cs "$dev_ref"))' \\"
  echo "    --body 'Consolidação de 2026-10-20: ver docs/sprint-consolidado-2026-10-20.md'"
  echo
  echo "(este modo não abre a PR nem empurra nada — é de propósito)"
}

case "${1:-}" in
  enviar)    enviar ;;
  confirmar) confirmar ;;
  limpar)    limpar ;;
  promover)  promover ;;
  *) echo "uso: $0 {enviar|confirmar|limpar|promover}" >&2; exit 2 ;;
esac
