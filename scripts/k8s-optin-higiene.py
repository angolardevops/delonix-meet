#!/usr/bin/env python3
"""Higiene dos manifestos OPT-IN de `deploy/k8s` — os que a kustomization NÃO
inclui, e que por isso nenhum `kubectl kustomize` renderiza.

PORQUE EXISTE. A kustomization inclui 12 dos 17 ficheiros. Os outros aplicam-se
à mão (`kubectl apply -f deploy/k8s/52-data-plain.yaml`), e não passavam por
portão nenhum: foi assim que o `52-data-plain.yaml` andou sem `securityContext`
e o `50-data.yaml` sem largar capacidades, sem nada acusar. O
`check-k8s-render.sh` lê o que os renders PRODUZEM — e um ficheiro fora da
kustomization não é produzido por render nenhum.

QUATRO INVARIANTES, e deliberadamente NÃO a `livenessProbe`: numa base de dados
uma sonda de liveness transforma uma consulta lenta num ciclo de reinícios, e a
readiness já a tira do Service. Onde a liveness faz falta é num worker que se
pendura, e isso precisa de código no worker.

Sem PyYAML não dá — este portão já corre num CI que o tem (os outros blocos
Python do check-k8s-render.sh importam yaml).

Uso:  k8s-optin-higiene.py <dir>        (por omissão deploy/k8s)
"""

import glob
import os
import re
import sys

import yaml

# Excepções, cada uma com a razão pela qual NÃO é só uma linha de YAML.
# Formato: (ficheiro, workload, contentor, invariante) -> razão
EXCEPCOES = {
    ("09-whisper.yaml", "delonix-whisper", "whisper", "non-root"):
        "a imagem é construída neste repo (delonix-whisper:latest); pôr non-root "
        "exige saber/fixar o uid no Dockerfile dela, não é uma linha de manifesto",
}


def falta_de(pod_spec: dict, ct: dict) -> list[str]:
    sc = ct.get("securityContext") or {}
    res = ct.get("resources") or {}
    falhas = []
    if "ALL" not in ((sc.get("capabilities") or {}).get("drop") or []):
        falhas.append("drop-ALL")
    if sc.get("allowPrivilegeEscalation") is not False:
        falhas.append("no-escalada")
    if not res.get("requests") or not res.get("limits"):
        falhas.append("recursos")
    if not (pod_spec.get("securityContext") or {}).get("runAsNonRoot"):
        falhas.append("non-root")
    return falhas


def main() -> int:
    base = sys.argv[1] if len(sys.argv) > 1 else "deploy/k8s"
    kust = os.path.join(base, "kustomization.yaml")
    if not os.path.exists(kust):
        print(f"✗ opt-in: não encontrei {kust}")
        return 1
    with open(kust, encoding="utf-8") as f:
        incluidos = set(re.findall(r"^\s*-\s+(\S+\.yaml)", f.read(), re.M))
    if not incluidos:
        print(f"✗ opt-in: não consegui ler os `resources` de {kust}")
        return 1

    erros, vistos, usadas = [], 0, set()
    for caminho in sorted(glob.glob(os.path.join(base, "*.yaml"))):
        nome = os.path.basename(caminho)
        if nome == "kustomization.yaml" or nome in incluidos:
            continue
        with open(caminho, encoding="utf-8") as f:
            docs = [d for d in yaml.safe_load_all(f) if d]
        for d in docs:
            tpl = (d.get("spec") or {}).get("template")
            if not isinstance(tpl, dict) or "spec" not in tpl:
                continue
            pod = tpl["spec"]
            quem = d["metadata"]["name"]
            for ct in pod.get("containers", []) + pod.get("initContainers", []):
                vistos += 1
                for inv in falta_de(pod, ct):
                    chave = (nome, quem, ct["name"], inv)
                    if chave in EXCEPCOES:
                        usadas.add(chave)
                        continue
                    erros.append(
                        f"✗ opt-in: {nome} {d['kind']}/{quem}:{ct['name']} sem «{inv}» — "
                        "este ficheiro está FORA da kustomization, logo nenhum render o cobre; "
                        "aplica-se com `kubectl apply -f` e chega ao cluster assim mesmo"
                    )

    # Uma excepção que já não é usada é lixo que esconde a próxima regressão.
    for chave, razao in EXCEPCOES.items():
        if chave not in usadas:
            erros.append(f"✗ opt-in: a excepção {chave} já não é necessária ({razao}) — apaga-a")

    if not vistos:
        print("✗ opt-in: não encontrei workload nenhum fora da kustomization — o portão "
              "deixou de saber ler os ficheiros")
        return 1
    for e in erros:
        print(e)
    if erros:
        return 1
    print(f"  ✓ opt-in: {vistos} contentores fora da kustomization com recursos, "
          f"capacidades largadas, sem escalada e non-root "
          f"({len(EXCEPCOES)} excepção(ões) com razão escrita)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
