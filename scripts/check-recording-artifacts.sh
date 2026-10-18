#!/usr/bin/env bash
# ============================================================
#  Fitness function: todo o nome de ficheiro que o código de produção
#  constrói está CLASSIFICADO, e todo o que é um derivado de uma gravação
#  passa pelo registo (`recording_artifacts`, migração 0126).
#
#  PORQUE EXISTE. Medido a 2026-10-18 no `ece92e6a4`: os ficheiros derivados de
#  uma gravação não estavam registados em sítio nenhum, e cada caminho que
#  apaga tinha de se lembrar deles um a um. A retenção fazia três
#  `let _ = remove_file(...)` à mão — a composta, a miniatura e a pasta das
#  pistas — e o quarto derivado FICOU ESQUECIDO: os `clip-<id>.webm`/`.vtt` da
#  migração 0118 não eram apagados por varredor nenhum
#  (`grep -rn "clip-" server/src/recorder.rs` não dava um único acerto). Nada
#  disto dava vermelho: o `ON DELETE CASCADE` levava as LINHAS e fazia a fuga
#  parecer fechada.
#
#  O defeito é portanto de OMISSÃO, e repete-se da mesma maneira com o derivado
#  seguinte (waveform, legendas por pista, pré-visualização). É o que este
#  portão existe para travar: um literal novo com forma de nome de ficheiro
#  obriga quem o escreve a dizer o que ele é, e se for um derivado de uma
#  gravação a ligá-lo ao registo.
#
#  O QUE SE MEDE, nos dois sentidos:
#
#   1. todo o literal de `server/src/**.rs` com forma de nome de ficheiro (um
#      `{marcador}` e uma extensão) ou de pasta derivada (`tmp-`/`iso-`/
#      `{ISO_PREFIX}`) está em `scripts/artefactos-de-gravacao.txt`;
#   2. toda a linha dessa lista encontra o seu literal no código — é o
#      CONTROLO POSITIVO, e não é decoração: um `grep` que não encontra nada é
#      indistinguível de um `grep` que procurou no sítio errado. Sem isto, mudar
#      o nome de um módulo ou do `recordings_dir` deixava o portão verde a medir
#      o vazio (foi assim que um `location /ws {` vazio passou em silêncio);
#   3. o vocabulário dos `kind` da lista casa EXACTAMENTE com as constantes
#      `KIND_*` do `recording_artifacts.rs`, nos dois sentidos;
#   4. todo o ficheiro com uma linha `registado:` chama de facto um
#      `register`/`register_canonical` — a classificação não é uma promessa.
#
#  O QUE ESTE PORTÃO NÃO MEDE, e vale dizê-lo:
#
#   · que a linha seja escrita ANTES do ficheiro. A ordem prova-se a correr:
#     `recorder::tests::sem_registo_as_pistas_ficam_no_tmp` deixa a tabela em
#     falta e exige que os ficheiros NÃO saiam do `tmp-`;
#   · que o produtor seja exercitado por um teste. Isso é do
#     `nenhum_ficheiro_no_disco_fica_sem_linha_no_registo`
#     (`server/tests/artefactos_de_gravacao.rs`), que varre o `recordings_dir`
#     e não enumera nomes;
#   · ficheiros construídos por concatenação em vez de um literal
#     (`push(nome)`, `with_extension`). Um nome sem literal não é visível daqui.
#   · `server/tests/**` — as fixtures dos testes de integração não são
#     produtores. (O que lá vive e PRODUZ é o código de produção que elas
#     chamam, e esse está em `src/`.)
#
#  Uso:  bash scripts/check-recording-artifacts.sh   (exit 1 se divergir)
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

LISTA=scripts/artefactos-de-gravacao.txt
MODULO=server/src/recording_artifacts.rs
falhas=0

erro() {
    echo "  ✗ $*"
    falhas=$((falhas + 1))
}

# Pertence ao conjunto? Em bash puro, e isso é deliberado: um
# `produtor | grep -q` com `pipefail` mente. O `grep -q` sai no primeiro
# acerto, fecha o pipe, o produtor leva SIGPIPE e o estado do pipeline vem
# não-zero — um «encontrei» que se lê como «não encontrei». O
# `check-repo-hygiene.sh` recusa esse padrão, e foi ele que o apanhou aqui.
contem() {
    local agulha="$1"
    shift
    local x
    for x in "$@"; do
        [ "$x" = "$agulha" ] && return 0
    done
    return 1
}

# Quantas linhas casam. `grep -c` lê a entrada toda, logo não há SIGPIPE.
conta() { grep -c "$@" || true; }

[ -f "$LISTA" ] || { echo "✗ $LISTA não existe"; exit 1; }
[ -f "$MODULO" ] || { echo "✗ $MODULO não existe — o registo mudou de sítio"; exit 1; }

# ------------------------------------------------------------
#  Os literais que o código constrói
# ------------------------------------------------------------
# Forma de nome de ficheiro: tem um `{marcador}` e acaba numa extensão curta.
# Exclui-se o que não é um caminho: rotas (`/api/...`) e SQL (`SELECT`, `$1`).
literais_no_codigo() {
    grep -rhoE '"[^"]*\{[a-zA-Z_]+\}[^"]*\.[a-z0-9]{2,6}"' --include='*.rs' server/src/ \
        | sed 's/^"//; s/"$//' \
        | grep -vE '^/|SELECT |\$[0-9]'
    # E as pastas derivadas, que não têm extensão.
    grep -rhoE '"(tmp-|iso-)\{[a-zA-Z_]+\}"|"\{ISO_PREFIX\}\{[a-zA-Z_]+\}"' \
        --include='*.rs' server/src/ \
        | sed 's/^"//; s/"$//'
}

# Em que ficheiro(s) um literal aparece.
ficheiros_de() {
    grep -rlF "\"$1\"" --include='*.rs' server/src/ | sed 's|^server/||'
}

mapfile -t CODIGO < <(literais_no_codigo | sort -u)

# CONTROLO POSITIVO da extracção: sem isto um regex partido dava verde a medir
# zero literais. Estes três SABEMOS que estão lá.
if [ "${#CODIGO[@]}" -lt 10 ]; then
    erro "a extracção de literais trouxe ${#CODIGO[@]} — o regex está partido ou o código mudou de sítio"
fi
for esperado in 'clip-{id}.webm' '{rec_id}.jpg' '{ISO_PREFIX}{rec_id}'; do
    contem "$esperado" "${CODIGO[@]}" \
        || erro "CONTROLO POSITIVO: a extracção não encontrou '$esperado', que está no código — procurou no sítio errado"
done

# ------------------------------------------------------------
#  A lista, lida uma vez
# ------------------------------------------------------------
# `ficheiro | literal | classe | razão`
LINHAS=$(grep -vE '^[[:space:]]*(#|$)' "$LISTA")
mapfile -t LISTADOS < <(
    echo "$LINHAS" | awk -F'|' '{gsub(/^[ \t]+|[ \t]+$/,"",$2); print $2}' | sort -u
)
while IFS='|' read -r f lit classe razao; do
    f=$(echo "$f" | xargs); lit=$(echo "$lit" | xargs)
    classe=$(echo "$classe" | xargs); razao=$(echo "$razao" | xargs)
    [ -n "$lit" ] || continue
    [ -n "$classe" ] || erro "'$lit' ($f) não tem classe"
    # Uma razão vazia é uma linha que não classifica nada: é o que esta lista
    # existe para impedir.
    [ "${#razao}" -ge 20 ] || erro "'$lit' ($f) não tem razão escrita (tem ${#razao} caracteres)"
    case "$classe" in
        registado:*|nao-artefacto|fora-do-recordings-dir|nome-de-exibicao|fixture|dormente) ;;
        *) erro "'$lit' ($f): classe desconhecida '$classe'" ;;
    esac
    # (2) CONTROLO POSITIVO por linha: o literal ainda está no código?
    contem "$lit" "${CODIGO[@]}" \
        || erro "'$lit' está na lista e NÃO está no código ($f) — a lista descreve um ficheiro que já não existe, ou o portão deixou de o ver"
    # ... e no ficheiro que a linha diz.
    if [ -n "$f" ] && [ "$(ficheiros_de "$lit" | conta -xF "$f")" -eq 0 ]; then
        erro "'$lit' está classificado em '$f' e o literal não aparece lá — a classificação está a descrever outro sítio"
    fi
    # (4) Uma linha `registado:<kind>` obriga o `kind` a estar LIGADO a um
    # produtor: a constante tem de ser usada em algum sítio além da sua própria
    # declaração. Um `KIND_*` que só existe na declaração é um artefacto que
    # ninguém registra — o estado exacto de que esta frente nasceu.
    #
    # A pergunta é pelo `kind` e não pelo ficheiro do literal de propósito:
    # quem constrói o nome e quem escreve a linha podem ser módulos
    # diferentes, e são (a miniatura é nomeada pelo `media_probe` e registada
    # pelo `canonical` do registo, na transacção da gravação).
    if [ "${classe#registado:}" != "$classe" ]; then
        k="${classe#registado:}"
        const="KIND_$(echo "$k" | tr '[:lower:]' '[:upper:]')"
        usos=$(grep -rhn "$const" --include='*.rs' server/src/ | conta -vF "const $const")
        if [ "$usos" -eq 0 ]; then
            erro "'$lit' ($f) diz-se '$classe' e a constante $const não é usada em sítio nenhum — nada registra este artefacto"
        fi
    fi
done <<< "$LINHAS"

# (1) O sentido que apanha o derivado seguinte.
while IFS= read -r lit; do
    contem "$lit" "${LISTADOS[@]}" \
        || erro "literal NÃO classificado: '$lit' (em $(ficheiros_de "$lit" | tr '\n' ' ')) — se é um ficheiro derivado de uma gravação tem de ser registado em recording_artifacts (0126) e, de qualquer modo, tem de ter uma linha em $LISTA"
done < <(printf '%s\n' "${CODIGO[@]}")

# ------------------------------------------------------------
#  (3) O vocabulário dos `kind`, nos dois sentidos
# ------------------------------------------------------------
mapfile -t KINDS_CODIGO < <(
    grep -oE 'const KIND_[A-Z_]+: &str = "[a-z_]+"' "$MODULO" \
        | sed 's/.*"\(.*\)"/\1/' | sort -u
)
mapfile -t KINDS_LISTA < <(
    echo "$LINHAS" | awk -F'|' '{gsub(/^[ \t]+|[ \t]+$/,"",$3); print $3}' \
        | grep '^registado:' | sed 's/^registado://' | sort -u
)
if [ "${#KINDS_CODIGO[@]}" -eq 0 ]; then
    erro "CONTROLO POSITIVO: não se extraiu nenhuma constante KIND_* de $MODULO"
fi
for k in "${KINDS_CODIGO[@]}"; do
    contem "$k" "${KINDS_LISTA[@]}" \
        || erro "o kind '$k' existe em $MODULO e nenhuma linha da lista o produz — ou sobra no código, ou falta o produtor"
done
for k in "${KINDS_LISTA[@]}"; do
    contem "$k" "${KINDS_CODIGO[@]}" \
        || erro "a lista produz o kind '$k' e ele não é uma constante KIND_* de $MODULO"
done

# ------------------------------------------------------------
#  A retenção não volta a apagar à mão
# ------------------------------------------------------------
# O defeito original era uma lista de `remove_file` escrita à mão no
# `retention_sweep`. Se voltar, volta a fuga.
#
# Os COMENTÁRIOS saem antes de medir, e isto não é detalhe: a primeira versão
# deste portão deu vermelho ao comentário que EXPLICA o defeito («aqui estavam
# três `let _ = remove_file(...)` à mão»). Uma invariante que casa com a prosa
# que a descreve é a mesma armadilha que, neste repositório, fez um
# `/pendingRoom && \(/` casar com `!pendingRoom && (` e aprovar duas vezes a
# regressão que devia travar.
sem_comentarios() { grep -vE '^[[:space:]]*//'; }
mao=$(sed -n '/pub async fn retention_sweep/,/^}/p' server/src/recorder.rs \
        | sem_comentarios | conta -E 'remove_file|remove_dir_all')
if [ "$mao" -gt 0 ]; then
    erro "o retention_sweep voltou a apagar ficheiros à mão — a lista do que uma gravação tem no disco é do registo (0126), senão o derivado seguinte é esquecido outra vez"
fi
purga=$(sed -n '/pub async fn retention_sweep/,/^}/p' server/src/recorder.rs \
          | sem_comentarios | conta -F 'recording_artifacts::purge')
if [ "$purga" -eq 0 ]; then
    erro "o retention_sweep não chama o recording_artifacts::purge — nada apaga os ficheiros das gravações expiradas"
fi

# ------------------------------------------------------------
if [ "$falhas" -gt 0 ]; then
    echo
    echo "✗ $falhas problema(s). Cada literal novo com forma de nome de ficheiro"
    echo "  precisa de uma linha em $LISTA; se for um derivado de uma gravação,"
    echo "  o produtor escreve a linha do registo ANTES do ficheiro"
    echo "  (server/src/recording_artifacts.rs)."
    exit 1
fi
echo "✓ artefactos de gravação: ${#CODIGO[@]} literais classificados, ${#KINDS_CODIGO[@]} kinds em sincronia, a retenção apaga pelo registo"
