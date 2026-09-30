#!/usr/bin/env bash
# ============================================================
#  Prova de fumo da imagem FreeSWITCH do Meet (R223).
#
#  Arranca a imagem com a configuração vanilla que ela traz, SEM REDE
#  (`--network none`: a vanilla escuta SIP com passwords por omissão e não
#  pode ver rede nenhuma), e exige:
#   1. as três fontes fixadas (/REF-*) iguais aos ARG do Containerfile;
#   2. os módulos de que os dialplans do Meet dependem a CARREGAR, não só
#      presentes em disco — mod_lua e mod_curl eram os que faltavam;
#   3. o mod_lua a executar um script que chama a API do FreeSWITCH e vê o
#      mod_curl carregado — a mesma cadeia que o dialin_ivr.lua usa;
#   4. o luac5.2 da imagem a compilar os scripts do Meet.
#
#  NÃO prova a media nem a ponte: isso é a R222 (voice/freeswitch/image/README.md).
#
#  Uso:  bash scripts/freeswitch-image-smoke.sh [imagem]
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
IMAGE=${1:-${FS_IMAGE:-delonix-meet/freeswitch:1.11.3}}
NAME=fs-smoke-$$
CF=voice/freeswitch/image/Containerfile
fail=0
bad() { echo "✗ $*"; fail=1; }
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; rm -rf "${tmp:-}"; }
trap cleanup EXIT

# 1) as fontes fixadas
for ref in FREESWITCH SOFIA_SIP SPANDSP; do
  want=$(sed -n "s/^ARG ${ref}_REF=//p" "$CF")
  file=$(echo "$ref" | tr 'A-Z_' 'a-z-')
  got=$(docker run --rm --network none --entrypoint cat "$IMAGE" "/REF-$file" 2>/dev/null)
  [ -n "$want" ] && [ "$got" = "$want" ] || bad "/REF-$file na imagem ($got) ≠ ARG ${ref}_REF ($want)"
done

# 2) arranque sem rede
# Dentro da árvore e não em /tmp: o Docker Desktop só partilha a home com a VM.
tmp=$PWD/.fs-smoke.$$; mkdir -p "$tmp"
cat > "$tmp/smoke.lua" <<'LUA'
local api = freeswitch.API()
stream:write("lua-ok curl=" .. api:execute("module_exists", "mod_curl"))
LUA
chmod 644 "$tmp/smoke.lua"
docker run -d --name "$NAME" --network none -v "$tmp/smoke.lua:/smoke.lua:ro" \
  "$IMAGE" freeswitch -nonat -nf -nc >/dev/null || { echo "✗ a imagem não arrancou"; exit 1; }
cli() { docker exec "$NAME" fs_cli -x "$1" 2>/dev/null; }
up=0
for _ in $(seq 1 60); do
  if cli status | grep -q "UP"; then up=1; break; fi
  sleep 1
done
if [ "$up" -ne 1 ]; then
  echo "✗ o FreeSWITCH não ficou UP em 60 s"; docker logs --tail 40 "$NAME"; exit 1
fi

# A vanilla não carrega o mod_curl; os dialplans do Meet precisam dele.
cli "load mod_curl" >/dev/null
for m in mod_sofia mod_event_socket mod_conference mod_dptools mod_commands mod_opus mod_lua mod_curl; do
  [ "$(cli "module_exists $m" | tr -d '[:space:]')" = "true" ] || bad "módulo não carrega: $m"
done

# 3) o mod_lua executa e vê a API
out=$(cli "lua /smoke.lua")
[ "$out" = "lua-ok curl=true" ] || bad "mod_lua não executou o script de fumo (saída: '$out')"

# 4) o luac da imagem compila os scripts do Meet
FS_IMAGE=$IMAGE LUAC=image bash scripts/check-lua-sintaxe.sh || fail=1

if [ "$fail" -eq 0 ]; then
  echo "  ✓ imagem $IMAGE: fontes fixadas, módulos do Meet carregam, mod_lua executa"
fi
exit "$fail"
