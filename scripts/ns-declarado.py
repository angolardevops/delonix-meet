#!/usr/bin/env python3
"""Lê UMA das declarações do namespace do Meet, para o check-observabilidade.sh
as poder cruzar.

O namespace onde o Meet corre está escrito em três sítios independentes, e nada
os mantinha em passo. Esquecer o do ServiceMonitor num rename não dá erro: dá
zero alvos, com o painel verde e o produto em baixo.

Sem PyYAML de propósito — o CI não instala nada para os portões, e um portão que
depende de um pacote que pode não estar é um portão que salta em silêncio.

Uso:  ns-declarado.py {namespace|scrape|comando} <ficheiro>
Imprime o namespace, ou nada (código 1) se não o conseguir ler.
"""

import re
import sys


def do_objecto(texto: str) -> str | None:
    """O nome do objecto `kind: Namespace` — não de outro kind no mesmo ficheiro."""
    for doc in texto.split("\n---\n"):
        if re.search(r"^kind:\s*Namespace\s*$", doc, re.M):
            m = re.search(r"^\s+name:\s*(\S+)", doc, re.M)
            if m:
                return m.group(1).strip("\"'")
    return None


def do_scrape(texto: str) -> str | None:
    """O `namespaceSelector.matchNames` do ServiceMonitor, em qualquer das duas
    formas de YAML. Mais do que um nome é devolvido separado por vírgulas, para
    que o cruzamento falhe em vez de aceitar o primeiro."""
    m = re.search(r"^\s*matchNames:\s*\[([^\]]+)\]", texto, re.M)
    if m:
        nomes = [x.strip().strip("\"'") for x in m.group(1).split(",")]
        return ",".join(n for n in nomes if n)
    bloco = re.search(r"^\s*matchNames:\s*\n((?:[ \t]+-[ \t]*\S+\n?)+)", texto, re.M)
    if bloco:
        nomes = [
            linha.strip().lstrip("-").strip().strip("\"'")
            for linha in bloco.group(1).strip().split("\n")
        ]
        return ",".join(n for n in nomes if n)
    return None


def do_comando(texto: str) -> str | None:
    """O `-n <ns>` do comando `helm upgrade` documentado no cabeçalho."""
    m = re.search(r"^#.*helm upgrade.*?-n\s+([\w-]+)", texto, re.M)
    return m.group(1) if m else None


LEITORES = {"namespace": do_objecto, "scrape": do_scrape, "comando": do_comando}


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[1] not in LEITORES:
        print(f"uso: {sys.argv[0]} {{{'|'.join(LEITORES)}}} <ficheiro>", file=sys.stderr)
        return 2
    with open(sys.argv[2], encoding="utf-8") as f:
        ns = LEITORES[sys.argv[1]](f.read())
    if not ns:
        return 1
    print(ns)
    return 0


if __name__ == "__main__":
    sys.exit(main())
