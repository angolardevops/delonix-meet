#!/usr/bin/env bash
# ============================================================
#  Fitness function: o `CHECK` do `recording_tracks.kind` aceita EXACTAMENTE
#  os `kind` que o código produz.
#
#  PORQUE EXISTE. Medido a 2026-10-18 no `049948388`: o `CHECK` da migração
#  0124 foi escrito a partir de um COMENTÁRIO do `open_track` que enumerava
#  três dos quatro valores («"video" | "screen" | "audio"»). Faltava o
#  `screen-audio` — o som do sistema de quem partilha ecrã, que o SFU produz,
#  grava num ficheiro próprio e publica há muito. Consequência: o `INSERT`
#  dessa pista era recusado pela base, o `registar_pistas_iso` escrevia um
#  `tracing::error!` e seguia, e o ficheiro ficava em `iso-<gravação>/` para
#  sempre — sem nenhuma linha que o pudesse pedir e sem ninguém a ver um erro.
#
#  Nada disto dava vermelho em sítio nenhum. A migração estava sintacticamente
#  correcta, o código compilava, a rota devolvia 200 com a lista incompleta. É
#  precisamente a forma de defeito que um portão tem de apanhar: as duas listas
#  divergem em silêncio, e quem acrescentar um `kind` novo ao SFU sem migração
#  repete o defeito exactamente da mesma maneira.
#
#  O QUE SE MEDE: a lista do `recorder::TRACK_KINDS` (a fonte de verdade
#  em código, de onde o SFU constrói os `kind`) contra a lista do `CHECK` da
#  migração mais recente que o define. Ordenadas, comparadas como conjuntos.
#
#  CONTROLO POSITIVO, e não é decoração: um `grep` que não encontra nada é
#  indistinguível de um `grep` que procurou no sítio errado. As duas
#  extracções têm de trazer `video` — que SABEMOS estar nas duas — senão o
#  portão falha por não ter medido, em vez de dar verde por engano.
#
#  O QUE ESTE PORTÃO NÃO MEDE: que o SFU não construa um `kind` fora das
#  constantes (um literal novo escrito à mão). Isso é do
#  `a_base_aceita_todos_os_kinds_e_recusa_um_inventado`, em
#  `server/src/recorder.rs`, que insere cada valor contra Postgres real e
#  afirma que um valor inventado é RECUSADO.
#
#  Uso:  bash scripts/check-recording-track-kinds.sh   (exit 1 se divergirem)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

RECORDER=${RECORDER_RS:-server/crates/delonix-meet-domain/src/content/composition.rs}
MIGS=${MIGRATIONS_DIR:-server/migrations}
r=$'\e[31m'; g=$'\e[32m'; z=$'\e[0m'
fail=0

# ---- 1. a lista do código ---------------------------------------------------
# `pub const TRACK_KINDS: [&str; N] = [KIND_A, KIND_B, …];` nomeia
# constantes, não literais — logo resolvem-se as constantes citadas.
#
# O ficheiro é ACHATADO primeiro (`tr '\n' ' '`) porque a declaração não cabe
# sempre numa linha: medido, o `rustfmt` parte o array em várias linhas assim
# que lhe acrescentam um quinto valor, e um `grep -oP` por linha devolvia a
# lista VAZIA e punha o portão vermelho numa alteração correcta — a falhar pela
# razão errada, a apontar para o sítio errado.
nomes=$(tr '\n' ' ' < "$RECORDER" \
        | grep -oP 'TRACK_KINDS\s*:\s*\[&str;\s*\d+\]\s*=\s*\[\K[^]]*' \
        | tr -d ' ' | tr ',' '\n' | grep -v '^$')
codigo=""
for n in $nomes; do
  # `pub const KIND_X: &str = "valor";`
  v=$(grep -oP "const\\s+${n}\\s*:\\s*&str\\s*=\\s*\"\\K[^\"]+" "$RECORDER" | head -1)
  if [ -z "$v" ]; then
    echo "${r}✗ kinds: o TRACK_KINDS cita «$n», que o $RECORDER não define${z}"
    fail=1
    continue
  fi
  codigo+="$v"$'\n'
done
codigo=$(printf '%s' "$codigo" | grep -v '^$' | sort -u)

# ---- 2. a lista da base -----------------------------------------------------
# A migração mais recente (ordem de nome, que é a ordem de aplicação) que define
# o `CHECK` do `kind`.
#
# **Os comentários SAEM primeiro**, e é o ponto todo: este portão existe porque
# uma restrição foi escrita a partir de um comentário, e a primeira versão dele
# lia os comentários como se fossem DDL. Uma migração que explicasse a história
# («a 0124 tinha `kind IN ('video','screen','audio')`») era lida como a
# restrição em vigor — o portão a repetir, sobre si mesmo, o defeito que
# persegue. `sed` tira tudo de `--` até ao fim da linha antes de qualquer grep.
# O `grep -q` lê de uma VARIÁVEL, nunca de um pipe: com `pipefail`, o SIGPIPE
# que ele provoca ao sair cedo dá um falso «não encontrado» — e é o portão a
# mentir sobre a sua própria medição.
sem_comentarios() { sed -E 's/--.*$//' "$1"; }
ultima=""
for f in $(ls "$MIGS"/*.sql 2>/dev/null | sort); do
  ddl=$(sem_comentarios "$f")
  grep -qE 'kind[[:space:]]+IN[[:space:]]*\(' <<<"$ddl" || continue
  grep -q recording_tracks <<<"$ddl" || continue
  ultima=$f
done
if [ -z "$ultima" ]; then
  echo "${r}✗ kinds: não encontrei nenhuma migração com o CHECK do recording_tracks.kind em $MIGS${z}"
  echo "   (o portão não mediu nada — isto é falha, não verde)"
  exit 1
fi
base=$(sem_comentarios "$ultima" \
       | grep -oP 'kind[[:space:]]+IN[[:space:]]*\(\K[^)]*' \
       | tr -d " '" | tr ',' '\n' | grep -v '^$' | sort -u)

# ---- 3. controlo positivo: as duas extracções funcionaram -------------------
for par in "código:$codigo:$RECORDER" "base:$base:$ultima"; do
  nome=${par%%:*}; resto=${par#*:}; lista=${resto%:*}; onde=${resto##*:}
  if [ -z "$lista" ]; then
    echo "${r}✗ kinds: a lista do $nome veio VAZIA ($onde) — a extracção partiu-se${z}"
    fail=1
  # `grep -q` com uma herestring, nunca atrás de um pipe: com `pipefail`, o
  # SIGPIPE que o `grep -q` provoca ao sair cedo dá um falso «não encontrado» —
  # e um controlo positivo que mente é pior do que não o ter.
  elif ! grep -qx video <<<"$lista"; then
    echo "${r}✗ kinds: a lista do $nome não traz «video» ($onde) — o portão está a olhar para o sítio errado${z}"
    fail=1
  fi
done
[ "$fail" -eq 0 ] || exit 1

# ---- 4. a comparação --------------------------------------------------------
so_codigo=$(comm -23 <(printf '%s\n' "$codigo") <(printf '%s\n' "$base"))
so_base=$(comm -13 <(printf '%s\n' "$codigo") <(printf '%s\n' "$base"))

if [ -n "$so_codigo" ]; then
  echo "${r}✗ kinds: o código produz $(printf '%s ' $so_codigo)— e o CHECK de $ultima RECUSA-o${z}"
  echo "   É a fuga do 049948388 a repetir-se: a base rejeita o INSERT, o ficheiro"
  echo "   fica no disco sem linha que o sirva. Falta uma migração a acrescentar o valor."
  fail=1
fi
if [ -n "$so_base" ]; then
  echo "${r}✗ kinds: o CHECK de $ultima aceita $(printf '%s ' $so_base)— que o $RECORDER não nomeia${z}"
  echo "   Um valor que nenhum código produz é uma promessa que ninguém cumpre:"
  echo "   ou se acrescenta a constante, ou sai do CHECK."
  fail=1
fi

if [ "$fail" -eq 0 ]; then
  echo "${g}✓ kinds: $(printf '%s ' $codigo)— código e CHECK de $(basename "$ultima") de acordo${z}"
fi
exit "$fail"
