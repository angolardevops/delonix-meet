#!/usr/bin/env bash
# ============================================================
#  Delonix Meet — Secret `delonix-secrets` do Kubernetes, FORA do repositório
#  (R155). É o que o `make secrets-k8s` corre, e o `make stage`/`make prod`
#  chamam-no antes do helm e dos manifestos.
#
#  Os valores estiveram escritos em deploy/k8s/01-config.yaml num repositório
#  PÚBLICO e estão queimados (scripts/leaked-secrets-accepted.txt). Nenhum
#  ficheiro versionado volta a ter valores: nascem aqui, aleatórios, no cluster.
#
#  Idempotente e conservador:
#   - chave que FALTA → gera-se (`openssl rand`);
#   - chave que EXISTE → NUNCA se sobrescreve (trocar o JWT desliga toda a gente,
#     trocar a password do Postgres parte a ligação à base);
#   - chave que existe com um valor QUEIMADO → falha com os comandos de rotação.
#     O servidor recusaria arrancar de qualquer forma; falhar aqui diz porquê.
#
#  Coerência Postgres: o DATABASE_URL é construído com o POSTGRES_PASSWORD do
#  próprio Secret, e o `make stage`/`make prod` passam ESSE valor ao helm.
#
#  Rodar (só o que não precisa de mexer na base):
#     ROTATE="JWT_SECRET TURN_SECRET PROVISIONING_SECRET" deploy/k8s-secrets.sh
#  A password do Postgres roda-se à mão (ALTER ROLE) — docs/deployment.md §6.
#
#  Variáveis: NS (delonix-meet), SECRET (delonix-secrets),
#             PG_HOST (delonix-postgres-postgresql; nome curto = Service em $NS,
#             com ponto = host completo), ROTATE, KUBECTL (kubectl).
# ============================================================
set -euo pipefail

NS="${NS:-delonix-meet}"
SECRET="${SECRET:-delonix-secrets}"
PG_HOST="${PG_HOST:-delonix-postgres-postgresql}"
ROTATE="${ROTATE:-}"
KUBECTL="${KUBECTL:-kubectl}"
LEDGER="$(cd "$(dirname "$0")/.." && pwd)/scripts/leaked-secrets-accepted.txt"

say() { printf '   %s\n' "$*"; }
die() { printf '\033[1;31m   ✗ %s\033[0m\n' "$*" >&2; exit 1; }

command -v openssl >/dev/null || die "falta o openssl"
[ -f "$LEDGER" ] || die "falta $LEDGER"

for k in $ROTATE; do
  case "$k" in
    JWT_SECRET|TURN_SECRET|PROVISIONING_SECRET) ;;
    *) die "ROTATE=$k não é suportado aqui: a password do Postgres roda-se com ALTER ROLE (docs/deployment.md §6)";;
  esac
done

# Valores actuais (vazio = chave ausente). Nunca impressos.
declare -A cur=()
if "$KUBECTL" -n "$NS" get secret "$SECRET" >/dev/null 2>&1; then
  exists=1
  for k in PROVISIONING_SECRET JWT_SECRET TURN_SECRET POSTGRES_PASSWORD POSTGRES_USER POSTGRES_DB DATABASE_URL; do
    raw=$("$KUBECTL" -n "$NS" get secret "$SECRET" -o "jsonpath={.data.$k}" 2>/dev/null || true)
    cur[$k]=""
    if [ -n "$raw" ]; then cur[$k]=$(printf '%s' "$raw" | base64 -d); fi
  done
else
  exists=0
fi

# Valores queimados em uso? (comparação exacta, linha a linha do livro)
burned_in_use=""
while IFS= read -r v; do
  case "$v" in ''|'#'*) continue;; esac
  for k in PROVISIONING_SECRET JWT_SECRET TURN_SECRET POSTGRES_PASSWORD; do
    if [ "${cur[$k]:-}" = "$v" ] && [[ " $ROTATE " != *" $k "* ]]; then
      burned_in_use="$burned_in_use $k"
    fi
  done
  case "${cur[DATABASE_URL]:-}" in *":$v@"*) burned_in_use="$burned_in_use DATABASE_URL";; esac
done < "$LEDGER"
report_burned() {
  printf '\033[1;31m   ✗ %s/%s tem valores PUBLICADOS no repositório:%s\033[0m\n' "$NS" "$SECRET" "$burned_in_use" >&2
  say "O servidor recusa-os. Rodar ANTES de continuar (docs/deployment.md §6):" >&2
  say "  ROTATE=\"JWT_SECRET TURN_SECRET PROVISIONING_SECRET\" make secrets-k8s   # só os que aparecem acima" >&2
  say "  POSTGRES_PASSWORD / DATABASE_URL: ALTER ROLE + patch do Secret (§6)" >&2
}
# Sem ROTATE, um valor queimado pára tudo. Com ROTATE, roda-se o que foi pedido
# e só no fim se falha pelo que sobrar — senão rodar o JWT dependia de a
# password do Postgres (que precisa de ALTER ROLE) já estar resolvida.
if [ -n "$burned_in_use" ] && [ -z "$ROTATE" ]; then
  report_burned
  exit 1
fi

declare -A new=()
# DATABASE_URL sem POSTGRES_PASSWORD (Secret feito à mão): a password vem do URL,
# nunca uma nova — uma nova partia a coerência com a base que já existe.
if [ -z "${cur[POSTGRES_PASSWORD]:-}" ] && [ -n "${cur[DATABASE_URL]:-}" ]; then
  from_url=$(printf '%s' "${cur[DATABASE_URL]}" | sed -n 's#^[a-z]*://[^:/@]*:\([^@]*\)@.*#\1#p')
  if [ -n "$from_url" ]; then new[POSTGRES_PASSWORD]="$from_url"; fi
fi
gen_if_missing() { # chave, comando gerador
  local k=$1; shift
  if [ -z "${cur[$k]:-}" ] || [[ " $ROTATE " == *" $k "* ]]; then new[$k]=$("$@"); fi
}
gen_if_missing JWT_SECRET          openssl rand -hex 48
gen_if_missing TURN_SECRET         openssl rand -hex 32
gen_if_missing PROVISIONING_SECRET openssl rand -hex 32
[ -n "${new[POSTGRES_PASSWORD]:-}" ] || gen_if_missing POSTGRES_PASSWORD openssl rand -hex 24  # hex: seguro num URL
[ -n "${cur[POSTGRES_USER]:-}" ] || new[POSTGRES_USER]=delonix
[ -n "${cur[POSTGRES_DB]:-}" ]   || new[POSTGRES_DB]=delonix_meet

pg_pass="${new[POSTGRES_PASSWORD]:-${cur[POSTGRES_PASSWORD]:-}}"
pg_user="${new[POSTGRES_USER]:-${cur[POSTGRES_USER]:-}}"
pg_db="${new[POSTGRES_DB]:-${cur[POSTGRES_DB]:-}}"
if [ -z "${cur[DATABASE_URL]:-}" ]; then
  case "$PG_HOST" in *.*) pg_fqdn="$PG_HOST" ;; *) pg_fqdn="$PG_HOST.$NS.svc.cluster.local" ;; esac
  new[DATABASE_URL]="postgres://${pg_user}:${pg_pass}@${pg_fqdn}:5432/${pg_db}"
else
  case "${cur[DATABASE_URL]}" in
    *":${pg_pass}@"*) ;;
    *) printf '\033[1;33m   ! DATABASE_URL não usa o POSTGRES_PASSWORD do mesmo Secret — confirma à mão\033[0m\n' >&2;;
  esac
fi

if [ "${#new[@]}" -eq 0 ]; then
  say "$SECRET já tem todas as chaves — mantido"
  exit 0
fi

# Os valores vão por ficheiro 0600 e não pela linha de comandos (ps/histórico).
tmp=$(mktemp); chmod 600 "$tmp"; trap 'rm -f "$tmp"' EXIT
if [ "$exists" = 0 ]; then
  for k in "${!new[@]}"; do printf '%s=%s\n' "$k" "${new[$k]}"; done > "$tmp"
  "$KUBECTL" -n "$NS" create secret generic "$SECRET" --from-env-file="$tmp" >/dev/null
  say "✓ $SECRET criado (aleatório): $(printf '%s ' "${!new[@]}" | xargs -n1 | sort | xargs)"
else
  # Merge patch com só as chaves novas: as existentes ficam intactas.
  body=$(for k in "${!new[@]}"; do printf '"%s":"%s",' "$k" "$(printf '%s' "${new[$k]}" | base64 | tr -d '\n')"; done)
  printf '{"data":{%s}}' "${body%,}" > "$tmp"
  "$KUBECTL" -n "$NS" patch secret "$SECRET" --type=merge --patch-file="$tmp" >/dev/null
  say "✓ $SECRET completado/rodado: $(printf '%s ' "${!new[@]}" | xargs -n1 | sort | xargs)"
fi

if [ -n "$burned_in_use" ]; then
  report_burned
  exit 1
fi
