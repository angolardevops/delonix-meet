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

if [ "$falta" -eq 0 ]; then
  n=$(grep -oE '"/api/orgs/\{org_id\}[^"]*"' "$MAIN" | sort -u | wc -l)
  echo "✓ isolamento: as $n rotas com escopo de organização são todas exercitadas"
fi
exit $falta
