#!/usr/bin/env bash
# Toda a rota com escopo de ORGANIZAÇÃO é exercitada pelo teste de isolamento.
#
# Porquê: o `check-route-auth.sh` garante que cada rota tem extractor de
# AUTENTICAÇÃO — sabe QUEM é. Não diz nada sobre AUTORIZAÇÃO: se o handler
# confere que esse quem pertence à organização do caminho. Essa segunda metade
# é o `isolamento.mjs`, e ele cobria 13 das 19 rotas de organização.
#
# As seis que faltavam incluíam a trilha de auditoria (ler a de outra empresa),
# a configuração de SSO, e dois DELETE — apagar chave de API e webhook de outra
# organização. Nenhuma estava vulnerável; nenhuma estava provada. Ver R95.
#
# Mesmo método do inventário de testes ponta-a-ponta (R72): compara-se o que
# EXISTE com o que se TESTA, e a diferença é a lista do que não protege nada.
set -uo pipefail
cd "$(dirname "$0")/.."

MAIN=server/src/main.rs
ISO=web/e2e/isolamento.mjs
[ -f "$MAIN" ] && [ -f "$ISO" ] || { echo "✗ ficheiros em falta"; exit 1; }

falta=0

# ---- 1. rotas com escopo de ORGANIZAÇÃO ----
while read -r rota; do
  # O sufixo depois de `{org_id}` é o que identifica o recurso. Os parâmetros
  # de caminho saem: no teste eles são interpolados com um id a sério.
  sufixo=${rota#/api/orgs/\{org_id\}}
  alvo=$(echo "$sufixo" | sed 's/{[a-z_]*}//g; s|/$||')
  [ -z "$alvo" ] && continue
  if ! grep -q "orgId}${alvo}" "$ISO"; then
    echo "✗ isolamento: $rota não é exercitada por $ISO"
    falta=1
  fi
done < <(grep -oE '"/api/orgs/\{org_id\}[^"]*"' "$MAIN" | tr -d '"' | sort -u)

# ---- 2. recursos por ID (reunião, gravação, quadro, sala) ----
#
# A mesma pergunta, outra família (R96). Uma sala é uma CAPABILITY — quem sabe
# o código vê os metadados e pede para entrar. Um recurso por ID não é: a acta
# de uma reunião, o ficheiro de uma gravação e o PNG de um quadro não têm
# código para partilhar, e o `id` é opaco e não autoriza nada.
#
# O que se compara é o SUFIXO do recurso (`/minutes`, `/agenda`, `/png`), que é
# como o teste o escreve com o id interpolado.
while read -r rota; do
  # As públicas por desenho têm a razão escrita no rotas-publicas.txt.
  grep -qF "$rota" scripts/rotas-publicas.txt && continue
  base=$(echo "$rota" | sed -E 's|^/api/([a-z-]+).*|\1|')
  sufixo=$(echo "$rota" | sed -E 's|^/api/[a-z-]+||; s|/\{[a-z_]+\}||g')
  # Rotas de colecção (sem parâmetro) não são recursos por id.
  echo "$rota" | grep -q '{' || continue
  if ! grep -qE "/api/${base}/\\$\{[A-Za-z0-9_.]+\}${sufixo}" "$ISO"; then
    echo "✗ isolamento: $rota (recurso por id) não é exercitada por $ISO"
    falta=1
  fi
done < <(grep -oE '"/api/(rooms|recordings|meetings|whiteboards|action-items)[^"]*"' "$MAIN" | tr -d '"' | sort -u)

if [ "$falta" -eq 0 ]; then
  n=$(grep -oE '"/api/(orgs/\{org_id\}|rooms|recordings|meetings|whiteboards|action-items)[^"]*"' "$MAIN" | sort -u | wc -l)
  echo "✓ isolamento: as $n rotas de inquilino e de recurso são todas exercitadas"
fi
exit $falta
