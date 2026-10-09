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
# Vazio a 2026-10-09: a última excepção (o `09-whisper` non-root) foi FECHADA —
# o uid ficou fixo no whisper-server/Dockerfile e medido com o motor. Fica aqui
# a estrutura, e a regra que a acompanha: uma excepção traz a razão pela qual
# NÃO é só uma linha de YAML, e o portão falha quando ela deixa de ser
# necessária — uma excepção morta esconde a próxima regressão.
EXCEPCOES: dict[tuple[str, str, str, str], str] = {}


def envs_das_sondas(ct: dict) -> set[str]:
    """As variáveis que os comandos das sondas `exec` referem.

    Porquê: uma sonda que refere `$FOO` que o contentor NÃO declara não dá
    erro — dá `test -f ""`, que é sempre falso, ou um `stat` de nada. O pod
    entra em ciclo de reinícios (ou nunca arranca) por causa de um nome
    trocado, e a mensagem não aponta para lado nenhum.
    """
    nomes = set()
    for chave in ("readinessProbe", "livenessProbe", "startupProbe"):
        cmd = ((ct.get(chave) or {}).get("exec") or {}).get("command") or []
        for parte in cmd:
            nomes |= set(re.findall(r"\$\{?([A-Z_][A-Z0-9_]*)\}?", str(parte)))
    return nomes


def envs_declaradas(ct: dict) -> set[str]:
    return {e["name"] for e in (ct.get("env") or []) if "name" in e}


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
                em_falta = envs_das_sondas(ct) - envs_declaradas(ct)
                for nome_var in sorted(em_falta):
                    erros.append(
                        f"✗ opt-in: {nome} {d['kind']}/{quem}:{ct['name']} tem uma sonda que "
                        f"refere ${nome_var} e o contentor não declara essa variável — a sonda "
                        "não dá erro, dá sempre falso, e o pod entra em ciclo de reinícios"
                    )
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

    # Nenhum manifesto DESTE REPO pode FIXAR um priorityClassName — nem a base,
    # nem os overlays que publicamos. Não sabemos que classes existem no cluster
    # de quem nos aplica, e um nome que não exista faz o API server RECUSAR o
    # pod: a instalação falha no `kubectl apply`, não num aviso. No chart é uma
    # maçaneta (cruzamento 6d do check-helm.sh); no kustomize, quem precisar
    # dela põe-na num overlay SEU, fora deste repo.
    #
    # Esta verificação cobre TODOS os ficheiros, não só os opt-in — daí o
    # prefixo «k8s» e não «opt-in».
    raiz = os.path.dirname(base.rstrip("/")) or "."
    candidatos = sorted(glob.glob(os.path.join(base, "*.yaml")))
    candidatos += sorted(glob.glob(os.path.join(raiz, "k8s-overlays", "**", "*.yaml"),
                                   recursive=True))
    for caminho in candidatos:
        nome_f = os.path.relpath(caminho, raiz)
        if os.path.basename(caminho) == "kustomization.yaml":
            continue
        with open(caminho, encoding="utf-8") as f:
            try:
                docs_f = [x for x in yaml.safe_load_all(f) if x]
            except yaml.YAMLError:
                continue  # patches estratégicos podem não ser YAML inteiro
        for d in docs_f:
            if not isinstance(d, dict):
                continue
            tpl = (d.get("spec") or {}).get("template")
            if not isinstance(tpl, dict) or not isinstance(tpl.get("spec"), dict):
                continue
            pcn = tpl["spec"].get("priorityClassName")
            if pcn:
                quem = (d.get("metadata") or {}).get("name", "?")
                erros.append(
                    f"✗ k8s: {nome_f} {d.get('kind', '?')}/{quem} fixa priorityClassName "
                    f"«{pcn}» — este repo não sabe que classes existem no cluster de quem o "
                    "aplica, e uma que não exista faz o API server recusar o pod. No chart é "
                    "uma maçaneta; aqui, põe-na num overlay teu, fora deste repo"
                )

    # O batimento do worker de IA: a variável que as sondas leem tem de ser a que
    # o worker escreve. São dois ficheiros em linguagens diferentes, e um rename
    # num deles dá uma sonda que mede um ficheiro que ninguém toca — ou seja, um
    # pod que reinicia a cada 12 minutos sem razão visível.
    worker_yaml = os.path.join(base, "60-ai-gpu-worker.yaml")
    worker_py = os.path.join("ai-worker", "transcribe_worker.py")
    if os.path.exists(worker_yaml) and os.path.exists(worker_py):
        with open(worker_yaml, encoding="utf-8") as f:
            texto = f.read()
        if "HEARTBEAT_FILE" in texto:
            with open(worker_py, encoding="utf-8") as f:
                fonte = f.read()
            if "HEARTBEAT_FILE" not in fonte:
                erros.append(
                    "✗ opt-in: as sondas do 60-ai-gpu-worker leem $HEARTBEAT_FILE e o "
                    "ai-worker/transcribe_worker.py não lê essa variável — a sonda mediria "
                    "um ficheiro que ninguém toca, e o pod reiniciava a cada ~12 min"
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
