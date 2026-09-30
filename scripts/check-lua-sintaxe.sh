#!/usr/bin/env bash
# ============================================================
#  Fitness function: sintaxe dos scripts Lua do FreeSWITCH (R223).
#
#  O `dialin_ivr.lua` e o `ramais_dial.lua` estão no caminho de quem liga,
#  e até aqui nenhum portão os lia: não havia Lua na máquina de
#  desenvolvimento, na imagem, nem no CI. Um erro de sintaxe só aparecia
#  a quem ligava — como silêncio, porque o FreeSWITCH regista o erro e
#  desliga.
#
#  Compila (`luac -p`, sem executar) todo o *.lua debaixo de voice/, com o
#  Lua 5.2 — a versão que o mod_lua da imagem liga (liblua5.2).
#
#  Onde vai buscar o luac, por ordem:
#   1. `luac5.2` no PATH (o CI instala o pacote lua5.2);
#   2. a imagem do FreeSWITCH, se existir localmente ($FS_IMAGE).
#  `LUAC=image` salta o 1 (a prova de fumo da imagem usa-o).
#  Sem nenhum dos dois FALHA — um portão que salta em silêncio dá luz
#  verde sem ter provado nada.
#
#  Uso:  bash scripts/check-lua-sintaxe.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
FS_IMAGE=${FS_IMAGE:-delonix-meet/freeswitch:1.11.3}

mapfile -t files < <(find voice -name '*.lua' -type f | sort)
if [ "${#files[@]}" -eq 0 ]; then
  echo "✗ R223: nenhum *.lua debaixo de voice/ — o portão não está a olhar para onde devia"; exit 1
fi

if [ "${LUAC:-}" != image ] && command -v luac5.2 >/dev/null 2>&1; then
  run() { luac5.2 -p "$@"; }
  via="luac5.2 do sistema"
elif command -v docker >/dev/null 2>&1 && docker image inspect "$FS_IMAGE" >/dev/null 2>&1; then
  run() { docker run --rm --network none -v "$PWD/voice:/voice:ro" --entrypoint luac5.2 "$FS_IMAGE" -p "${@/#voice\//\/voice\/}"; }
  via="luac5.2 da imagem $FS_IMAGE"
else
  echo "✗ R223: não há luac5.2 — instala o pacote lua5.2, ou constrói a imagem (make freeswitch-image)"; exit 1
fi

if out=$(run "${files[@]}" 2>&1); then
  printf "  ✓ sintaxe Lua do FreeSWITCH (%d ficheiros, %s)\n" "${#files[@]}" "$via"
else
  echo "✗ R223: script Lua do FreeSWITCH não compila ($via):"
  echo "$out" | sed 's/^/     /'
  exit 1
fi
