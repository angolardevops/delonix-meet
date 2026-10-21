# ADR-0024 — Produção do Ollama no Meet

**Estado:** Aceite · **Data:** 2026-10-09 · **Decisor:** o dono do produto

## Contexto

O Ollama já está deployado como parte do caminho de produção do Meet
(`deploy/k8s/07-ollama.yaml`, incluído na `kustomization.yaml` base — aplica-se
sempre que `deploy/k8s` se aplica), e já ligado ao servidor: tradução em tempo
real das legendas e resumo da acta (MoM) no fecho da reunião, ambos em
`server/src/ai.rs`, via `OLLAMA_URL`/`OLLAMA_MODEL_*`.

O cluster de produção (ADR-0020, VMs no Proxmox `192.168.1.10`) já está de pé
e a correr. Duas lacunas ficavam por fechar antes deste ADR:

1. A imagem do container estava fixada em `ollama/ollama:latest` — contra a
   doutrina de artefacto imutável do repo (nenhuma outra imagem externa em
   `deploy/k8s/` usa `:latest`; ver `coturn/coturn:4.6`, `postgres:16-alpine`,
   `kamailio/kamailio:5.8.6-bookworm`). Um `:latest` muda de conteúdo sem
   aviso nenhum — o mesmo manifesto aplicado duas vezes pode arrancar duas
   imagens diferentes.
2. A GPU tinha um caminho imperativo: `deploy/ansible/roles/k8s_app/tasks/main.yml`
   corria um `kubectl patch` condicional (`when: ollama_gpu`) DEPOIS do
   `kubectl apply`, acrescentando `nvidia.com/gpu` ao Deployment já aplicado.
   Isto não é GitOps — o patch não fica no manifesto declarado, e um
   `kubectl apply -k` seguinte não sabe que ele existe. A variável
   `ollama_gpu` nunca chegou a ficar documentada em `deploy/config.example.yml`.

## Decisão

**O Ollama fica um Deployment leve DENTRO do cluster do próprio Meet** — não
um serviço gerido à parte, nem partilhado com outros produtos do workspace
`ngolacloud`. A mesma regra do ADR-0020: o Meet não partilha substrato com o
resto do `ngolacloud`.

**A imagem fica fixada por versão.** `ollama/ollama:0.40.2` substitui
`ollama/ollama:latest` em `deploy/k8s/07-ollama.yaml`. 0.40.2 é a última
versão estável confirmada a 2026-10-09. Sem pinagem por digest (`@sha256:`):
nenhuma outra imagem externa deste repo a usa em `deploy/k8s/` — é uma lacuna
conhecida e documentada (`docs/convergencia-2026-10-08-k8s.md`: «falta nos
dois [k8s/ e o chart]; há `make pin` para as versões, não para digests»), e
este ADR segue o padrão existente em vez de o inventar.

**GPU: autorizada por orçamento a 2026-10-09; SEM hardware físico
identificado ainda.** O caminho fica preparado e desligado:
`deploy/k8s-overlays/components/ollama-gpu/` é um componente kustomize
(`nodeSelector: {nvidia.com/gpu.present: "true"}`, toleration para o taint
`nvidia.com/gpu:NoSchedule`, `runtimeClassName: nvidia`,
`resources.limits."nvidia.com/gpu": 1`), no mesmo padrão já usado pelo worker
de Whisper em GPU (`deploy/k8s/60-ai-gpu-worker.yaml`). Não entra em nenhum
overlay por omissão — nem a base `deploy/k8s/`, nem `saas`, nem `enterprise`.
Quando a GPU chegar, ligá-lo é **uma linha** em `components:` na
`kustomization.yaml` do ambiente, não um `kubectl patch` à mão. O `kubectl
patch` imperativo e a variável Ansible `ollama_gpu` foram removidos — não
tinham efeito nenhum sem hardware, e ficavam como um segundo caminho de
activação a divergir do manifesto declarado.

Isto é uma decisão explícita a registar, não um detalhe de implementação: a
GPU está aprovada, o dinheiro existe, só falta a máquina.

## O que isto NÃO decide

- **Dimensionamento de produção sob carga real.** Os `requests`/`limits`
  actuais em `07-ollama.yaml` (`250m`/`2Gi` a `4`/`8Gi`) nunca foram medidos
  contra tráfego real de tradução ao vivo + resumo de acta em simultâneo.
- **Qual modelo corre na GPU, quando ela chegar.** Hoje só há
  `qwen2.5:1.5b` e `qwen2.5:7b`, escolhidos para CPU (tamanho pequeno,
  quantização que corre sem acelerador). Um modelo maior ou diferente para
  GPU é uma decisão separada, posterior à chegada do hardware.
- **O número de réplicas ou HA do Ollama.** Continua `replicas: 1`,
  `strategy: Recreate` (o PVC de modelos é RWO) — sem alta disponibilidade.
- **Pinagem por digest (`@sha256:`) de nenhuma imagem deste repo.** A lacuna
  fica como estava, documentada em `docs/convergencia-2026-10-08-k8s.md`.

## Consequências

- `deploy/k8s/07-ollama.yaml` deixa de arrastar uma imagem que muda de
  conteúdo sem aviso — subir a versão passa a ser um diff revisável, não um
  `docker pull` silencioso no próximo rollout.
- `deploy/ansible/roles/k8s_app/tasks/main.yml` perde um `kubectl patch`
  imperativo e a lógica condicional que o rodeava; `deploy/ansible/group_vars/all.yml`
  perde a variável `ollama_gpu`, morta desde que nunca teve hardware para
  activar a sério.
- Activar a GPU no dia em que o hardware existir é uma mudança revisável de
  UMA linha numa `kustomization.yaml`, não um comando corrido à mão num
  cluster de produção — e fica coberta pelo mesmo `scripts/check-k8s-render.sh`
  que já cobre os outros overlays.
- **Nada disto foi validado contra GPU real.** Não há hardware no cluster de
  produção para testar o componente `ollama-gpu` — o `nodeSelector`, a
  `toleration` e o `runtimeClassName: nvidia` foram copiados do padrão já em
  produção para o worker de Whisper, mas o Ollama em si nunca correu com
  acelerador. A primeira activação é que vai confirmar ou corrigir isto.
