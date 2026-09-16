# Delonix Meet — Kubernetes Deployment

Este diretório contém os manifestos e configurações para realizar o deploy da plataforma **Delonix Meet** em um cluster Kubernetes, garantindo alta disponibilidade (HA) e escalabilidade horizontal.

## Estrutura dos Manifestos

*   `00-namespace.yaml`: Define o namespace isolado `delonix-meet`.
*   `01-config.yaml`: Contém o `ConfigMap` e o `Secret` para injetar configurações de ambiente (ex: credenciais da base de dados, REDIS_URL).
*   `02-server.yaml`: Deployment do backend Rust (`delonix-server`) com 3 réplicas, health checks e limites de recursos bem definidos.
*   `03-web.yaml`: Deployment do frontend React (servido via Nginx) com 3 réplicas e alocação leve de recursos.
*   `04-ingress.yaml`: Configuração do Nginx Ingress Controller, com anotações específicas para suportar a atualização para WebSockets (`/ws`, `/rtc`).
*   `helm-values/`: Configurações personalizadas para o deploy das bases de dados em High Availability através de Helm.

## Edições (saas, enterprise) — overlays

Esta pasta é a **base** e continua a ser o que `make stage`/`make prod` aplicam,
sem alteração. As edições do ADR-0005 são overlays kustomize **ao lado**, em
[`../k8s-overlays/`](../k8s-overlays/):

*   `k8s-overlays/saas`: `DELONIX_EDITION=saas`, Job de migração (`args: [migrate]`), Redis obrigatório, HPA.
*   `k8s-overlays/enterprise`: `DELONIX_EDITION=enterprise`, `REGISTRATION_MODE=invite`, `TENANCY_MODE=single`, uma réplica.
*   `k8s-overlays/components/edition-common`: Service `delonix-server-internal` (8181 interno + 9180 gRPC, ClusterIP, **nunca** num Ingress), NetworkPolicy, mTLS por cert-manager, `startupProbe`, rootfs só-de-leitura, `LOG_FORMAT=json`.

Não estão em `deploy/k8s/overlays/` porque o kustomize recusa um overlay dentro
da própria base («cycle detected»). Portão: `bash scripts/check-k8s-render.sh`
(renderiza a base e os dois overlays, e falha se uma porta interna chegar a um
Ingress ou se o `/ws` perder a afinidade do ADR-0001).

## Alta Disponibilidade (HA) de Estado e Dados

Para cenários de produção, não recomendamos StatefulSets isolados. O `Makefile` recorre aos *charts* oficiais da Bitnami para instanciar:
1.  **PostgreSQL HA:** Uma arquitetura primário-secundário utilizando Repmgr.
2.  **Redis Sentinel:** Utilizado pelo barramento de sinalização distribuído do Delonix Meet para sincronizar estado WebRTC entre diferentes nós em tempo real.

## Comandos

Os comandos estão integrados na raiz do projeto:

*   `make stage`: Sobe um cluster Kubernetes local via `kind`, instala o Nginx Ingress Controller e aplica todos os manifestos (ideal para CI/CD ou testes robustos locais).
*   `make prod`: Aplica os manifestos no contexto Kubernetes atualmente ativo na sua máquina (`kubectl config current-context`).

> O `make dev` mantém-se inalterado, correndo a stack via `docker-compose` para o fluxo rápido de desenvolvimento.
