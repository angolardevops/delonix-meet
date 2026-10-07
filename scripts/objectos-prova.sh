#!/usr/bin/env bash
# ============================================================
#  Prova do cliente de OBJECTOS contra um MinIO a sério (ADR-0020, D2).
#
#  PORQUE EXISTE. O `server/src/objectos.rs` entrou no repo com três testes de
#  unidade — a chave, o ambiente incompleto e o `Debug` sem a chave secreta — e
#  **nenhum deles fala com um armazenamento**. O que o `aws-sdk-s3` faz com um
#  endpoint de MinIO, com `force_path_style`, com um `Range` e com um bucket que
#  não existe não se adivinha do tipo: mede-se. Sem isto, «o cliente está feito»
#  era uma afirmação sobre código que nunca tocou num servidor.
#
#  Ergue um MinIO em contentor (rede e endereços próprios, API publicada só em
#  loopback), cria o bucket com o `mc`, corre o teste de integração que só
#  existe quando há endpoint, e desmonta.
#
#  Corre no delonix (daemonless, sem root) se existir, senão no docker;
#  MOTOR=docker|delonix escolhe-o. As diferenças estão em scripts/motor.sh.
#
#  Uso:
#    bash scripts/objectos-prova.sh             ergue, mede, desmonta
#    bash scripts/objectos-prova.sh up|mede|down    um passo de cada vez
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=scripts/motor.sh
. scripts/motor.sh

REDE=meet-objectos-prova
SUBREDE=10.231.7.0/24
MINIO=meet-objectos-minio
MC=meet-objectos-mc
# O delonix só publica portas de contentores com endereço em 10.200–254.x, e o
# anfitrião não chega aos endereços dos contentores: a API vai para loopback.
PORTA=${PORTA:-9123}
BUCKET=${BUCKET:-meet-gravacoes-prova}
ACESSO=provadelonix
SEGREDO=prova-delonix-1234
IMG_MINIO=${IMG_MINIO:-minio/minio:latest}
IMG_MC=${IMG_MC:-minio/mc:latest}

r=$'\e[31m'; g=$'\e[32m'; y=$'\e[33m'; z=$'\e[0m'

up() {
  down >/dev/null 2>&1
  m_rede_cria "$REDE" "$SUBREDE" || { echo "${r}✗ não consegui criar a rede $REDE${z}"; exit 1; }

  m_run -d --name "$MINIO" "$M_NET" "$REDE" \
    -p "127.0.0.1:$PORTA:9000" \
    -e "MINIO_ROOT_USER=$ACESSO" \
    -e "MINIO_ROOT_PASSWORD=$SEGREDO" \
    "$IMG_MINIO" "${M_SEP[@]}" server /data --address ':9000' >/dev/null \
    || { echo "${r}✗ o MinIO não arrancou${z}"; exit 1; }

  # Espera pelo `/minio/health/live`, que é o que o MinIO diz quando está
  # pronto a servir — e não pela porta aberta, que abre antes disso.
  for i in $(seq 1 60); do
    if curl -fsS -m 2 "http://127.0.0.1:$PORTA/minio/health/live" >/dev/null 2>&1; then
      echo "  ${g}✓ MinIO de pé em 127.0.0.1:$PORTA (ao fim de ${i}s)${z}"
      break
    fi
    [ "$i" = 60 ] && { echo "${r}✗ o MinIO não respondeu ao health em 60s${z}"; m_logs "$MINIO" 2>&1 | tail -20; exit 1; }
    sleep 1
  done

  # O bucket cria-se com o `mc`, DE DENTRO da rede: o cliente não o cria, de
  # propósito — quem cria buckets é quem instala, não o servidor de reuniões.
  m_run --rm --name "$MC" "$M_NET" "$REDE" --entrypoint /bin/sh "$IMG_MC" "${M_SEP[@]}" -c \
    "mc alias set p http://$MINIO:9000 $ACESSO $SEGREDO >/dev/null && mc mb --ignore-existing p/$BUCKET" \
    || { echo "${r}✗ não consegui criar o bucket $BUCKET${z}"; exit 1; }
  echo "  ${g}✓ bucket «$BUCKET» criado${z}"
}

mede() {
  echo
  echo "— o cliente contra o MinIO —"
  OBJECT_STORE_ENDPOINT="http://127.0.0.1:$PORTA" \
  OBJECT_STORE_BUCKET="$BUCKET" \
  OBJECT_STORE_ACCESS_KEY="$ACESSO" \
  OBJECT_STORE_SECRET_KEY="$SEGREDO" \
  cargo test --manifest-path server/Cargo.toml --test objectos_minio -- --nocapture --test-threads=1
  local rc=$?
  [ "$rc" = 0 ] || { echo "${r}✗ a prova falhou${z}"; return 1; }

  # Controlo negativo do próprio arnês: SEM endpoint, o teste tem de se
  # declarar ignorado em vez de passar em silêncio — um teste de integração que
  # passa sem o serviço de pé não prova nada.
  echo
  echo "— controlo: sem endpoint, o teste não finge que mediu —"
  local saida
  saida=$(env -u OBJECT_STORE_ENDPOINT -u OBJECT_STORE_ACCESS_KEY -u OBJECT_STORE_SECRET_KEY \
    cargo test --manifest-path server/Cargo.toml --test objectos_minio -- --nocapture 2>&1)
  if grep -q "sem OBJECT_STORE" <<<"$saida"; then
    echo "  ${g}✓ sem ambiente, o teste diz que não mediu${z}"
  else
    echo "${r}✗ sem ambiente o teste não avisou — passaria por prova${z}"
    printf '%s\n' "$saida" | tail -20
    return 1
  fi
}

down() {
  m_rm "$MINIO" "$MC" >/dev/null 2>&1
  m_rede_apaga "$REDE"
  echo "  ${y}· desmontado${z}"
}

case "${1:-tudo}" in
  up)   up ;;
  mede) mede ;;
  down) down ;;
  tudo)
    up || exit 1
    mede; rc=$?
    down
    [ "$rc" = 0 ] && echo "${g}✓ prova dos objectos: o cliente escreve, lê por intervalos, mede e apaga num MinIO a sério${z}"
    exit "$rc"
    ;;
  *) echo "uso: $0 [up|mede|down|tudo]"; exit 2 ;;
esac
