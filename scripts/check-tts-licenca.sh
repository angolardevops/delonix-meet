#!/usr/bin/env bash
# ============================================================
#  Fitness function: a GPL do Piper fica CONFINADA à imagem do tts-server.
#
#  O molde é o da decisão D3 (ffmpeg): o servidor do Meet é LGPL e o que é
#  copyleft vive numa imagem à parte, com a sua própria oferta de código. O
#  Piper é GPL-3.0-or-later desde a v1.3.0 — embute o espeak-ng (GPLv3) —, e
#  isso não é um defeito: é uma obrigação de EMPACOTAMENTO que esta casa já
#  sabe cumprir. Este portão impede que uma edição descuidada a quebre:
#
#   1. a GPL não sai da sua imagem: NENHUM outro Dockerfile instala piper;
#   2. a oferta de código existe NO ARTEFACTO: o Dockerfile traz o sdist e
#      verifica-o por SHA-256 (§6 da GPL-3.0 cumprida por acompanhamento, que
#      não depende de mantermos um servidor de downloads vivo três anos);
#   3. a voz é fixada por REVISÃO e por SHA-256, e a sua licença está escrita;
#   4. o catálogo de línguas do serviço e o do DOMÍNIO não divergem — o Rust
#      decide o que PODE ser pedido, o Python o que SABE dizer, e os dois
#      discordarem é um defeito que só aparece em produção.
#
#  É estático: não constrói a imagem nem corre o Piper. Prova o que está
#  escrito, que é o que uma edição distraída muda.
#
#  Uso:  bash scripts/check-tts-licenca.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

D=tts-server/Dockerfile
L=tts-server/LICENCAS.md
A=tts-server/app.py
R=server/crates/delonix-meet-domain/src/conferencing/dubbing.rs
fail=0
bad() { echo "✗ tts-licença: $1"; fail=1; }

for f in "$D" "$L" "$A" "$R"; do
  [ -f "$f" ] || { bad "falta $f"; echo "exit 1"; exit 1; }
done

# ---- 1. a GPL não sai da sua imagem ---------------------------------------
outros=$(git ls-files -z -- '*Dockerfile*' ':!tts-server/*' \
  | xargs -0 grep -ln 'piper[-_]tts\|piper1-gpl' 2>/dev/null || true)
if [ -n "$outros" ]; then
  bad "o Piper (GPL-3.0) aparece noutra imagem — a GPL tem de ficar confinada ao tts-server:"
  echo "$outros" | sed 's/^/     /'
fi

# ---- 2. a oferta de código vem no artefacto -------------------------------
grep -q 'PIPER_SDIST_SHA256' "$D" || bad "o Dockerfile não fixa o SHA-256 do código-fonte do Piper"
grep -q 'sha256sum -c' "$D" || bad "o Dockerfile não VERIFICA nenhum SHA-256 (fixar sem verificar não fixa nada)"
grep -q '/usr/share/piper-src' "$D" || bad "o código-fonte do Piper não fica DENTRO da imagem — a oferta da GPL §6 não se cumpre"
grep -qi 'GPL-3.0' "$L" || bad "o LICENCAS.md não diz que a imagem é GPL-3.0"

# O SHA do Dockerfile e o do LICENCAS.md têm de ser o MESMO: dois números que
# divergem são pior do que um só, porque dão a sensação de estarem conferidos.
sd=$(grep -oE 'PIPER_SDIST_SHA256=[0-9a-f]{64}' "$D" | head -1 | cut -d= -f2)
[ -n "$sd" ] || bad "não consegui ler o SHA-256 do sdist no Dockerfile"
if [ -n "$sd" ] && ! grep -q "$sd" "$L"; then
  bad "o SHA-256 do código-fonte no Dockerfile não está registado no LICENCAS.md"
fi

# ---- 3. a voz é fixada, e a licença dela escrita ---------------------------
grep -qE 'VOICES_REV=[0-9a-f]{40}' "$D" || bad "a voz não está presa a uma REVISÃO do catálogo"
vs=$(grep -oE 'VOICE_SHA256=[0-9a-f]{64}' "$D" | head -1 | cut -d= -f2)
[ -n "$vs" ] || bad "a voz não está fixada por SHA-256"
if [ -n "$vs" ] && ! grep -q "$vs" "$L"; then
  bad "o SHA-256 da voz não está registado no LICENCAS.md"
fi
grep -q 'MODEL_CARD' "$D" || bad "o MODEL_CARD da voz não é trazido para a imagem — a licença dela ficaria por provar"

# ---- 4. os dois catálogos não divergem ------------------------------------
# O Rust lista as línguas que PODEM ser pedidas; o Python, as que sabe dizer.
rust=$(grep -oE 'lingua: "[a-zA-Z-]+"' "$R" | cut -d'"' -f2 | sort -u)
py=$(sed -n '/^VOZES = {/,/^}/p' "$A" | grep -oE '"[a-zA-Z-]+":' | tr -d '":' | sort -u)
if [ "$rust" != "$py" ]; then
  bad "o catálogo do domínio e o do serviço divergem — isto só aparecia em produção:"
  echo "     domínio ($R): $(echo "$rust" | tr '\n' ' ')"
  echo "     serviço ($A): $(echo "$py" | tr '\n' ' ')"
fi

# E a decisão escrita: não se oferece pt-AO, nem se disfarça com pt-BR.
for proibida in pt-AO pt-BR; do
  if grep -qx "$proibida" <<<"$py"; then
    bad "o serviço oferece «$proibida» — o ADR-0030 diz que não há voz angolana e que não se finge uma (ver LICENCAS.md)"
  fi
done

[ "$fail" = 0 ] && echo "✓ tts-licença: a GPL fica na sua imagem; fonte acompanhada e verificada; voz presa por revisão+SHA-256 com licença escrita; catálogo do domínio e do serviço iguais ($(echo "$py" | tr '\n' ' '))"
exit $fail
