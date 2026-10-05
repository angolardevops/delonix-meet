#!/usr/bin/env bash
# ============================================================
#  Fitness function: só o bordo diz de que organização é uma central (ADR-0016).
#
#  A central de uma organização entra autenticada no bordo, e o bordo di-lo ao
#  FreeSWITCH no cabeçalho `X-Delonix-Central`. Três coisas têm de ser verdade
#  para esse cabeçalho valer alguma coisa, e qualquer uma se perde numa edição:
#
#    1. o bordo TIRA os `X-Delonix-*` que vêm de fora, antes de decidir quem liga;
#    2. o bordo só o ESCREVE depois de verificar o digest, e só por TLS;
#    3. o IVR só ACREDITA nele vindo de um endereço do bordo (lista `delonix_bordo`).
#
#  Estático: lê voice/kamailio/kamailio.cfg e o dialin_ivr.lua. A prova com
#  chamadas — incluindo um cabeçalho forjado pelas duas portas — é
#  `bash scripts/pbx-tronco-prova.sh central` (fora do CI).
#
#  Uso:  bash scripts/check-bordo-central.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
CFG=voice/kamailio/kamailio.cfg
LUA=voice/freeswitch/scripts/dialin_ivr.lua
ARRANQUE=voice/cluster/freeswitch-entrypoint.sh
erros=0
erro() { echo "✗ ADR-0016: $*"; erros=1; }
for f in "$CFG" "$LUA" "$ARRANQUE"; do
  [ -f "$f" ] || { echo "✗ ADR-0016: falta $f — o portão não está a olhar para onde devia"; exit 1; }
done
# A primeira linha (sem contar comentários) em que aparece o padrão; 0 se não aparece.
# O número lê-se para uma variável e o `0` sai de ela estar vazia, não do estado
# do pipeline (R298): com `pipefail`, o `head -1` a sair cedo pode matar
# o `grep` anterior com SIGPIPE, e o `|| echo 0` acrescentava um `0` ao número.
linha() {
  local n
  n=$(grep -nE "$2" "$1" | grep -vE '^[0-9]+:[[:space:]]*(#|--)' | head -1 | cut -d: -f1)
  echo "${n:-0}"
}

# 1. Tira-se o que vem de fora ANTES de se olhar para quem liga.
tira=$(linha "$CFG" 'remove_hf(_re)?\("\^?X-Delonix-')
decide=$(linha "$CFG" 'allow_source_address\(')
[ "$tira" -gt 0 ] || erro "$CFG não tira os cabeçalhos X-Delonix-* que vêm de fora"
[ "$decide" -gt 0 ] || erro "$CFG já não decide por allow_source_address — revê este portão"
[ "$tira" -gt 0 ] && [ "$decide" -gt 0 ] && [ "$tira" -gt "$decide" ] &&
  erro "$CFG:$tira tira os X-Delonix-* DEPOIS de decidir quem liga ($CFG:$decide)"

# 2. Escreve-se uma vez só, depois do digest, numa rota que recusa quem não vem por TLS.
n=$(grep -E 'append_hf\("X-Delonix-Central' "$CFG" | grep -cvE '^[[:space:]]*#')
[ "$n" -eq 1 ] || erro "$CFG escreve X-Delonix-Central $n vezes (tem de ser uma, na rota CENTRAL)"
rota=$(linha "$CFG" '^route\[CENTRAL\]')
tls=$(linha "$CFG" 'proto[[:space:]]*!=[[:space:]]*TLS')
digest=$(linha "$CFG" 'pv_proxy_authenticate\(')
escreve=$(linha "$CFG" 'append_hf\("X-Delonix-Central')
if [ "$rota" -eq 0 ] || [ "$tls" -eq 0 ] || [ "$digest" -eq 0 ] || [ "$escreve" -eq 0 ]; then
  erro "$CFG: falta a rota CENTRAL, a recusa sem TLS, a verificação do digest ou o cabeçalho"
elif ! { [ "$rota" -lt "$tls" ] && [ "$tls" -lt "$digest" ] && [ "$digest" -lt "$escreve" ]; }; then
  erro "$CFG: a ordem tem de ser rota CENTRAL ($rota) → recusa sem TLS ($tls) → digest ($digest) → cabeçalho ($escreve)"
fi

# 3. O IVR só acredita no cabeçalho vindo do bordo, e a lista do bordo existe.
le=$(linha "$LUA" 'sip_h_X-Delonix-Central')
confia=$(linha "$LUA" 'acl .*delonix_bordo')
valida=$(linha "$LUA" 'validate-central')
[ "$le" -gt 0 ] && [ "$confia" -gt 0 ] && [ "$valida" -gt 0 ] ||
  erro "$LUA: falta ler o cabeçalho, perguntar à lista delonix_bordo ou validar pela central"
[ "$confia" -gt 0 ] && [ "$valida" -gt 0 ] && [ "$confia" -gt "$valida" ] &&
  erro "$LUA: a origem só é verificada ($confia) depois de o PIN ser validado pela central ($valida)"
grep -q 'list name=\\"delonix_bordo\\" default=\\"deny\\"' "$ARRANQUE" ||
  erro "$ARRANQUE não define a lista delonix_bordo a recusar por omissão"

[ "$erros" -eq 0 ] || exit 1
printf "  ✓ só o bordo diz de que organização é uma central (tira à entrada, escreve depois do digest por TLS, o IVR só acredita no bordo)\n"
