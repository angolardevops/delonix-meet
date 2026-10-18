#!/usr/bin/env bash
# ============================================================
#  Fitness function: catraca do audit de dependências.
#
#  Porquê catraca e não `npm audit --audit-level=high`: a árvore ENTRA com 7
#  avisos (6 altos + 1 crítico), todos por transitividade das bibliotecas de
#  IA no browser. Um portão absoluto ficava vermelho no primeiro dia e seria
#  desligado na semana seguinte — que é a pior das saídas, porque depois já
#  ninguém repara no aviso NOVO. Esta catraca deixa passar o que já está
#  aceite e escrito em `scripts/dep-audit-accepted.txt` (com a razão), e falha
#  em qualquer pacote vulnerável que apareça de novo.
#
#  Reavaliar a lista aceite é trabalho com dono, não é «para sempre».
#
#  Uso:  bash scripts/check-dep-audit.sh
#        BLESS=1 bash scripts/check-dep-audit.sh   (regrava a lista aceite)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
ACCEPTED="scripts/dep-audit-accepted.txt"
fail=0

# ---------- npm ----------
if [ -d web/node_modules ] || npm --version >/dev/null 2>&1; then
  current=$(cd web && npm audit --json 2>/dev/null \
    | python3 -c '
import json,sys
try: d=json.load(sys.stdin)
except Exception: sys.exit(0)
for n,v in sorted(d.get("vulnerabilities",{}).items()):
    if v.get("severity") in ("high","critical"):
        print(n)
' || true)

  if [ "${BLESS:-0}" = "1" ]; then
    { echo "# Pacotes com avisos ALTOS/CRÍTICOS aceites conscientemente."
      echo "# Cada linha precisa de razão. Rever a cada release."
      echo "$current"
    } > "$ACCEPTED"
    echo "✓ lista de avisos aceites regravada ($(echo "$current" | grep -c . ) pacotes)"
    exit 0
  fi

  accepted=$(grep -vE '^\s*(#|$)' "$ACCEPTED" 2>/dev/null | awk '{print $1}' | sort -u)
  novos=$(comm -23 <(echo "$current" | sort -u) <(echo "$accepted") | grep -v '^$' || true)
  if [ -n "$novos" ]; then
    echo "✗ audit: aviso ALTO/CRÍTICO NOVO em dependências npm:"
    echo "$novos" | sed 's/^/     /'
    echo "     Vê o detalhe:  cd web && npm audit"
    echo "     Se for para aceitar, acrescenta a $ACCEPTED COM A RAZÃO."
    fail=1
  else
    echo "  ✓ npm: sem avisos altos/críticos novos"
  fi
fi

# ---------- cargo ----------
RUSTSEC_ACCEPTED="scripts/rustsec-accepted.txt"

# ---- a razão de uma aceitação tem de CONTINUAR verdadeira ----
# Os três avisos do `zip` (RUSTSEC-2026-0367/0368/0369, um deles 9.1 crítico)
# estão aceites com a razão «só escrevemos ZIP, nunca extraímos» — os três são
# exclusivamente sobre `ZipArchive::extract` e `ZipStreamReader::extract`. Essa
# razão deixa de ser verdadeira no instante em que o `server/` ganhar uma API
# de extracção, e quem a acrescentar NÃO vem ler o ficheiro de aceites. Por
# isso é aqui que se recusa: a aceitação verifica-se a si mesma ou não vale.
# Procura-se a CHAMADA `.extract(`, não o tipo. Nomear o `ZipArchive` não é
# extrair: server/tests/account_data_export.rs:26 constrói um para LER as
# entradas (`by_index` + `read_to_string`) do ZIP que o próprio teste pediu,
# e isso não toca no caminho vulnerável. Vigiar o tipo punha esse teste
# vermelho; vigiar a chamada apanha o que os avisos descrevem. As formas
# `Tipo::extract(x)` entram à parte, que o `.extract(` não as apanha.
# Corre fora do `command -v cargo-audit`: é um grep, e nesta máquina o
# cargo-audit pode não estar instalado — o guarda não se pode calar por isso.
#
# A detecção dos IDs imita o parser do `--ignore` abaixo (sem comentários,
# primeiro campo) em vez de ancorar em `^RUSTSEC`: uma linha INDENTADA é
# aceite pelo parser, e com a âncora o guarda ficava mudo justamente quando o
# cargo-audit estava a ignorar o aviso.
aceites_zip=$(grep -vE '^\s*(#|$)' "$RUSTSEC_ACCEPTED" 2>/dev/null \
                | awk '{print $1}' | grep -cE '^RUSTSEC-2026-036[789]$' || true)
if [ "$aceites_zip" -gt 0 ]; then
  # CONTROLO POSITIVO, primeiro. Um grep que não encontra nada é
  # indistinguível de um grep que procurou no sítio errado — e um portão que
  # passa em silêncio é pior do que portão nenhum. Se o `ZipWriter` de
  # data_exports.rs deixar de aparecer, os caminhos abaixo estão obsoletos
  # (ficheiro movido, crate renomeada) e o guarda perdeu o rasto: recusa.
  if ! grep -rqE 'ZipWriter' server/src --include='*.rs' 2>/dev/null; then
    echo "✗ audit: o guarda do zip perdeu o rasto — não encontrou o \`ZipWriter\`"
    echo "     em server/src (devia estar em server/src/data_exports.rs). Os"
    echo "     caminhos deste guarda estão obsoletos: corrige-os, senão ele"
    echo "     passa a aprovar em silêncio um \`extract\` que não vê."
    fail=1
  fi
  # `server/target` e `server/vendor` ficam FORA: são código de terceiros, e
  # um `ZipArchive` lá dentro não é nosso. Os quatro de dentro são onde o
  # nosso Rust vive — `ls -d server/*/` para confirmar se isto mudar.
  # As linhas de COMENTÁRIO não contam: mencionar `ZipArchive::extract` num
  # aviso ao colega não é extrair, e punha o CI vermelho sem mudança real.
  extraccoes=$(grep -rnE '\.extract\(|Zip(Archive|StreamReader)::extract' \
                 server/src server/crates server/tests server/examples \
                 --include='*.rs' 2>/dev/null \
                 | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' || true)
  if [ -n "$extraccoes" ]; then
    echo "✗ audit: o server/ passou a EXTRAIR ZIP, e os RUSTSEC-2026-0367/0368/0369"
    echo "     estão aceites em $RUSTSEC_ACCEPTED com a razão «nunca extraímos»:"
    echo "$extraccoes" | sed 's/^/     /'
    echo '     Sobe o `zip` para >=9.0.0 (onde estão corrigidos) e APAGA as três'
    echo "     linhas de aceitação. A razão escrita acabou de ficar falsa."
    fail=1
  else
    echo "  ✓ zip: sem APIs de extracção no server/ — a razão dos -0367/0368/0369 mantém-se"
  fi
fi

if command -v cargo-audit >/dev/null 2>&1; then
  # Cada ID aceite entra como --ignore. Um aviso NOVO não está na lista e
  # portanto faz falhar — que é exactamente o ponto da catraca.
  ignores=(); n_ignored=0
  while read -r id _; do
    case "$id" in RUSTSEC-*) ignores+=(--ignore "$id"); n_ignored=$((n_ignored+1));; esac
  done < <(grep -vE '^\s*(#|$)' "$RUSTSEC_ACCEPTED" 2>/dev/null || true)

  if cargo audit --file server/Cargo.lock "${ignores[@]+"${ignores[@]}"}" >/tmp/dlx-cargo-audit.log 2>&1; then
    echo "  ✓ cargo-audit: sem avisos RustSec novos ($n_ignored aceites em $RUSTSEC_ACCEPTED)"
  else
    echo "✗ audit: aviso RustSec NOVO em server/Cargo.lock:"
    tail -40 /tmp/dlx-cargo-audit.log | sed 's/^/     /'
    echo "     Se for para aceitar, acrescenta o ID a $RUSTSEC_ACCEPTED COM A RAZÃO."
    fail=1
  fi
else
  echo "  · cargo-audit ausente — instala com 'cargo install cargo-audit --locked' (o CI instala-o)"
fi

[ "$fail" = 0 ] && echo "✓ audit de dependências sem novidades"
exit $fail
