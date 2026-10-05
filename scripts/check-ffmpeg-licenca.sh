#!/usr/bin/env bash
# ============================================================
#  Fitness function: o ffmpeg da imagem do SERVIDOR é LGPL (decisão D3, achado B3; docs/tv/b3-ffmpeg-lgpl-2026-10-04.md).
#
#  O servidor só precisa de `-c copy`, `aac` nativo, libvpx-vp9 e libopus — nada
#  que exija libx264. Por isso a imagem leva uma build LGPL e a GPL fica
#  confinada à imagem do Channel Engine (D3). Este portão impede que uma edição
#  descuidada do Dockerfile a faça GPL ou não redistribuível sem ninguém decidir:
#
#   1. O `Dockerfile.server` tem de pedir `--disable-gpl` e `--disable-nonfree`,
#      fixar a versão e o SHA-256 do código-fonte, e apontar `FFMPEG_BIN`.
#   2. Não pode (fora de comentários) activar `--enable-gpl`, `--enable-nonfree`,
#      `--enable-version3`, nem pedir libx264/libx265/libfdk-aac.
#   3. Se `FFMPEG_BIN` apontar a um binário que exista (um ffmpeg de uma imagem
#      já construída), `ffmpeg -L` tem de dizer «Lesser» e a linha de
#      configuração não pode ter `--enable-gpl|nonfree|version3`.
#
#  É estático: não prova que a imagem compila nem que o ffmpeg corre dentro
#  dela — isso é construir a imagem e correr `ffmpeg -version` nela. A parte 3
#  só actua quando há um binário; o CI não constrói esta imagem.
#
#  Uso:  bash scripts/check-ffmpeg-licenca.sh
#        FFMPEG_BIN=/opt/ffmpeg/bin/ffmpeg bash scripts/check-ffmpeg-licenca.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

F=Dockerfile.server
fail=0
bad() { echo "✗ ffmpeg-licença: $1"; fail=1; }
# Só as linhas que NÃO são comentário: um comentário pode explicar porque não
# se activa uma opção sem a activar.
# Lidas UMA vez para uma variável e dadas ao `grep -q` por `<<<`, nunca por um
# pipe (R298): com `pipefail`, um `grep -q` que sai à primeira
# correspondência mata o produtor com SIGPIPE e o pipeline dá erro — no ciclo
# dos proibidos isso lia-se como «não usa», que é o portão a dar verde.
code=$(grep -vE '^[[:space:]]*#' "$F")

for need in '--disable-gpl' '--disable-nonfree' 'FFMPEG_SHA256' 'FFMPEG_VERSION' 'FFMPEG_BIN'; do
  grep -q -- "$need" <<<"$code" || bad "$F não tem «$need»"
done
for forbid in '--enable-gpl' '--enable-nonfree' '--enable-version3' 'libx264' 'libx265' 'libfdk' 'enable-libx2'; do
  grep -q -- "$forbid" <<<"$code" && bad "$F usa «$forbid» — a imagem do servidor é LGPL (docs/tv/b3-ffmpeg-lgpl-2026-10-04.md)"
done
# O SHA-256 tem de estar preenchido (64 hex), não um marcador de zeros.
sha=$(grep -E 'ARG FFMPEG_SHA256=' <<<"$code")
grep -qE '=[0-9a-f]{64}$' <<<"$sha" \
  || bad "FFMPEG_SHA256 não está fixado a 64 dígitos hexadecimais"
grep -qE '=0{64}$' <<<"$sha" \
  && bad "FFMPEG_SHA256 é o marcador de zeros — falta fixar o SHA-256 depois de verificar a assinatura"

if [ -n "${FFMPEG_BIN:-}" ] && [ -x "$FFMPEG_BIN" ]; then
  licenca=$("$FFMPEG_BIN" -L 2>/dev/null | head -3)
  grep -q 'Lesser' <<<"$licenca" \
    || bad "$FFMPEG_BIN não declara a licença LGPL (ffmpeg -L)"
  "$FFMPEG_BIN" -version 2>/dev/null | grep -E 'enable-(gpl|nonfree|version3)' \
    && bad "$FFMPEG_BIN foi configurado com GPL, nonfree ou version3"
fi

if [ "$fail" -eq 0 ]; then
  echo "✓ ffmpeg-licença: a imagem do servidor pede uma build LGPL, com a versão e o SHA-256 fixados"
  exit 0
fi
exit 1
