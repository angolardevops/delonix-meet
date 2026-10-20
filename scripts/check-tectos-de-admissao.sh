#!/usr/bin/env bash
# ============================================================
#  Fitness function: NENHUMA forma de implantar fica sem tecto de participantes.
#
#  Porquê: `NODE_PEER_CAPACITY` e `ORG_MAX_PARTICIPANTS` eram lidos por `opt()`
#  e a AUSÊNCIA valia `None` — que em `signaling::org_participant_limit` é SEM
#  TECTO. O `compose.yaml` não declarava nenhum dos dois (medido a 2026-10-18):
#  aceitava a 41.ª pessoa e degradava a sala para todos, sem sinal. O máximo
#  PROVADO em bancada são 40 (docs/ops/teste-de-carga-2026-10-05.md:29).
#
#  O código passou a ter omissão, mas uma omissão no código não é desculpa para
#  uma implantação calada: quem opera tem de ver o tecto no ficheiro que edita.
#  São três formas (compose, kustomize, Helm) e as três têm de o dizer.
#
#  Verifica:
#   1. `compose.yaml` declara as DUAS variáveis no ambiente do servidor;
#   2. `deploy/k8s/01-config.yaml` declara as DUAS;
#   3. o chart EMITE as duas em todos os perfis — e emite-as também com `0`,
#      que é «sem tecto, de propósito» e o que uma rampa de carga põe
#      (values.yaml: «SE ESTÁS A MEDIR O TECTO, PÕE 0»). Até 2026-10-18 o
#      template omitia a variável quando o valor era `0`, e então o servidor
#      caía na omissão: o operador julgava ter desligado o tecto e não tinha.
#
#  Controlo positivo: o portão afirma que VÊ uma variável que sabe estar em
#  cada ficheiro. Um grep que não encontra nada é indistinguível de um grep
#  apontado ao sítio errado.
#
#  Uso:  bash scripts/check-tectos-de-admissao.sh   (HELM=… escolhe o binário)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

TECTOS=(NODE_PEER_CAPACITY ORG_MAX_PARTICIPANTS)
erros=0
falha() { echo "✗ $*"; erros=1; }

# ---- controlo positivo: o portão está a olhar para ficheiros que existem e
# que contêm o que ele julga conterem. Sem isto, um caminho errado dá verde.
for par in "compose.yaml:BIND_ADDR" "deploy/k8s/01-config.yaml:RECORDINGS_DIR"; do
  f=${par%%:*}; sentinela=${par##*:}
  [ -f "$f" ] || { falha "$f não existe — o portão não está a olhar para onde devia"; continue; }
  grep -q "$sentinela" "$f" ||
    falha "$f não contém $sentinela: o portão está a olhar para o ficheiro errado (controlo positivo)"
done

# ---- 1 e 2: as duas formas declarativas dizem os dois tectos.
for f in compose.yaml deploy/k8s/01-config.yaml; do
  [ -f "$f" ] || continue
  for v in "${TECTOS[@]}"; do
    grep -qE "^[[:space:]]*$v:" "$f" ||
      falha "$f não declara $v. Ausente, o servidor usa a omissão do código e quem opera este ficheiro não vê tecto nenhum escrito (ADR-0017)"
  done
done

# ---- 3: o chart emite os dois, em todos os perfis, e também com `0`.
CHART=deploy/helm/delonix-meet
HELM=${HELM:-helm}
if ! command -v "$HELM" >/dev/null 2>&1; then
  echo "  ! chart: NÃO CORREU — esta máquina não tem o helm (HELM=$HELM)"
else
  render() { "$HELM" template t "$CHART" "$@" 2>/dev/null; }
  PROD=(-f "$CHART/values-production.yaml" -f "$CHART/ci/production-inputs.yaml")
  LOCAL=(-f "$CHART/values-local.yaml")

  verifica_perfil() { # nome, args…
    local nome=$1; shift
    local saida; saida=$(render "$@")
    [ -n "$saida" ] || { falha "chart ($nome): não renderizou — sem render não há prova"; return; }
    for v in "${TECTOS[@]}"; do
      grep -qE "^[[:space:]]*$v:" <<<"$saida" ||
        falha "chart ($nome) não emite $v no ConfigMap"
    done
  }
  verifica_perfil produção "${PROD[@]}"
  verifica_perfil laboratório "${LOCAL[@]}"

  # O caso que o template já partiu uma vez: com `0`, a variável TEM de sair na
  # mesma. Omiti-la devolve o servidor à omissão — o oposto do que `0` pede.
  rampa=$(render "${LOCAL[@]}" --set server.nodePeerCapacity=0 --set server.orgMaxParticipants=0)
  for v in "${TECTOS[@]}"; do
    grep -qE "^[[:space:]]*$v:[[:space:]]*\"0\"" <<<"$rampa" ||
      falha "chart: com $v=0 a variável não é emitida como \"0\". O servidor cai na omissão e a rampa de carga mede contra um tecto que o operador julga ter desligado (values.yaml: «SE ESTÁS A MEDIR O TECTO, PÕE 0»)"
  done
fi

[ "$erros" -eq 0 ] || exit 1
printf "  ✓ as três formas de implantar declaram os dois tectos de admissão (e o chart emite \`0\` tal e qual)\n"
