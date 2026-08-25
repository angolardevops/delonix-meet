# ============================================================
#  Delonix Meet — orquestração de ambientes (dev / prod)
#
#  Um comando por ambiente, pronto a usar:
#     make dev     → sobe infra + backend + frontend (dev), imprime URLs
#     make prod    → deploy de produção (segredos + build + publish + smoke)
#
#  `make` (sem alvo) ou `make help` lista tudo.
# ============================================================
SHELL := /bin/bash
.DEFAULT_GOAL := help

# ---- Configuração (override: make dev NODE_BIN=/caminho) ----
ROOT       := $(shell pwd)
# Auto-detecta o bin do Node instalado via nvm (versão mais recente);
# override com: make dev NODE_BIN=/caminho/para/node/bin
_NVM_BIN   := $(shell ls -d "$(HOME)/.nvm/versions/node"/v*/bin 2>/dev/null | tail -1)
NODE_BIN   ?= $(or $(_NVM_BIN),/home/walter/.nvm/versions/node/v25.0.0/bin)
ENV_FILE   ?= /etc/delonix/delonix.env
API_URL    ?= http://127.0.0.1:8180
WEB_PORT   ?= 5173
RUNDIR     := $(ROOT)/.dev
# Segredo partilhado da API interna de voz (IVR). O MESMO valor tem de ser usado
# pelo backend E pela camada de media (FreeSWITCH), senão a auth do IVR falha.
VOICE_SECRET ?= dev-voice-secret-abc123
# Certificados TLS/SRTP da voz (dev: self-signed no repo, gitignored).
VOICE_TLS_DIR ?= $(ROOT)/voice/tls
export PATH := $(NODE_BIN):$(PATH)

# ---- Kubernetes / kind ----
KIND_CLUSTER      ?= delonix-stage
# Versionamento de imagens: tag derivada do git (ex.: v1.0.0-31-g89689ca).
# Cada `make image-push` gera uma tag NOVA e faz pin nos Deployments
# (kubectl set image) → rollouts deterministas, sem o problema do :latest
# stale. Override: make image-push IMAGE_TAG=v1.1.0
IMAGE_TAG         ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo dev)
IMAGE_SERVER_REPO ?= delonix-server
IMAGE_WEB_REPO    ?= delonix-web
IMAGE_SERVER      := $(IMAGE_SERVER_REPO):$(IMAGE_TAG)
IMAGE_WEB         := $(IMAGE_WEB_REPO):$(IMAGE_TAG)
METALLB_VERSION   ?= v0.14.9

# Cores
C := \033[1;36m
G := \033[1;32m
Y := \033[1;33m
Z := \033[0m

.PHONY: help
help: ## Mostra esta ajuda
	@printf "$(C)Delonix Meet — Makefile$(Z)\n\n"
	@grep -hE '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
	  | awk 'BEGIN{FS=":.*?## "}{printf "  $(G)%-16s$(Z) %s\n", $$1, $$2}'
	@printf "\n  Dev:  $(Y)make dev$(Z)   ·  Prod: $(Y)make prod$(Z)   ·  Parar dev: $(Y)make stop$(Z)\n"

# ============================================================
#  DEV — ambiente completo pronto a usar
# ============================================================
.PHONY: dev
dev: infra api-bg web-bg ## Sobe infra + backend + frontend (dev) e imprime URLs
	@if grep -q 'meet\.delonix\.local' /etc/hosts; then \
	  sudo sed -i 's/.*meet\.delonix\.local.*/127.0.0.1 meet.delonix.local/' /etc/hosts; \
	else \
	  echo "127.0.0.1 meet.delonix.local" | sudo tee -a /etc/hosts > /dev/null; \
	fi
	@printf "\n$(G)✔ Ambiente de DEV pronto.$(Z)\n"
	@printf "   API:      $(API_URL)\n"
	@printf "   App:      $(G)http://localhost:$(WEB_PORT)$(Z)  ← abre AQUI (câmara/mic funcionam em localhost)\n"
	@printf "   HTTPS:    $(G)https://meet.delonix.local$(Z)  (nginx local → 127.0.0.1:8180)\n"
	@printf "   $(Y)Câmara por IP na rede$(Z) exige HTTPS: usa o Nginx ($(Y)make prod$(Z) → https://<ip>) ou $(Y)make web-https$(Z).\n"
	@printf "   Logs:  $(Y)make logs$(Z)   ·   Parar:  $(Y)make stop$(Z)\n"

.PHONY: infra
infra: ## Sobe Postgres/Redis/coturn (docker compose) e espera healthy
	@printf "$(C)▶ infra (docker compose up --wait)$(Z)\n"
	@docker compose up -d --wait

.PHONY: api-bg
api-bg: ## Compila e arranca o backend em dev (via systemd se existir; senão detach)
	@printf "$(C)▶ backend (dev)$(Z)\n"
	@mkdir -p $(RUNDIR)
	@cd server && cargo build --release
	@if systemctl --user cat delonix-server >/dev/null 2>&1; then \
	  systemctl --user restart delonix-server && printf "  (via systemd)\n"; \
	else \
	  pkill -f '[d]elonix-server' 2>/dev/null || true; \
	  ( cd server && DELONIX_ALLOW_INSECURE=1 VOICE_INTERNAL_SECRET=$(VOICE_SECRET) \
	    setsid ./target/release/delonix-server > $(RUNDIR)/api.log 2>&1 < /dev/null & echo $$! > $(RUNDIR)/api.pid ); \
	fi
	@for i in $$(seq 1 20); do sleep 1; \
	  [ "$$(curl -s -o /dev/null -w '%{http_code}' $(API_URL)/health 2>/dev/null)" = "200" ] && break; \
	  [ $$i = 20 ] && { printf "$(Y)  ✗ backend não respondeu (ver: journalctl --user -u delonix-server -e  ou  make logs-api)$(Z)\n"; exit 1; }; done
	@printf "$(G)  ✓ backend a correr ($(API_URL))$(Z)\n"

.PHONY: web-bg
web-bg: ## Arranca o Vite dev (HMR) em background — HTTP em localhost (contexto seguro)
	@printf "$(C)▶ frontend (vite dev, background)$(Z)\n"
	@mkdir -p $(RUNDIR)
	@pkill -f '[d]elonix-meet/web.*vite' 2>/dev/null || true; pkill -f '[n]ode.*vite' 2>/dev/null || true
	@cd web && [ -d node_modules ] || npm ci
	@# HTTP em localhost = já é contexto seguro → câmara/mic funcionam sem cert.
	@# Para câmara por IP na rede, usar o Nginx HTTPS (make prod) ou 'make web-https'.
	@cd web && NO_HTTPS=1 PORT=$(WEB_PORT) setsid npm run dev > $(RUNDIR)/web.log 2>&1 < /dev/null & echo $$! > $(RUNDIR)/web.pid
	@for i in $$(seq 1 20); do sleep 1; \
	  curl -s -o /dev/null "http://localhost:$(WEB_PORT)" 2>/dev/null && break; \
	  [ $$i = 20 ] && { printf "$(Y)  ✗ frontend não respondeu (ver: make logs-web)$(Z)\n"; exit 1; }; done
	@printf "$(G)  ✓ frontend em http://localhost:$(WEB_PORT)  (câmara OK em localhost)$(Z)\n"

.PHONY: web-https
web-https: ## Vite dev em HTTPS (basic-ssl) — para acesso por IP na rede com câmara
	@cd web && [ -d node_modules ] || npm ci; PORT=$(WEB_PORT) npm run dev

.PHONY: api
api: infra ## Backend em FOREGROUND (dev) — Ctrl-C para parar
	@cd server && DELONIX_ALLOW_INSECURE=1 VOICE_INTERNAL_SECRET=$(VOICE_SECRET) cargo run

.PHONY: web
web: ## Frontend em FOREGROUND (vite HMR, HTTP localhost) — Ctrl-C para parar
	@cd web && [ -d node_modules ] || npm ci; NO_HTTPS=1 PORT=$(WEB_PORT) npm run dev

.PHONY: stop
stop: ## Para o backend + frontend de dev (mantém a infra)
	@printf "$(C)▶ a parar dev$(Z)\n"
	@# Usa o pid gravado ao arrancar; fallback para pkill com bracket trick (não se auto-mata).
	@if [ -f $(RUNDIR)/api.pid ]; then kill $$(cat $(RUNDIR)/api.pid) 2>/dev/null || true; rm -f $(RUNDIR)/api.pid; \
	else pkill -f '[d]elonix-server' 2>/dev/null || true; fi
	@if [ -f $(RUNDIR)/web.pid ]; then kill $$(cat $(RUNDIR)/web.pid) 2>/dev/null || true; rm -f $(RUNDIR)/web.pid; fi
	@pkill -f '[n]ode.*vite' 2>/dev/null || true
	@printf "$(G)  ✓ parado (infra continua; 'make down' para a infra também)$(Z)\n"

.PHONY: down
down: stop ## Para TODA a stack Delonix: dev (processos + docker compose + voice)
	@printf "$(C)▶ docker compose down (dev infra)$(Z)\n"
	@docker compose down
	@printf "$(C)▶ docker compose down (voice, se ativo)$(Z)\n"
	@docker compose -f voice/docker-compose.voice.yml down 2>/dev/null || true
	@printf "$(G)  ✓ stack completa parada (kind continua; 'make destroy' para o k8s)$(Z)\n"

.PHONY: kill
kill: ## Para TUDO (processos locais + docker + k8s) — estado zero até 'make dev' ou 'make stage'
	@printf "$(C)▶ a parar processos locais (backend + frontend)...$(Z)\n"
	@if [ -f $(RUNDIR)/api.pid ]; then kill $$(cat $(RUNDIR)/api.pid) 2>/dev/null || true; rm -f $(RUNDIR)/api.pid; fi
	@if [ -f $(RUNDIR)/web.pid ]; then kill $$(cat $(RUNDIR)/web.pid) 2>/dev/null || true; rm -f $(RUNDIR)/web.pid; fi
	@pkill -f 'target/release/delonix-server' 2>/dev/null || true
	@sleep 1 && pkill -9 -f 'target/release/delonix-server' 2>/dev/null || true
	@pkill -f '[n]ode.*vite\|[v]ite.*delonix' 2>/dev/null || true
	@printf "$(C)▶ a parar docker compose (infra dev + voice)...$(Z)\n"
	@docker compose down 2>/dev/null || true
	@docker compose -f voice/docker-compose.voice.yml down 2>/dev/null || true
	@printf "$(C)▶ a escalar workloads k8s para 0 (namespace delonix-meet)...$(Z)\n"
	@kubectl scale deployment --all -n delonix-meet --replicas=0 2>/dev/null || true
	@kubectl scale statefulset --all -n delonix-meet --replicas=0 2>/dev/null || true
	@printf "$(G)  ✓ TUDO parado. Nada corre até fazer 'make dev' (local) ou 'make stage' (k8s).$(Z)\n"

.PHONY: logs logs-api logs-web
logs: ## Segue os logs do backend + frontend (dev)
	@mkdir -p $(RUNDIR) && touch $(RUNDIR)/api.log $(RUNDIR)/web.log
	@tail -n 40 -f $(RUNDIR)/api.log $(RUNDIR)/web.log
logs-api: ; @mkdir -p $(RUNDIR) && touch $(RUNDIR)/api.log && tail -n 60 -f $(RUNDIR)/api.log
logs-web: ; @mkdir -p $(RUNDIR) && touch $(RUNDIR)/web.log && tail -n 60 -f $(RUNDIR)/web.log

# ============================================================
#  BUILD / TEST / MIGRATE
# ============================================================
.PHONY: build
build: ## Compila backend (release) + frontend (produção)
	@printf "$(C)▶ build backend + frontend$(Z)\n"
	@cd server && cargo build --release
	@cd web && npm ci && npm run build
	@printf "$(G)  ✓ build concluído$(Z)\n"

.PHONY: test
test: fitness web-deps ## Corre os testes (fitness functions + cargo test + typecheck do frontend)
	@printf "$(C)▶ testes$(Z)\n"
	@cd server && cargo test --release
	@cd web && node_modules/.bin/tsc -p tsconfig.json --noEmit && printf "$(G)  ✓ tsc limpo$(Z)\n"
	@cd web && node_modules/.bin/vitest run && printf "$(G)  ✓ vitest (R1/R2)$(Z)\n"

.PHONY: web-deps
web-deps: ## Garante web/node_modules (npm ci) — sem isto o `make test` morria com um 'Error 127' opaco
	@if [ ! -x web/node_modules/.bin/tsc ]; then \
	  printf "$(C)▶ web/node_modules ausente — npm ci$(Z)\n"; \
	  cd web && npm ci; \
	fi

.PHONY: fitness
fitness: ## Fitness functions: formatação, higiene, AUTORIZAÇÃO DE ROTAS, docs, afinidade (R3), clippy, deps, RLS
	@# `fmt --check` AQUI e não só no CI: sem ele, uma alteração formatada a
	@# meio passa o `make test` local e só falha no CI, depois de um push e de
	@# vários minutos de espera. O portão local tem de ser o mesmo do remoto.
	@cd server && cargo fmt --check && printf "$(G)  ✓ formatação Rust$(Z)\n"
	@bash scripts/check-repo-hygiene.sh
	@bash scripts/check-route-auth.sh
	@bash scripts/check-docs-drift.sh
	@bash scripts/check-room-affinity.sh
	@bash scripts/check-clippy-ratchet.sh
	@bash scripts/check-dep-audit.sh
	@bash scripts/check-tenant-rls.sh

.PHONY: migrate
migrate: ## Corre as migrações pendentes (sqlx migrate run)
	@printf "$(C)▶ migrações$(Z)\n"
	@cd server && cargo sqlx migrate run
	@printf "$(G)  ✓ migrações aplicadas$(Z)\n"

# ============================================================
#  IMAGENS DOCKER — build local + carregamento no kind
# ============================================================

# MOTOR DE BUILD — delonix, com docker como alternativa.
#   Esta plataforma NÃO tem docker por princípio: o delonix-runtime é
#   daemonless e rootless, e a regra do workspace é nunca depender de um socket
#   docker global (ver HARNESS.md da raiz do ngolacloud). O `delonix build`
#   aceita os mesmos flags (-f/-t/--build-arg/--no-cache), por isso a troca é
#   directa; quem tiver um ambiente clássico com docker continua a funcionar.
BUILDER ?= $(shell command -v delonix >/dev/null 2>&1 && echo delonix || echo docker)
ifeq ($(BUILDER),delonix)
  IMG_BUILD := delonix build
  IMG_TAG_CMD := delonix image tag
  IMG_LS    := delonix image ls
  IMG_PULL  := delonix image pull
  # Carregar imagem no cluster SEM registo: `delonix cluster load` empacota a
  # imagem do store local e importa-a no containerd de cada nó — o equivalente
  # exacto do `kind load docker-image`, mas sem o binário `kind` (que é um
  # cliente docker e exigiria um provider Docker/Podman que esta máquina não
  # tem por princípio). Requer delonix >= v0.35.0.
  IMG_LOAD  := delonix cluster load
  # `delonix image save` exige `-o` (docker/podman escrevem em stdout por
  # omissão) — daí a forma `<cmd> <imagem> -o <ficheiro>` no export-images.
  IMG_SAVE  := delonix image save
  # Um nó kind precisa de delegação de cgroup2 — sem o scope o kubelet arranca
  # em loop e o cluster nunca fica Ready (a mensagem do próprio `cluster create`
  # avisa disso).
  CLUSTER_CREATE := systemd-run --user --scope -q -p Delegate=yes delonix cluster create
  # O kubeconfig do cluster kind-mode: usado só se existir, para o kubectl deste
  # Makefile apontar ao cluster certo sem depender do ~/.kube/config ambiente.
  DLX_KUBECONFIG := $(HOME)/.local/share/delonix/clusters/$(KIND_CLUSTER)-kubeconfig.yaml
  ifneq ($(wildcard $(DLX_KUBECONFIG)),)
    export KUBECONFIG := $(DLX_KUBECONFIG)
  endif
else
  IMG_BUILD := docker build
  IMG_TAG_CMD := docker tag
  IMG_LS    := docker images
  IMG_PULL  := docker pull
  IMG_LOAD  := kind load docker-image
  IMG_SAVE  := docker save
  CLUSTER_CREATE := kind create cluster
endif

# make image   → constrói delonix-server:latest e delonix-web:latest
# make push    → carrega as imagens no cluster ($(IMG_LOAD))
# make image-push → build + load (o que é preciso antes de make stage)
#
# Porquê carregar em vez de registry?
#   O cluster corre sem acesso ao Docker Hub (offline / rate-limit). Carregar
#   injeta a imagem diretamente no containerd do nó, sem registry externo.
#   Com delonix: `cluster load` (v0.35.0+); com docker: `kind load docker-image`.

.PHONY: image
image: ## Constrói delonix-server e delonix-web com a tag versionada ($(IMAGE_TAG))
	@printf "$(C)▶ build $(IMAGE_SERVER) (Rust — pode demorar ~10 min sem cache)$(Z)\n"
	@# SEMPRE rebuild do frontend: o dist tem de refletir o código atual
	@# (um dist stale foi a causa de "estilos perdidos" em stage — nunca reusar).
	@export PATH="$(NODE_BIN):$$PATH"; \
	  cd web && { [ -d node_modules ] || npm ci; } && npm run build
	@$(IMG_BUILD) -f Dockerfile.server -t $(IMAGE_SERVER) .
	@printf "$(C)▶ build $(IMAGE_WEB) (dist local → nginx, rápido)$(Z)\n"
	@$(IMG_BUILD) -f Dockerfile.web.stage -t $(IMAGE_WEB) .
	@# :latest acompanha a última build (bootstrap dos manifests em cluster novo).
	@$(IMG_TAG_CMD) $(IMAGE_SERVER) $(IMAGE_SERVER_REPO):latest
	@$(IMG_TAG_CMD) $(IMAGE_WEB) $(IMAGE_WEB_REPO):latest
	@printf "$(G)  ✓ imagens prontas: tag $(Y)$(IMAGE_TAG)$(Z)$(G) (+latest)$(Z)\n"
	@$(IMG_LS) 2>/dev/null | grep -E "delonix-(server|web)" || true

.PHONY: push
push: ## load das imagens no cluster + PIN da tag versionada nos Deployments
	@printf "$(C)▶ load $(IMAGE_SERVER) + $(IMAGE_WEB) → $(KIND_CLUSTER)$(Z)\n"
	@$(IMG_LOAD) $(IMAGE_SERVER) --name $(KIND_CLUSTER)
	@$(IMG_LOAD) $(IMAGE_WEB) --name $(KIND_CLUSTER)
	@$(MAKE) --no-print-directory pin

.PHONY: pin
pin: ## Fixa a tag $(IMAGE_TAG) nos Deployments e espera o rollout
	@printf "$(C)▶ pin das imagens nos Deployments (tag $(IMAGE_TAG))$(Z)\n"
	@kubectl -n delonix-meet set image deployment/delonix-server server=$(IMAGE_SERVER)
	@kubectl -n delonix-meet set image deployment/delonix-web web=$(IMAGE_WEB)
	@kubectl -n delonix-meet rollout status deployment/delonix-server --timeout=180s
	@kubectl -n delonix-meet rollout status deployment/delonix-web --timeout=120s
	@printf "$(G)  ✓ cluster a correr $(IMAGE_SERVER) / $(IMAGE_WEB)$(Z)\n"

.PHONY: image-push
image-push: image push ## Build versionado + load no cluster + pin (pipeline completo p/ stage k8s)

# Pré-puxa imagens da infra (Bitnami Postgres/Redis) do Docker Hub e
# injeta-as no kind. Resolve o ImagePullBackOff quando o cluster não
# tem acesso direto ao Docker Hub (ambiente offline ou rate-limited).
#
# Nota: usa bitnami/postgresql (single-node) e bitnami/redis standalone
# para stage/kind porque o chart postgresql-HA (pgpool + postgresql-repmgr)
# removeu as suas imagens do Docker Hub em 2024 para o OCI registry privado.
.PHONY: infra-pull
infra-pull: ## Pré-carrega imagens Bitnami (Postgres single/Redis standalone) no cluster
	@printf "$(C)▶ a extrair imagens da infra (charts de stage)...$(Z)\n"
	@IMGS=$$(helm template delonix-postgres bitnami/postgresql \
	    -f deploy/k8s/helm-values/postgres-stage-values.yaml -n delonix-meet 2>/dev/null \
	    | grep -E '^\s+image:' | awk '{gsub(/"/, "", $$2); print $$2}' | sort -u); \
	 IMGS="$$IMGS $$(helm template delonix-redis bitnami/redis \
	    -f deploy/k8s/helm-values/redis-stage-values.yaml -n delonix-meet 2>/dev/null \
	    | grep -E '^\s+image:' | awk '{gsub(/"/, "", $$2); print $$2}' | sort -u)"; \
	 for img in $$IMGS; do \
	   [ -z "$$img" ] && continue; \
	   printf "  ▷ $$img\n"; \
	   $(IMG_PULL) "$$img" 2>/dev/null \
	     || printf "  $(Y)  ! pull falhou — sem acesso ao registo para $$img$(Z)\n"; \
	   $(IMG_LOAD) "$$img" --name $(KIND_CLUSTER) 2>/dev/null || true; \
	 done
	@printf "$(G)  ✓ imagens da infra carregadas no kind$(Z)\n"

# ============================================================
#  METALLB — Load Balancer bare-metal (kind auto-detect | kubeadm estático)
# ============================================================

# Para kind: deteta automaticamente a subnet da rede docker 'kind'
# (ex: 172.30.0.0/24 → pool 172.30.0.200-172.30.0.250).
# Para kubeadm/prod: o range estático é passado pelo Ansible (inventory.ini).
.PHONY: metallb-kind
metallb-kind: ## Instala MetalLB no kind e cria pool com IPs da rede docker kind (auto-detect)
	@printf "$(C)▶ MetalLB $(METALLB_VERSION) → kind cluster '$(KIND_CLUSTER)'$(Z)\n"
	@helm repo add metallb https://metallb.github.io/metallb 2>/dev/null || true
	@helm repo update metallb 2>/dev/null | tail -1
	@helm upgrade --install metallb metallb/metallb \
	  -n metallb-system --create-namespace \
	  --set speaker.frr.enabled=false \
	  --version $(METALLB_VERSION) \
	  --wait --timeout=120s
	@printf "$(C)▶ a detetar subnet IPv4 da rede docker 'kind'...$(Z)\n"
	@SUBNET=$$(docker network inspect kind 2>/dev/null \
	    | grep -E '"Subnet":[[:space:]]*"[0-9]' | head -1 \
	    | awk -F'"' '{print $$4}'); \
	 [ -z "$$SUBNET" ] && { printf "$(Y)  ✗ rede docker 'kind' não encontrada — cria o cluster primeiro (make stage)$(Z)\n"; exit 1; }; \
	 BASE=$$(echo "$$SUBNET" | cut -d'.' -f1-3); \
	 RANGE="$${BASE}.200-$${BASE}.250"; \
	 printf "   subnet: $$SUBNET  →  pool MetalLB: $$RANGE\n"; \
	 sed "s|METALLB_RANGE|$$RANGE|g" deploy/k8s/metallb-pool.yaml | kubectl apply -f -
	@printf "$(C)▶ a converter ingress-nginx-controller para LoadBalancer...$(Z)\n"
	@kubectl patch svc ingress-nginx-controller -n ingress-nginx \
	  -p '{"spec":{"type":"LoadBalancer"}}' 2>/dev/null || true
	@printf "$(G)  ✓ MetalLB pronto. IP externo do ingress:$(Z)\n"
	@kubectl get svc ingress-nginx-controller -n ingress-nginx \
	  --no-headers -o custom-columns="IP:status.loadBalancer.ingress[0].ip" 2>/dev/null || true
	@printf "   Adicionar ao /etc/hosts: $(Y)<IP-acima>  meet.delonix.local$(Z)\n"

# ============================================================
#  KUBERNETES STAGE & PROD
# ============================================================
.PHONY: stage
stage: image-push ## Build + kind load + deploy k8s completo no cluster kind local
	@printf "$(C)▶ Criando cluster '$(KIND_CLUSTER)' (idempotente)...$(Z)\n"
	@$(CLUSTER_CREATE) --name $(KIND_CLUSTER) 2>/dev/null || true
	@printf "$(C)▶ Instalando NGINX Ingress Controller...$(Z)\n"
	@kubectl apply -f https://raw.githubusercontent.com/kubernetes/ingress-nginx/main/deploy/static/provider/kind/deploy.yaml
	@kubectl wait --namespace ingress-nginx \
	  --for=condition=ready pod \
	  --selector=app.kubernetes.io/component=controller \
	  --timeout=90s
	@printf "$(C)▶ MetalLB (LoadBalancer bare-metal para kind)...$(Z)\n"
	@$(MAKE) --no-print-directory metallb-kind
	@printf "$(C)▶ Adicionando repositórios Helm (Bitnami)...$(Z)\n"
	@helm repo add bitnami https://charts.bitnami.com/bitnami 2>/dev/null || true
	@helm repo update
	@printf "$(C)▶ Pré-carregando imagens da infra no kind (evita ImagePullBackOff)...$(Z)\n"
	@$(MAKE) --no-print-directory infra-pull
	@printf "$(C)▶ Namespace + Secret TLS...$(Z)\n"
	@$(MAKE) --no-print-directory certs
	@kubectl apply -f deploy/k8s/00-namespace.yaml
	@kubectl create secret tls delonix-tls-secret \
	  --cert=deploy/certs/wildcard.delonix.local.crt \
	  --key=deploy/certs/wildcard.delonix.local.key \
	  -n delonix-meet --dry-run=client -o yaml | kubectl apply -f -
	@printf "$(C)▶ Helm: Postgres (single-node) + Redis (standalone)...$(Z)\n"
	@# Usa bitnami/postgresql em vez de postgresql-ha: o chart HA (pgpool +
	@# postgresql-repmgr) removeu as imagens do Docker Hub em 2024; o chart
	@# simples continua acessível via registry-1.docker.io. Para prod usa-se
	@# postgresql-ha (make prod) com acesso ao OCI registry da Bitnami.
	@helm upgrade --install delonix-postgres bitnami/postgresql \
	  -f deploy/k8s/helm-values/postgres-stage-values.yaml -n delonix-meet
	@helm upgrade --install delonix-redis bitnami/redis \
	  -f deploy/k8s/helm-values/redis-stage-values.yaml -n delonix-meet
	@printf "$(C)▶ Aplicação Delonix (config + server + web + ingress + coturn)...$(Z)\n"
	@kubectl apply -f deploy/k8s/01-config.yaml
	@kubectl apply -f deploy/k8s/02-server.yaml
	@kubectl apply -f deploy/k8s/03-web.yaml
	@kubectl apply -f deploy/k8s/04-ingress.yaml
	@kubectl apply -f deploy/k8s/51-coturn.yaml   # media relay (R4) — imprescindível
	@# Os manifests referenciam :latest (bootstrap) — re-pina a tag versionada
	@# desta build, senão o apply desfazia o pin do image-push.
	@$(MAKE) --no-print-directory pin
	@printf "$(G)  ✓ Stage (Kind) pronto!$(Z)\n"
	@LB_IP=$$(kubectl get svc ingress-nginx-controller -n ingress-nginx \
	    --no-headers -o custom-columns="IP:status.loadBalancer.ingress[0].ip" 2>/dev/null | grep -v '<none>'); \
	  if [ -n "$$LB_IP" ]; then \
	    if grep -q 'meet\.delonix\.local' /etc/hosts; then \
	      sudo sed -i "s/.*meet\.delonix\.local.*/$$LB_IP meet.delonix.local/" /etc/hosts; \
	    else \
	      echo "$$LB_IP meet.delonix.local" | sudo tee -a /etc/hosts > /dev/null; \
	    fi; \
	    printf "   /etc/hosts atualizado: $(G)$$LB_IP meet.delonix.local$(Z)\n"; \
	  else \
	    printf "   $(Y)⚠ MetalLB ainda sem IP — adiciona manualmente ao /etc/hosts$(Z)\n"; \
	  fi
	@printf "   URL:  $(G)https://meet.delonix.local$(Z)\n"
	@printf "   Pods: $(Y)kubectl get po -n delonix-meet$(Z)\n"

DOMAIN ?= meet.delonix.local

.PHONY: prod
prod: ## Deploy de produção K8s (Ansible + Helm + Manifestos + Let's Encrypt)
	@printf "$(C)▶ Provisionando Cluster K8s Bare-Metal via Ansible...$(Z)\n"
	@ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/playbook.yml
	@printf "$(C)▶ Instalando cert-manager (Let's Encrypt)...$(Z)\n"
	@kubectl apply -f https://github.com/cert-manager/cert-manager/releases/download/v1.14.4/cert-manager.yaml
	@kubectl wait --namespace cert-manager --for=condition=ready pod --selector=app.kubernetes.io/instance=cert-manager --timeout=120s
	@kubectl apply -f deploy/k8s/05-cert-manager.yaml
	@printf "$(C)▶ Deploy da Infra (Namespace + Helm Postgres/Redis)...$(Z)\n"
	@kubectl apply -f deploy/k8s/00-namespace.yaml
	@helm repo add bitnami https://charts.bitnami.com/bitnami
	@helm repo update
	@helm upgrade --install delonix-postgres bitnami/postgresql-ha -f deploy/k8s/helm-values/postgres-values.yaml -n delonix-meet
	@helm upgrade --install delonix-redis bitnami/redis -f deploy/k8s/helm-values/redis-values.yaml -n delonix-meet
	@printf "$(C)▶ Compilando e gerando Docker Image (Distroless Security)...$(Z)\n"
	@docker build -t delonix-meet-server:latest -f Dockerfile.server .
	@printf "$(C)▶ Fazendo deploy da Aplicação com Domínio $(DOMAIN)...$(Z)\n"
	@kubectl apply -f deploy/k8s/01-config.yaml
	@kubectl apply -f deploy/k8s/02-server.yaml
	@kubectl apply -f deploy/k8s/03-web.yaml
	@sed "s/meet.delonix.local/$(DOMAIN)/g" deploy/k8s/04-ingress.yaml | kubectl apply -f -
	@kubectl apply -f deploy/k8s/51-coturn.yaml   # media relay (R4). Prod: trocar VIP metallb por LB de cloud + IP público em 01-config/51-coturn
	@printf "$(G)  ✓ Deploy de Produção concluído! O Cert-Manager obterá os certificados para $(DOMAIN).$(Z)\n"

.PHONY: destroy
destroy: ## Faz backup do etcd, postgres e redis e destrói o cluster
	@printf "$(C)▶ Iniciando o processo de destruição e backup do cluster...$(Z)\n"
	@mkdir -p backups
	@printf "$(C)  - A efetuar backup do PostgreSQL...$(Z)\n"
	@kubectl exec -n delonix-meet -it delonix-postgres-postgresql-ha-postgresql-0 -- pg_dump -U postgres delonix > backups/postgres_backup.sql || true
	@printf "$(C)  - A efetuar backup do Redis...$(Z)\n"
	@kubectl exec -n delonix-meet -it delonix-redis-master-0 -- redis-cli SAVE || true
	@kubectl cp delonix-meet/delonix-redis-master-0:/data/dump.rdb backups/redis_dump.rdb || true
	@printf "$(C)  - Destruindo Helm charts e Manifestos...$(Z)\n"
	@helm uninstall delonix-postgres -n delonix-meet || true
	@helm uninstall delonix-redis -n delonix-meet || true
	@kubectl delete namespace delonix-meet || true
	@kind delete cluster --name delonix-stage || true
	@printf "$(G)  ✓ Cluster destruído com sucesso. Backups em ./backups/$(Z)\n"

.PHONY: restore
restore: ## Reconstrói o cluster e faz o restore das bases de dados
	@printf "$(C)▶ Iniciando o processo de restauro do cluster...$(Z)\n"
	@$(MAKE) stage
	@printf "$(C)  - A restaurar PostgreSQL...$(Z)\n"
	@kubectl cp backups/postgres_backup.sql delonix-meet/delonix-postgres-postgresql-ha-postgresql-0:/tmp/backup.sql || true
	@kubectl exec -n delonix-meet -it delonix-postgres-postgresql-ha-postgresql-0 -- psql -U postgres -d delonix -f /tmp/backup.sql || true
	@printf "$(C)  - A restaurar Redis...$(Z)\n"
	@kubectl cp backups/redis_dump.rdb delonix-meet/delonix-redis-master-0:/data/dump.rdb || true
	@printf "$(G)  ✓ Cluster restaurado com sucesso.$(Z)\n"

.PHONY: prod-legacy
prod-legacy: ## Deploy bare-metal legado (systemd + nginx)
	@bash deploy/deploy.sh

# ============================================================
#  DEPLOY ZERO-TOUCH — single-host OU multi-host (Ansible + 12-factor)
#  IPs, DNS, TLS e segredos gerados sem intervenção humana.
#  Único ficheiro a editar: deploy/config.yml (ver deploy/config.example.yml).
#  Docs: docs/ops/zero-touch-deploy.md
# ============================================================
ANSIBLE_ARGS ?=

.PHONY: deploy
deploy: ## Deploy zero-touch (lê deploy/config.yml; single ou multi)
	@if [ ! -f deploy/config.yml ]; then \
	  cp deploy/config.example.yml deploy/config.yml; \
	  printf "$(Y)  criei deploy/config.yml — edita (domain/tls/dns/modo) e corre 'make deploy' de novo$(Z)\n"; \
	  exit 1; \
	fi
	@printf "$(C)▶ deploy zero-touch (Ansible)$(Z)\n"
	@cd deploy/ansible && ansible-playbook site.yml $(ANSIBLE_ARGS)

.PHONY: deploy-check
deploy-check: ## Dry-run do deploy (ansible --check --diff, não altera nada)
	@cd deploy/ansible && ansible-playbook site.yml --check --diff $(ANSIBLE_ARGS)

.PHONY: deploy-config
deploy-config: ## Cria deploy/config.yml a partir do exemplo (se não existir)
	@[ -f deploy/config.yml ] && printf "$(Y)  deploy/config.yml já existe$(Z)\n" \
	  || { cp deploy/config.example.yml deploy/config.yml; printf "$(G)  ✓ criado deploy/config.yml — edita-o$(Z)\n"; }

# ---- PREPROD KAESO (kind remoto em 172.16.20.117) ----
#  Fluxo: build local → export de imagens → Ansible SSH → kind load → apply manifests
#  Pré-req: chave SSH ou --ask-become-pass (sysadmin@172.16.20.117)
#
#  make export-images   → gera /tmp/dlx-images/*.tar.gz (só build, sem deploy)
#  make deploy-kaeso    → export-images + ansible (deploy completo)

.PHONY: export-images
export-images: image ## Exporta imagens Docker para /tmp/dlx-images/ (transfer. para kind remoto)
	@printf "$(C)▶ a exportar imagens para /tmp/dlx-images/$(Z)\n"
	@mkdir -p /tmp/dlx-images
	@# Uma imagem por arquivo: o `delonix image save` guarda UMA referência por
	@# arquivo (o docker aceita várias). A tag versionada é a que o deploy usa;
	@# `:latest` deixou de ser exportada de propósito — era ela que dava o
	@# "imagem stale" documentado no HARNESS.md.
	@$(IMG_SAVE) $(IMAGE_SERVER) -o /tmp/dlx-images/delonix-server.tar
	@$(IMG_SAVE) $(IMAGE_WEB)    -o /tmp/dlx-images/delonix-web.tar
	@gzip -f /tmp/dlx-images/delonix-server.tar /tmp/dlx-images/delonix-web.tar
	@printf "$(G)  ✓ imagens exportadas:$(Z)\n"
	@ls -lh /tmp/dlx-images/*.tar.gz

.PHONY: deploy-kaeso
deploy-kaeso: export-images ## Build + export + Ansible deploy no preprod kaeso (kind remoto)
	@printf "$(C)▶ deploy preprod kaeso (172.16.20.117)$(Z)\n"
	@[ -f deploy/ansible/.env.kaeso ] || { \
	  printf "$(Y)  ✗ Cria deploy/ansible/.env.kaeso com ANSIBLE_BECOME_PASSWORD=<senha>$(Z)\n"; exit 1; }
	@set -a && . deploy/ansible/.env.kaeso && set +a && \
	  _PASS_FILE=$$(mktemp) && \
	  printf '%s' "$$ANSIBLE_BECOME_PASSWORD" > "$$_PASS_FILE" && \
	  cd deploy/ansible && ansible-playbook site.yml \
	    -i inventory.ini \
	    --limit kaeso01 \
	    -e image_tag=$(IMAGE_TAG) \
	    --become-password-file "$$_PASS_FILE" \
	    $(ANSIBLE_ARGS); \
	  _RC=$$?; rm -f "$$_PASS_FILE"; exit $$_RC


# ============================================================
#  VOICE — camada de media do dial-in PSTN (opcional)
# ============================================================
.PHONY: certs
certs: ## Gera o wildcard *.delonix.local de DEV (mkcert; openssl como alternativa)
	@# O certificado e a CHAVE de dev NÃO são versionados: uma chave privada num
	@# repositório é uma chave comprometida, mesmo sendo só de dev — qualquer
	@# clone passa a poder personificar *.delonix.local em qualquer máquina que
	@# confie na mesma CA mkcert. Gera-se localmente e cada máquina tem a sua.
	@mkdir -p deploy/certs
	@if [ -f deploy/certs/wildcard.delonix.local.crt ] && [ -f deploy/certs/wildcard.delonix.local.key ]; then \
	  printf "$(G)  ✓ wildcard de dev já existe$(Z)\n"; \
	elif command -v mkcert >/dev/null 2>&1; then \
	  mkcert -cert-file deploy/certs/wildcard.delonix.local.crt \
	         -key-file  deploy/certs/wildcard.delonix.local.key \
	         "*.delonix.local" delonix.local >/dev/null 2>&1; \
	  chmod 600 deploy/certs/wildcard.delonix.local.key; \
	  printf "$(G)  ✓ wildcard de dev gerado com mkcert (confiado pelo SO)$(Z)\n"; \
	else \
	  openssl req -x509 -newkey rsa:2048 -nodes -days 825 \
	    -keyout deploy/certs/wildcard.delonix.local.key \
	    -out    deploy/certs/wildcard.delonix.local.crt \
	    -subj "/CN=*.delonix.local" \
	    -addext "subjectAltName=DNS:*.delonix.local,DNS:delonix.local" 2>/dev/null; \
	  chmod 600 deploy/certs/wildcard.delonix.local.key; \
	  printf "$(Y)  ! mkcert ausente — wildcard self-signed (o browser vai avisar).$(Z)\n"; \
	  printf "$(Y)    Instala o mkcert e corre 'make certs' outra vez para um cert confiado.$(Z)\n"; \
	fi

.PHONY: voice-certs voice-up voice-down
voice-certs: ## Gera certificados self-signed de dev para a voz (SIP-TLS/SRTP)
	@mkdir -p $(VOICE_TLS_DIR)
	@openssl req -x509 -newkey rsa:2048 -nodes \
	  -keyout $(VOICE_TLS_DIR)/privkey.pem -out $(VOICE_TLS_DIR)/fullchain.pem \
	  -days 825 -subj "/CN=delonix-voice" \
	  -addext "subjectAltName=DNS:localhost,DNS:delonix-voice,IP:127.0.0.1" 2>/dev/null
	@chmod 600 $(VOICE_TLS_DIR)/privkey.pem
	@printf "$(G)  ✓ certificados de voz em $(VOICE_TLS_DIR)$(Z)\n"

voice-up: ## Sobe Kamailio + FreeSWITCH (dial-in PSTN). Usa o mesmo VOICE_SECRET do backend
	@printf "$(C)▶ camada de media de voz (Kamailio + FreeSWITCH)$(Z)\n"
	@# ACL do trunk (só existe .example até haver IPs reais do fornecedor 5.1).
	@[ -f voice/kamailio/ao_trunk.txt ] || { cp voice/kamailio/ao_trunk.txt.example voice/kamailio/ao_trunk.txt; \
	  printf "$(Y)  ! criei voice/kamailio/ao_trunk.txt vazio — preencher com os IPs do trunk$(Z)\n"; }
	@# Certificados TLS de dev (gera se faltarem).
	@[ -f $(VOICE_TLS_DIR)/fullchain.pem ] || $(MAKE) --no-print-directory voice-certs
	@VOICE_INTERNAL_SECRET=$(VOICE_SECRET) DELONIX_CONTROL_URL=$(API_URL) DELONIX_TLS_DIR=$(VOICE_TLS_DIR) \
	  docker compose -f voice/docker-compose.voice.yml up -d
	@printf "$(G)  ✓ voz a subir (media real exige o trunk contratado — IPs em ao_trunk.txt)$(Z)\n"
voice-down: ## Para a camada de media de voz
	@docker compose -f voice/docker-compose.voice.yml down

# ============================================================
#  MANUTENÇÃO
# ============================================================
.PHONY: clean
clean: ## Limpa artefactos de build (cargo + dist do frontend)
	@cd server && cargo clean
	@rm -rf web/dist $(RUNDIR)
	@printf "$(G)  ✓ limpo$(Z)\n"
