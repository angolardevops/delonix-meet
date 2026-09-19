#!/usr/bin/env bash
# ============================================================
#  Fitness function de arquitetura evolutiva (Martin Fowler #8):
#  falha se a documentação (HARNESS.md) divergir do código real —
#  módulos backend e range de migrações. Corre em CI / pre-commit.
#
#  Porquê: este projeto usa a doc como harness para agentes de IA e
#  humanos. Doc desatualizada nas fronteiras é pior do que nenhuma —
#  os revisores raciocinam sobre um sistema que já não existe.
#
#  Uso:  bash scripts/check-docs-drift.sh   (exit 1 se houver drift)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

DOC="HARNESS.md"
fail=0

# 1) Cada módulo em server/src/*.rs (exceto main) tem de ser mencionado no HARNESS.md.
#    (main.rs é o bootstrap; não precisa de linha própria na tabela de módulos.)
for f in server/src/*.rs; do
  mod=$(basename "$f" .rs)
  [ "$mod" = "main" ] && continue
  if ! grep -q "\`$mod\.rs\`" "$DOC"; then
    echo "✗ drift: módulo 'server/src/$mod.rs' NÃO está documentado em $DOC (tabela §2)"
    fail=1
  fi
done

# 2) O range de migrações na doc tem de cobrir a última migração real.
last_mig=$(ls server/migrations/*.sql 2>/dev/null | sed -E 's/.*\/([0-9]+)_.*/\1/' | sort -n | tail -1)
if [ -n "${last_mig:-}" ]; then
  if ! grep -qE "0001[–-]$last_mig" "$DOC"; then
    echo "✗ drift: a última migração é '$last_mig' mas $DOC não refere o range '0001–$last_mig'"
    fail=1
  fi
fi

# 3) As versões das crates ESTRUTURAIS anunciadas na doc têm de bater com o
#    Cargo.toml. Foi drift a sério: a doc dizia axum 0.7 / sqlx 0.7 quando o
#    código já ia em 0.8 — um agente (ou um humano novo) a ler a tabela
#    raciocina sobre uma API que não é a que está lá, e escreve código que não
#    compila. Só se verificam as que mudam a forma do código.
CARGO="server/Cargo.toml"
for crate in axum sqlx webrtc redis reqwest; do
  # `name = "X.Y..."` ou `name = { version = "X.Y..." }` → fica com X.Y
  real=$(grep -E "^$crate[[:space:]]*=" "$CARGO" \
         | grep -oE '"[0-9]+\.[0-9]+' | head -1 | tr -d '"')
  [ -z "$real" ] && continue
  for doc in HARNESS.md AGENTS.md GEMINI.md; do
    [ -f "$doc" ] || continue
    # Só falha se a doc CITAR uma versão desta crate e for outra.
    claimed=$(grep -oiE "$crate[^0-9a-z]{0,3}[0-9]+\.[0-9]+" "$doc" \
              | grep -oE '[0-9]+\.[0-9]+' | sort -u)
    for c in $claimed; do
      if [ "$c" != "$real" ]; then
        echo "✗ drift: $doc diz '$crate $c' mas $CARGO tem '$real'"
        fail=1
      fi
    done
  done
done

# 4) A doc não pode anunciar verificação de SQL em compile time enquanto o
#    código usa a API de runtime. Esta mentira é cara: manda o leitor esperar
#    que um nome de coluna errado falhe no build, quando falha em produção.
macros=$(grep -rEo 'sqlx::(query|query_as|query_scalar)!' server/src | wc -l)
if [ "$macros" -eq 0 ]; then
  for doc in HARNESS.md AGENTS.md GEMINI.md; do
    [ -f "$doc" ] || continue
    if grep -qE 'query(_as)?!.*(compile.time|verificação em compile)' "$doc" \
       || grep -qiE 'macros? .*compile-time checking' "$doc"; then
      echo "✗ drift: $doc anuncia SQL verificado em compile time, mas server/src usa 0 macros \`query!\` (só API de runtime)"
      fail=1
    fi
  done
fi

# 5) O harness só pode citar revisores e skills que EXISTEM. Foi drift a sério:
#    durante meses o HARNESS.md e o AGENTS.md mandaram invocar seis revisores em
#    `agents/` que nunca estiveram no git (o `.claude/` inteiro era ignorado, e
#    `agents/` nunca foi criado). Um agente que segue o harness procurava-os,
#    não os encontrava, e seguia sem revisão — sem erro nenhum que o dissesse.
#    Verifica: (a) cada `delonix-meet-*` citado existe como agente ou skill;
#    (b) o nome no frontmatter bate com o ficheiro; (c) as ligações relativas
#    dentro de `.claude/` resolvem; (d) ninguém volta a citar o `agents/` antigo.
if ! python3 - <<'PYEOF'
import glob, os, re, sys
falha = False
agentes = {os.path.basename(p)[:-3] for p in glob.glob('.claude/agents/*.md')}
skills = {os.path.basename(os.path.dirname(p)) for p in glob.glob('.claude/skills/*/SKILL.md')}
# Os crates `delonix-meet-*` partilham o prefixo: os planeados (árvore do ADR-0004,
# `├── delonix-meet-x/`) e os que já existirem num Cargo.toml não são revisores.
crates = set(re.findall(r'(delonix-meet-[a-z]+)/', open('docs/adr/0004-organizacao-alvo-do-backend.md', encoding='utf-8').read()))
# O ADR-0006 refina a lista (tabela `| \`delonix-meet-x\` |`) — os crates dele também não são revisores.
crates |= set(re.findall(r'\| `(delonix-meet-[a-z]+)` \|', open('docs/adr/0006-backend-enterprise-contextos-edicoes-e-entrega.md', encoding='utf-8').read()))
for toml in glob.glob('server/**/Cargo.toml', recursive=True):
    crates |= set(re.findall(r'name\s*=\s*"(delonix-meet-[a-z]+)"', open(toml, encoding='utf-8').read()))

for p in sorted(glob.glob('.claude/agents/*.md')) + sorted(glob.glob('.claude/skills/*/SKILL.md')):
    texto = open(p, encoding='utf-8').read()
    esperado = os.path.basename(p)[:-3] if '/agents/' in p else os.path.basename(os.path.dirname(p))
    m = re.search(r'^name:\s*(\S+)', texto, re.M)
    if not m or m.group(1) != esperado:
        print(f"✗ drift: {p} tem 'name: {m.group(1) if m else '∅'}' mas devia ser '{esperado}'")
        falha = True
    for alvo in re.findall(r'\]\(([^)#\s]+)(?:#[^)]*)?\)', texto):
        if re.match(r'^[a-z]+:', alvo):
            continue
        if not os.path.exists(os.path.normpath(os.path.join(os.path.dirname(p), alvo))):
            print(f"✗ drift: {p} liga a '{alvo}', que não existe")
            falha = True

docs = ['HARNESS.md', 'AGENTS.md', 'GEMINI.md', '.cursorrules', '.clinerules',
        '.github/copilot-instructions.md'] + glob.glob('docs/**/*.md', recursive=True) \
       + glob.glob('.claude/**/*.md', recursive=True)
for p in docs:
    if not os.path.exists(p):
        continue
    texto = open(p, encoding='utf-8').read()
    for nome in sorted(set(re.findall(r'\bdelonix-meet-[a-z]+\b', texto))):
        if nome not in agentes and nome not in skills and nome not in crates:
            print(f"✗ drift: {p} cita '{nome}', que não existe em .claude/agents/ nem em .claude/skills/")
            falha = True
    for n, linha in enumerate(texto.splitlines(), 1):
        if re.search(r'(?<![.\w/])agents/(?!worktrees|launch\.json)', linha):
            print(f"✗ drift: {p}:{n} cita 'agents/' — os revisores estão em .claude/agents/")
            falha = True
sys.exit(1 if falha else 0)
PYEOF
then
  fail=1
fi

if [ "$fail" = 0 ]; then
  echo "✓ docs em sincronia com o código (módulos + migrações + versões + SQL + revisores e skills)"
fi
exit $fail
