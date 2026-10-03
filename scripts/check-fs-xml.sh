#!/usr/bin/env bash
# ============================================================
#  Fitness function: o XML de configuração do FreeSWITCH (R226).
#
#  Até aqui nada lia estes ficheiros: só a sintaxe do Lua tinha portão. A R226
#  encontrou três defeitos que um leitor do XML não vê e que só apareciam com
#  o FreeSWITCH a arrancar — ou a não arrancar. Este portão procura-os em todo
#  o *.xml e *.xml.inc debaixo de voice/:
#
#   1. XML mal formado.
#   2. Uma directiva X-PRE-PROCESS dentro de um comentário. O pré-processador
#      do FreeSWITCH procura-a em cada linha sem olhar a comentários
#      (switch_xml.c) e EXECUTA-A: o vars.xml.inc tinha o seu próprio include
#      no cabeçalho, incluía-se a si próprio e o arranque morria com
#      «unclosed <!--». Para desactivar uma directiva, muda-lhe o nome
#      (X-NO-PRE-PROCESS), como faz a vanilla.
#   3. `$${NOME_EM_MAIÚSCULAS}`. `$${…}` lê outra variável GLOBAL do
#      FreeSWITCH, não o ambiente: ficava vazia em silêncio. O ambiente lê-se
#      com cmd="env-set" e $NOME.
#
#  É estático: não prova que o FreeSWITCH carrega a configuração, nem o que
#  ela faz em chamada — isso é `bash scripts/softphone-prova.sh srtp-real`.
#
#  Uso:  bash scripts/check-fs-xml.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

exec python3 - <<'PYEOF'
import pathlib, re, sys
from xml.parsers import expat

files = sorted(p for p in pathlib.Path("voice").rglob("*")
               if p.is_file() and (p.name.endswith(".xml") or p.name.endswith(".xml.inc")))
if not files:
    print("✗ R226: nenhum *.xml debaixo de voice/ — o portão não está a olhar para onde devia")
    sys.exit(1)

erros = []
comentario = re.compile(r"<!--.*?-->", re.S)
ambiente = re.compile(r"\$\$\{[A-Z][A-Z0-9_]*\}")

def linha(texto, pos):
    return texto.count("\n", 0, pos) + 1

for p in files:
    texto = p.read_text(encoding="utf-8")
    try:
        expat.ParserCreate().Parse(texto, True)
    except expat.ExpatError as e:
        erros.append(f"{p}:{e.lineno}: XML mal formado ({expat.errors.messages[e.code]})")
    for c in comentario.finditer(texto):
        for m in re.finditer(r"x-pre-process", c.group(0), re.I):
            erros.append(f"{p}:{linha(texto, c.start() + m.start())}: directiva X-PRE-PROCESS dentro de um "
                         "comentário — o FreeSWITCH executa-a na mesma")
    for m in ambiente.finditer(comentario.sub(lambda c: re.sub(r"[^\n]", " ", c.group(0)), texto)):
        erros.append(f"{p}:{linha(texto, m.start())}: {m.group(0)} lê uma variável global, não o ambiente — "
                     'usa cmd="env-set" e $NOME')

if erros:
    print("✗ R226: configuração do FreeSWITCH com defeitos que só apareciam no arranque:")
    for e in erros:
        print("     " + e)
    sys.exit(1)
print(f"  ✓ XML do FreeSWITCH ({len(files)} ficheiros: bem formado, sem directivas em comentários, sem $${{AMBIENTE}})")
PYEOF
