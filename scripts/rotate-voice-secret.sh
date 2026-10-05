#!/usr/bin/env bash
# ============================================================
#  make voice-secret-rotate — troca o VOICE_INTERNAL_SECRET do .env.
#
#  É o segredo que autentica o FreeSWITCH perante o servidor (IVR, directório
#  dos ramais, CDR). O .env é a ÚNICA origem: o compose.yaml lê-o directamente
#  e o `make cluster` cria a partir dele o Secret `delonix-voice`. Trocar aqui e
#  voltar a subir é a rotação inteira — o servidor e o FreeSWITCH recebem o
#  valor novo no mesmo passo.
#
#  Quando rodar: sempre que o valor possa ter sido lido por quem não devia.
#  Em particular, qualquer instalação que tenha corrido com a configuração de
#  antes da R227 escreveu-o no log do FreeSWITCH — roda-o, e apaga esses logs.
#
#  Com um 2.º argumento troca outro segredo do mesmo género — `make esl-secret-rotate`
#  passa TELEPHONY_ESL_PASSWORD (a password do Event Socket, que dá `originate`).
#
#  O valor novo NÃO é mostrado. Uso:  bash scripts/rotate-voice-secret.sh [.env [NOME]]
# ============================================================
set -euo pipefail
cd "$(dirname "$0")/.."
ENVF=${1:-.env}
NOME=${2:-VOICE_INTERNAL_SECRET}
case $NOME in VOICE_INTERNAL_SECRET|TELEPHONY_ESL_PASSWORD) ;; *) echo "✗ só se roda VOICE_INTERNAL_SECRET ou TELEPHONY_ESL_PASSWORD"; exit 1;; esac

[ -f "$ENVF" ] || { echo "✗ não existe $ENVF — corre «make bootstrap»"; exit 1; }
command -v openssl >/dev/null 2>&1 || { echo "✗ precisa de openssl"; exit 1; }

novo=$(openssl rand -hex 32)
if grep -qE "^${NOME}=" "$ENVF"; then
  sed -i "s|^${NOME}=.*|${NOME}=${novo}|" "$ENVF"
else
  printf '%s=%s\n' "$NOME" "$novo" >>"$ENVF"
fi
chmod 600 "$ENVF"
[ "$(grep -cE "^${NOME}=[0-9a-f]{64}$" "$ENVF")" = 1 ] ||
  { echo "✗ o $ENVF não ficou com um ${NOME} válido"; exit 1; }

cat <<EOF
  ✓ $NOME trocado em $ENVF (64 hex; o valor não é mostrado)

  Falta pô-lo a valer — o servidor e o FreeSWITCH têm de reiniciar com ele:
    compose:  make compose-down && make compose-up   (o «up» sozinho não recria os que já existem)
    cluster:  make cluster                 (reaplica o Secret delonix-voice e reinicia a voz e o servidor)
    Helm:     o Secret é teu (secrets.existingSecret): troca-lhe a chave
              VOICE_INTERNAL_SECRET e reinicia o servidor e o FreeSWITCH.
  Depois:    make compose-voice-check       (o FreeSWITCH volta a falar com o servidor)
  E apaga os logs antigos do FreeSWITCH que possam ter o valor anterior.
EOF
