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
#  O valor novo NÃO é mostrado. Uso:  bash scripts/rotate-voice-secret.sh [.env]
# ============================================================
set -euo pipefail
cd "$(dirname "$0")/.."
ENVF=${1:-.env}

[ -f "$ENVF" ] || { echo "✗ não existe $ENVF — corre «make bootstrap»"; exit 1; }
command -v openssl >/dev/null 2>&1 || { echo "✗ precisa de openssl"; exit 1; }

novo=$(openssl rand -hex 32)
if grep -qE '^VOICE_INTERNAL_SECRET=' "$ENVF"; then
  sed -i "s|^VOICE_INTERNAL_SECRET=.*|VOICE_INTERNAL_SECRET=${novo}|" "$ENVF"
else
  printf 'VOICE_INTERNAL_SECRET=%s\n' "$novo" >>"$ENVF"
fi
chmod 600 "$ENVF"
[ "$(grep -cE '^VOICE_INTERNAL_SECRET=[0-9a-f]{64}$' "$ENVF")" = 1 ] ||
  { echo "✗ o $ENVF não ficou com um VOICE_INTERNAL_SECRET válido"; exit 1; }

cat <<EOF
  ✓ VOICE_INTERNAL_SECRET trocado em $ENVF (64 hex; o valor não é mostrado)

  Falta pô-lo a valer — o servidor e o FreeSWITCH têm de reiniciar com ele:
    compose:  make compose-up              (recria os contentores que leem o .env)
    cluster:  make cluster                 (reaplica o Secret delonix-voice e reinicia a voz e o servidor)
    Helm:     o Secret é teu (secrets.existingSecret): troca-lhe a chave
              VOICE_INTERNAL_SECRET e reinicia o servidor e o FreeSWITCH.
  Depois:    make compose-voice-check       (o FreeSWITCH volta a falar com o servidor)
  E apaga os logs antigos do FreeSWITCH que possam ter o valor anterior.
EOF
