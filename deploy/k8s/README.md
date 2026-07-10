# Delonix Meet — Kubernetes Deployment

Este diretório contém os manifestos e configurações para realizar o deploy da plataforma **Delonix Meet** em um cluster Kubernetes, garantindo alta disponibilidade (HA) e escalabilidade horizontal.

## Estrutura dos Manifestos

*   `00-namespace.yaml`: Define o namespace isolado `delonix-meet`.
*   `01-config.yaml`: Contém o `ConfigMap` e o `Secret` para injetar configurações de ambiente (ex: credenciais da base de dados, REDIS_URL).
*   `02-server.yaml`: Deployment do backend Rust (`delonix-server`) com 3 réplicas, health checks e limites de recursos bem definidos.
*   `03-web.yaml`: Deployment do frontend React (servido via Nginx) com 3 réplicas e alocação leve de recursos.
*   `04-ingress.yaml`: Configuração do Nginx Ingress Controller, com anotações específicas para suportar a atualização para WebSockets (`/ws`, `/rtc`).
*   `helm-values/`: Configurações personalizadas para o deploy das bases de dados em High Availability através de Helm.

## Alta Disponibilidade (HA) de Estado e Dados

Para cenários de produção, não recomendamos StatefulSets isolados. O `Makefile` recorre aos *charts* oficiais da Bitnami para instanciar:
1.  **PostgreSQL HA:** Uma arquitetura primário-secundário utilizando Repmgr.
2.  **Redis Sentinel:** Utilizado pelo barramento de sinalização distribuído do Delonix Meet para sincronizar estado WebRTC entre diferentes nós em tempo real.

## Comandos

Os comandos estão integrados na raiz do projeto:

*   `make stage`: Sobe um cluster Kubernetes local via `kind`, instala o Nginx Ingress Controller e aplica todos os manifestos (ideal para CI/CD ou testes robustos locais).
*   `make prod`: Aplica os manifestos no contexto Kubernetes atualmente ativo na sua máquina (`kubectl config current-context`).

> O `make dev` mantém-se inalterado, correndo a stack via `docker-compose` para o fluxo rápido de desenvolvimento.
