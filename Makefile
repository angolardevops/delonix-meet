# ============================================================
#  Delonix Meet — orquestração de ambientes (dev / prod)
#
#  O ciclo de desenvolvimento, por ordem:
#     make bootstrap   → prepara a máquina (ferramentas, dependências, .env, certificado)
#     make dev         → sobe infra + backend + frontend + nginx (dev), imprime URLs
#     make build       → imagens do backend e do frontend (as mesmas do cluster)
#     make compose-up  → simulação de produção numa máquina só (compose.yaml)
#     make cluster     → o stack completo num cluster local, em https://meet.ngolacloud.local
#     make prod        → deploy de produção (segredos + build + publish + smoke)
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
API_PORT   := $(shell printf '%s' "$(API_URL)" | sed -nE 's#.*:([0-9]+).*#\1#p')
API_PORT   := $(if $(API_PORT),$(API_PORT),8180)
WEB_PORT   ?= 5173
RUNDIR     := $(ROOT)/.dev
# Segredo partilhado da API interna de voz (IVR). O MESMO valor tem de ser usado
# pelo backend E pela camada de media (FreeSWITCH), senão a auth do IVR falha.
VOICE_SECRET ?= dev-voice-secret-abc123
# Certificados TLS/SRTP da voz (dev: self-signed no repo, gitignored).
# Nginx standalone de dev (termina TLS para https://meet.delonix.local; ver
# deploy/nginx-dev.conf.template). Path próprio, nunca /etc/nginx/sites-*.
NGINX_DEV_CONF := $(RUNDIR)/nginx-dev.conf
NGINX_DEV_PID  := $(RUNDIR)/nginx-dev.pid
export PATH := $(NODE_BIN):$(PATH)

# ---- Kubernetes / kind ----
KIND_CLUSTER      ?= delonix-stage
# Versionamento de imagens: tag derivada do git (ex.: v1.0.0-31-g89689ca).
# Cada `make image-push` gera uma tag NOVA e faz pin nos Deployments
# (kubectl set image) → rollouts deterministas, sem o problema do :latest
# stale. Override: make image-push IMAGE_TAG=v1.1.0
IMAGE_TAG         ?= $(shell git describe --tags --always --dirty --abbrev=10 2>/dev/null || echo dev)
IMAGE_SERVER_REPO ?= delonix-server
IMAGE_WEB_REPO    ?= delonix-web
IMAGE_SERVER      := $(IMAGE_SERVER_REPO):$(IMAGE_TAG)
IMAGE_WEB         := $(IMAGE_WEB_REPO):$(IMAGE_TAG)
METALLB_VERSION   ?= v0.14.9

# ---- Ciclo local: compose.yaml e `make cluster` ----
CLUSTER_NAME ?= meet
MEET_HOST    ?= meet.ngolacloud.local
# Ferramentas que o `make bootstrap` instala ficam no projecto, não no sistema.
TOOLS_BIN    := $(ROOT)/.tools/bin
HELM_VERSION ?= v3.16.4
export PATH := $(TOOLS_BIN):$(PATH)
# O motor de compose: delonix (daemonless) se existir, senão docker.
# O nome do projecto vai ANTES do subcomando no docker e DEPOIS no delonix.
ifneq ($(shell command -v delonix 2>/dev/null),)
  COMPOSE   ?= delonix compose
  # Caminho ABSOLUTO do ficheiro: com ele os binds relativos (./deploy/…)
  # resolvem-se contra a pasta do compose.yaml; sem ele, o delonix não os acha.
  COMPOSE_P := -f $(ROOT)/compose.yaml -p delonix-meet
  COMPOSE_EXEC := delonix container exec -it
  # `delonix compose up` responde «already exists, nothing to do» e NÃO recria um
  # contentor cuja configuração mudou (medido a 2026-10-04): o servidor tem de sair para
  # o `CORS_ORIGINS` novo (o do túnel) valer. Só o servidor; a base e o resto ficam.
  SERVER_RECREATE := delonix container stop delonix-server >/dev/null 2>&1; delonix container rm delonix-server >/dev/null 2>&1
else
  COMPOSE   ?= docker compose -f $(ROOT)/compose.yaml -p delonix-meet
  COMPOSE_P :=
  COMPOSE_EXEC := docker exec -it
  SERVER_RECREATE := true  # o `docker compose up` recria sozinho o serviço cuja configuração mudou
endif

# Cores
C := \033[1;36m
G := \033[1;32m
Y := \033[1;33m
Z := \033[0m

# Mata só o processo a OUVIR nesta porta — nunca por nome de comando.
# 'delonix-server'/'vite' são o mesmo nome em QUALQUER worktree deste repo;
# matar por nome apanha processos de outras sessões/checkouts na mesma máquina.
# $(1)=porta  $(2)=sinal (TERM|KILL)
define KILL_PORT
if command -v fuser >/dev/null 2>&1; then fuser -k -$(2) $(1)/tcp 2>/dev/null || true; elif command -v lsof >/dev/null 2>&1; then lsof -ti:$(1) 2>/dev/null | xargs -r kill -$(2) 2>/dev/null || true; fi
endef

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
dev: infra api-bg web-bg nginx-dev ## Sobe infra + backend + frontend (dev) + nginx local e imprime URLs
	@# Dono do hostname: `make dev` aponta meet.delonix.local para 127.0.0.1
	@# (nginx-dev, abaixo). `make stage` aponta o MESMO hostname para o VIP do
	@# kind — os dois nunca correm ao mesmo tempo com o nome certo; o último a
	@# rodar é que fica com o /etc/hosts.
	@if grep -q 'meet\.delonix\.local' /etc/hosts; then \
	  sudo sed -i 's/.*meet\.delonix\.local.*/127.0.0.1 meet.delonix.local/' /etc/hosts; \
	else \
	  echo "127.0.0.1 meet.delonix.local" | sudo tee -a /etc/hosts > /dev/null; \
	fi
	@printf "\n$(G)✔ Ambiente de DEV pronto.$(Z)\n"
	@printf "   API:      $(API_URL)\n"
	@printf "   App:      $(G)http://localhost:$(WEB_PORT)$(Z)  ← câmara/mic OK (localhost já é contexto seguro)\n"
	@printf "   HTTPS:    $(G)https://meet.delonix.local$(Z)  (nginx local de dev → Vite :$(WEB_PORT); câmara/mic OK por IP/hostname na rede)\n"
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
	  $(call KILL_PORT,$(API_PORT),TERM); \
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
	@$(call KILL_PORT,$(WEB_PORT),TERM)
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

.PHONY: nginx-dev
nginx-dev: certs ## Sobe nginx local de dev (só https://meet.delonix.local → Vite; ver deploy/nginx-dev.conf.template)
	@printf "$(C)▶ nginx (dev, https://meet.delonix.local)$(Z)\n"
	@if ! command -v nginx >/dev/null 2>&1; then \
	  printf "$(Y)  ! nginx não encontrado — a instalar (sudo apt-get install -y nginx)$(Z)\n"; \
	  sudo apt-get update -qq && sudo apt-get install -y nginx; \
	fi
	@mkdir -p $(RUNDIR)
	@sed -e 's#__ROOT__#$(ROOT)#g' -e 's#__WEB_PORT__#$(WEB_PORT)#g' \
	  deploy/nginx-dev.conf.template > $(NGINX_DEV_CONF)
	@sudo nginx -t -c $(NGINX_DEV_CONF)
	@if [ -f $(NGINX_DEV_PID) ]; then sudo kill -QUIT $$(cat $(NGINX_DEV_PID)) 2>/dev/null || true; sleep 1; fi
	@sudo nginx -c $(NGINX_DEV_CONF)
	@# --resolve em vez de depender do /etc/hosts: este target pode correr ANTES
	@# de `dev` escrever meet.delonix.local no /etc/hosts (é prerequisito dele).
	@for i in $$(seq 1 10); do sleep 0.5; \
	  [ "$$(curl -sk -o /dev/null -w '%{http_code}' --resolve meet.delonix.local:443:127.0.0.1 https://meet.delonix.local/ 2>/dev/null)" != "000" ] && break; \
	  [ $$i = 10 ] && { printf "$(Y)  ✗ nginx não respondeu em :443 (ver $(RUNDIR)/nginx-dev-error.log)$(Z)\n"; exit 1; }; done
	@printf "$(G)  ✓ nginx a correr (https://meet.delonix.local)$(Z)\n"

.PHONY: nginx-dev-stop
nginx-dev-stop: ## Para o nginx local de dev
	@[ -f $(NGINX_DEV_PID) ] && sudo kill -QUIT $$(cat $(NGINX_DEV_PID)) 2>/dev/null; rm -f $(NGINX_DEV_PID); true

.PHONY: api
api: infra ## Backend em FOREGROUND (dev) — Ctrl-C para parar
	@cd server && DELONIX_ALLOW_INSECURE=1 VOICE_INTERNAL_SECRET=$(VOICE_SECRET) cargo run

.PHONY: web
web: ## Frontend em FOREGROUND (vite HMR, HTTP localhost) — Ctrl-C para parar
	@cd web && [ -d node_modules ] || npm ci; NO_HTTPS=1 PORT=$(WEB_PORT) npm run dev

.PHONY: stop
stop: nginx-dev-stop ## Para o backend + frontend + nginx de dev (mantém a infra)
	@printf "$(C)▶ a parar dev$(Z)\n"
	@# Usa o pid gravado ao arrancar; fallback mata só quem ocupa a porta (nunca por nome).
	@if [ -f $(RUNDIR)/api.pid ]; then kill $$(cat $(RUNDIR)/api.pid) 2>/dev/null || true; rm -f $(RUNDIR)/api.pid; \
	else $(call KILL_PORT,$(API_PORT),TERM); fi
	@if [ -f $(RUNDIR)/web.pid ]; then kill $$(cat $(RUNDIR)/web.pid) 2>/dev/null || true; rm -f $(RUNDIR)/web.pid; fi
	@$(call KILL_PORT,$(WEB_PORT),TERM)
	@printf "$(G)  ✓ parado (infra continua; 'make down' para a infra também)$(Z)\n"

.PHONY: down
down: stop ## Para TODA a stack Delonix: dev (processos + docker compose + voice)
	@printf "$(C)▶ docker compose down (dev infra)$(Z)\n"
	@docker compose down
	@printf "$(G)  ✓ stack completa parada (kind continua; 'make destroy' para o k8s)$(Z)\n"

.PHONY: kill
kill: nginx-dev-stop ## Para TUDO (processos locais + docker + k8s) — estado zero até 'make dev' ou 'make stage'
	@printf "$(C)▶ a parar processos locais (backend + frontend)...$(Z)\n"
	@if [ -f $(RUNDIR)/api.pid ]; then kill $$(cat $(RUNDIR)/api.pid) 2>/dev/null || true; rm -f $(RUNDIR)/api.pid; fi
	@if [ -f $(RUNDIR)/web.pid ]; then kill $$(cat $(RUNDIR)/web.pid) 2>/dev/null || true; rm -f $(RUNDIR)/web.pid; fi
	@$(call KILL_PORT,$(API_PORT),TERM)
	@sleep 1 && $(call KILL_PORT,$(API_PORT),KILL)
	@$(call KILL_PORT,$(WEB_PORT),TERM)
	@printf "$(C)▶ a parar docker compose (infra dev + voice)...$(Z)\n"
	@docker compose down 2>/dev/null || true
	@printf "$(C)▶ a escalar workloads k8s para 0 (namespace delonix-meet)...$(Z)\n"
	@kubectl scale deployment --all -n ngolacloud-meet --replicas=0 2>/dev/null || true
	@kubectl scale statefulset --all -n ngolacloud-meet --replicas=0 2>/dev/null || true
	@printf "$(G)  ✓ TUDO parado. Nada corre até fazer 'make dev' (local) ou 'make stage' (k8s).$(Z)\n"

.PHONY: logs logs-api logs-web logs-nginx
logs: ## Segue os logs do backend + frontend (dev)
	@mkdir -p $(RUNDIR) && touch $(RUNDIR)/api.log $(RUNDIR)/web.log
	@tail -n 40 -f $(RUNDIR)/api.log $(RUNDIR)/web.log
logs-api: ; @mkdir -p $(RUNDIR) && touch $(RUNDIR)/api.log && tail -n 60 -f $(RUNDIR)/api.log
logs-web: ; @mkdir -p $(RUNDIR) && touch $(RUNDIR)/web.log && tail -n 60 -f $(RUNDIR)/web.log
logs-nginx: ; @mkdir -p $(RUNDIR) && touch $(RUNDIR)/nginx-dev-error.log $(RUNDIR)/nginx-dev-access.log && tail -n 60 -f $(RUNDIR)/nginx-dev-error.log $(RUNDIR)/nginx-dev-access.log

# ============================================================
#  BUILD / TEST / MIGRATE
# ============================================================
.PHONY: compile
compile: ## Compila backend (release) + frontend (produção), sem imagens
	@printf "$(C)▶ compilar backend + frontend$(Z)\n"
	@cd server && cargo build --release
	@cd web && npm ci && npm run build
	@printf "$(G)  ✓ compilação concluída$(Z)\n"

# `build` produz o ARTEFACTO que se implanta — as imagens —, e não binários
# soltos: é o que o `compose.yaml` e o `make cluster` correm. Quem só quer os
# binários (deploy bare-metal legado) usa `make compile`.
.PHONY: build
build: image ## Constrói as imagens do backend e do frontend (delonix-server, delonix-web)

.PHONY: test
test: fitness web-deps ## Corre os testes (fitness functions + cargo test + typecheck do frontend)
	@printf "$(C)▶ testes$(Z)\n"
	@cd server && DATABASE_URL=$${DATABASE_URL:-postgres://delonix:delonix_dev@localhost:5435/delonix_meet} TEST_REDIS_URL=$${TEST_REDIS_URL:-redis://localhost:6379} cargo test --release --workspace -- --test-threads=4
	@cd web && node_modules/.bin/tsc -p tsconfig.json --noEmit && printf "$(G)  ✓ tsc limpo$(Z)\n"
	@cd web && node_modules/.bin/vitest run && printf "$(G)  ✓ vitest (R1/R2)$(Z)\n"
	@cd web && npm run build >/dev/null && printf "$(G)  ✓ build do frontend (compila SCSS — R54)$(Z)\n"
	@$(MAKE) --no-print-directory test-ai-worker

.PHONY: test-ai-worker
test-ai-worker: ## Testes do worker de IA (Python): batimento, ciclo, gRPC com dobras
	@# Até 2026-10-08 estes 24 testes NÃO corriam em sítio nenhum — nem aqui nem
	@# no CI. Precisam só do grpcio/protobuf (o faster-whisper é importado tarde,
	@# dentro do construtor, e nenhum teste o constrói). Sem grpcio, três deles
	@# dão ModuleNotFoundError — e é por isso que isto DIZ o que falta em vez de
	@# passar por cima.
	@if ! python3 -c 'import grpc' >/dev/null 2>&1; then \
	  printf "$(Y)  · falta o grpcio: pip install grpcio==1.66.2 protobuf==5.27.5$(Z)\n"; \
	  printf "$(Y)    (3 dos 24 testes vão falhar com ModuleNotFoundError)$(Z)\n"; \
	fi
	@cd ai-worker/tests && python3 -m unittest discover -s . && printf "$(G)  ✓ worker de IA$(Z)\n"

.PHONY: web-deps
web-deps: ## Garante web/node_modules (npm ci) — sem isto o `make test` morria com um 'Error 127' opaco
	@if [ ! -x web/node_modules/.bin/tsc ]; then \
	  printf "$(C)▶ web/node_modules ausente — npm ci$(Z)\n"; \
	  cd web && npm ci; \
	fi

.PHONY: fitness
fitness: ## Fitness functions: formatação, higiene, CAPACIDADES VENDIDAS, autorização de rotas, docs, afinidade (R3), Lua e XML do FreeSWITCH, ARQUITECTURA (ADR-0004), clippy, deps, RLS
	@# `fmt --check` AQUI e não só no CI: sem ele, uma alteração formatada a
	@# meio passa o `make test` local e só falha no CI, depois de um push e de
	@# vários minutos de espera. O portão local tem de ser o mesmo do remoto.
	@cd server && cargo fmt --check && printf "$(G)  ✓ formatação Rust$(Z)\n"
	@bash scripts/check-repo-hygiene.sh
	@bash scripts/check-capability-claims.sh
	@bash scripts/check-frontend-lint.sh
	@bash scripts/check-route-auth.sh
	@bash scripts/check-docs-drift.sh
	@bash scripts/check-filas-reivindicacao.sh
	@bash scripts/check-room-affinity.sh
	@bash scripts/check-lua-sintaxe.sh
	@bash scripts/check-fs-xml.sh
	@bash scripts/check-ffmpeg-licenca.sh
	@bash scripts/check-bordo-anuncia.sh
	@bash scripts/check-bordo-central.sh
	@bash scripts/check-replicas-compose.sh
	@bash scripts/check-k8s-render.sh
	@HELM=$(HELM) bash scripts/check-helm.sh
	@HELM=$(HELM) bash scripts/check-observabilidade.sh
	@bash scripts/check-ansible-producao.sh
	@bash scripts/check-crypto-provider.sh
	@bash scripts/check-arquitectura-catraca.sh
	@bash scripts/check-crate-deps.sh
	@bash scripts/check-proto.sh
	@bash scripts/check-openapi.sh
	@bash scripts/check-clippy-ratchet.sh
	@bash scripts/check-dep-audit.sh
	@bash scripts/check-tenant-rls.sh

# ---- Chart Helm (deploy/helm/delonix-meet) ----
# O chart é um segundo caminho para o cluster, ao lado de deploy/k8s; o portão
# é o mesmo que o `make fitness` corre. HELM=/caminho/helm escolhe o binário.
HELM       ?= helm
HELM_CHART := deploy/helm/delonix-meet
HELM_DIST  := deploy/helm/dist

.PHONY: helm-lint helm-package
helm-lint: ## Portão do chart Helm: lint, recusas, render dos três perfis, ingress, afinidade, segredos, tags
	@HELM=$(HELM) bash scripts/check-helm.sh

helm-package: helm-lint ## Empacota o chart em deploy/helm/dist (appVersion = IMAGE_TAG)
	@mkdir -p $(HELM_DIST)
	@$(HELM) package $(HELM_CHART) -d $(HELM_DIST) --app-version $(IMAGE_TAG) 2>&1 | grep -v 'found symbolic link'

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
  # O `delonix build` 4.4.0 não faz COPY para uma imagem final sem shell
  # (distroless): ver o cabeçalho de Dockerfile.server.slim. A imagem de
  # produção sai de `make build BUILDER=docker` ou do CI.
  SERVER_DOCKERFILE ?= Dockerfile.server.slim
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
  SERVER_DOCKERFILE ?= Dockerfile.server
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
	@$(IMG_BUILD) -f $(SERVER_DOCKERFILE) -t $(IMAGE_SERVER) .
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
	@kubectl -n ngolacloud-meet set image deployment/delonix-server server=$(IMAGE_SERVER)
	@kubectl -n ngolacloud-meet set image deployment/delonix-web web=$(IMAGE_WEB)
	@kubectl -n ngolacloud-meet rollout status deployment/delonix-server --timeout=180s
	@kubectl -n ngolacloud-meet rollout status deployment/delonix-web --timeout=120s
	@printf "$(G)  ✓ cluster a correr $(IMAGE_SERVER) / $(IMAGE_WEB)$(Z)\n"

.PHONY: image-push
image-push: image push ## Build versionado + load no cluster + pin (pipeline completo p/ stage k8s)

# O FreeSWITCH do Meet (voice/freeswitch/image/). Constrói-se com docker e não
# com $(IMG_BUILD): quem o corre a seguir — scripts/fs-canais.sh (R222) e a
# prova de fumo (R223) — arranca-o com docker, e uma imagem no store do
# delonix não estaria lá. Compilar o FreeSWITCH demora ~15 min sem cache.
FS_IMAGE ?= delonix-meet/freeswitch:1.11.3

.PHONY: freeswitch-image
freeswitch-image: ## Constrói a imagem FreeSWITCH do Meet ($(FS_IMAGE)) e corre a prova de fumo
	@printf "$(C)▶ build $(FS_IMAGE) (FreeSWITCH compilado — ~15 min sem cache)$(Z)\n"
	@docker build -t $(FS_IMAGE) -f voice/freeswitch/image/Containerfile voice/freeswitch/image
	@bash scripts/freeswitch-image-smoke.sh $(FS_IMAGE)

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
	    -f deploy/k8s/helm-values/postgres-stage-values.yaml -n ngolacloud-meet 2>/dev/null \
	    | grep -E '^\s+image:' | awk '{gsub(/"/, "", $$2); print $$2}' | sort -u); \
	 IMGS="$$IMGS $$(helm template delonix-redis bitnami/redis \
	    -f deploy/k8s/helm-values/redis-stage-values.yaml -n ngolacloud-meet 2>/dev/null \
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
# O Service do Postgres visto de dentro do cluster, para o DATABASE_URL do
# Secret que scripts/k8s-app-secrets.sh monta a partir do .env.
STAGE_DB_HOST ?= delonix-postgres-postgresql.ngolacloud-meet.svc.cluster.local
# O `make prod` é LEGADO e não foi validado num cluster (plano de lacunas, O1):
# este host é o que o 01-config.yaml antigo apontava, mantido tal e qual.
PROD_DB_HOST  ?= $(STAGE_DB_HOST)

.PHONY: env-file
env-file:
	@[ -f .env ] || { printf "$(Y)  ✗ falta o .env com os segredos — corre «make bootstrap»$(Z)\n"; exit 1; }

stage: env-file image-push ## Build + kind load + deploy k8s completo no cluster kind local
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
	  -n ngolacloud-meet --dry-run=client -o yaml | kubectl apply -f -
	@printf "$(C)▶ Helm: Postgres (single-node) + Redis (standalone)...$(Z)\n"
	@# Usa bitnami/postgresql em vez de postgresql-ha: o chart HA (pgpool +
	@# postgresql-repmgr) removeu as imagens do Docker Hub em 2024; o chart
	@# simples continua acessível via registry-1.docker.io. Para prod usa-se
	@# postgresql-ha (make prod) com acesso ao OCI registry da Bitnami.
	@# A password da base vem do .env (make bootstrap), nunca de um ficheiro
	@# de valores versionado.
	@set -a; . ./.env; set +a; \
	  helm upgrade --install delonix-postgres bitnami/postgresql \
	    -f deploy/k8s/helm-values/postgres-stage-values.yaml -n ngolacloud-meet \
	    --set auth.password="$$POSTGRES_PASSWORD" --set auth.postgresPassword="$$POSTGRES_PASSWORD"
	@helm upgrade --install delonix-redis bitnami/redis \
	  -f deploy/k8s/helm-values/redis-stage-values.yaml -n ngolacloud-meet
	@printf "$(C)▶ Aplicação Delonix (config + server + web + ingress + coturn)...$(Z)\n"
	@kubectl apply -f deploy/k8s/01-config.yaml
	@bash scripts/k8s-app-secrets.sh $(STAGE_DB_HOST)
	@$(MAKE) --no-print-directory voice-secret-k8s
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
	@printf "   Pods: $(Y)kubectl get po -n ngolacloud-meet$(Z)\n"

DOMAIN ?= meet.delonix.local

# R154 — o segredo da API interna de IVR nasce aleatório no cluster e nunca
# num ficheiro do repositório. Idempotente: se o Secret já existe, não o toca
# (rodar = apagar o Secret e voltar a correr, e actualizar o FreeSWITCH).
.PHONY: voice-secret-k8s
voice-secret-k8s: ## Cria o Secret delonix-voice (VOICE_INTERNAL_SECRET aleatório) se não existir
	@if kubectl -n ngolacloud-meet get secret delonix-voice >/dev/null 2>&1; then \
	  printf "   delonix-voice já existe — mantido\n"; \
	else \
	  kubectl -n ngolacloud-meet create secret generic delonix-voice \
	    --from-literal=VOICE_INTERNAL_SECRET="$$(openssl rand -hex 32)" >/dev/null && \
	  printf "   $(G)✓ delonix-voice criado (VOICE_INTERNAL_SECRET aleatório, 64 hex)$(Z)\n"; \
	fi

.PHONY: prod
prod: env-file ## Deploy de produção K8s (Ansible + Helm + Manifestos + Let's Encrypt)
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
	@set -a; . ./.env; set +a; \
	  helm upgrade --install delonix-postgres bitnami/postgresql-ha -f deploy/k8s/helm-values/postgres-values.yaml -n ngolacloud-meet \
	    --set auth.password="$$POSTGRES_PASSWORD" --set auth.replicationPassword="$$POSTGRES_REPLICATION_PASSWORD"
	@helm upgrade --install delonix-redis bitnami/redis -f deploy/k8s/helm-values/redis-values.yaml -n ngolacloud-meet
	@printf "$(C)▶ Compilando e gerando Docker Image (Distroless Security)...$(Z)\n"
	@docker build -t delonix-meet-server:latest -f Dockerfile.server .
	@printf "$(C)▶ Fazendo deploy da Aplicação com Domínio $(DOMAIN)...$(Z)\n"
	@kubectl apply -f deploy/k8s/01-config.yaml
	@bash scripts/k8s-app-secrets.sh $(PROD_DB_HOST)
	@$(MAKE) --no-print-directory voice-secret-k8s
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
	@kubectl exec -n ngolacloud-meet -it delonix-postgres-postgresql-ha-postgresql-0 -- pg_dump -U postgres delonix > backups/postgres_backup.sql || true
	@printf "$(C)  - A efetuar backup do Redis...$(Z)\n"
	@kubectl exec -n ngolacloud-meet -it delonix-redis-master-0 -- redis-cli SAVE || true
	@kubectl cp delonix-meet/delonix-redis-master-0:/data/dump.rdb backups/redis_dump.rdb || true
	@printf "$(C)  - Destruindo Helm charts e Manifestos...$(Z)\n"
	@helm uninstall delonix-postgres -n ngolacloud-meet || true
	@helm uninstall delonix-redis -n ngolacloud-meet || true
	@kubectl delete namespace delonix-meet || true
	@kind delete cluster --name delonix-stage || true
	@printf "$(G)  ✓ Cluster destruído com sucesso. Backups em ./backups/$(Z)\n"

.PHONY: restore
restore: ## Reconstrói o cluster e faz o restore das bases de dados
	@printf "$(C)▶ Iniciando o processo de restauro do cluster...$(Z)\n"
	@$(MAKE) stage
	@printf "$(C)  - A restaurar PostgreSQL...$(Z)\n"
	@kubectl cp backups/postgres_backup.sql delonix-meet/delonix-postgres-postgresql-ha-postgresql-0:/tmp/backup.sql || true
	@kubectl exec -n ngolacloud-meet -it delonix-postgres-postgresql-ha-postgresql-0 -- psql -U postgres -d delonix -f /tmp/backup.sql || true
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

# ============================================================
#  CICLO LOCAL — bootstrap, simulação de produção e cluster
# ============================================================
.PHONY: bootstrap
bootstrap: ## Prepara a máquina: ferramentas, dependências, .env com segredos e certificado
	@MEET_HOST=$(MEET_HOST) HELM_VERSION=$(HELM_VERSION) bash scripts/bootstrap.sh

# O compose.yaml corre as imagens de `make build` atrás de uma borda com TLS.
# Não constrói nada: sem imagens, falha a dizer isso — não as vai buscar a lado nenhum.
.PHONY: compose-up compose-down compose-ps compose-logs compose-voice-check seed voice-secret-rotate esl-secret-rotate
voice-secret-rotate: ## Troca o VOICE_INTERNAL_SECRET do .env (depois: make compose-up e/ou make cluster)
	@bash scripts/rotate-voice-secret.sh
esl-secret-rotate: ## Troca a TELEPHONY_ESL_PASSWORD (Event Socket do FreeSWITCH) do .env (depois: make compose-down/up e/ou make cluster)
	@bash scripts/rotate-voice-secret.sh .env TELEPHONY_ESL_PASSWORD
seed: ## Cria a organização «ngolacloud», o administrador e a conta demo@ngolacloud.ao (BASE=https://…)
	@bash scripts/seed.sh $(or $(BASE),https://$(MEET_HOST):8443)
compose-voice-check: ## Mede a sinalização da voz no compose: bordo, tronco do PBX e chamada de prova ao IVR
	@bash scripts/compose-voice-check.sh
# LAN_IP=<ip desta máquina na rede local> expõe os RAMAIS a um softphone da
# mesma rede (make compose-up LAN_IP=192.168.1.120). Sem ele, tudo fica só em
# 127.0.0.1 — que é o que se quer por omissão.
compose-up: ## Simulação de produção (compose.yaml); LAN_IP=<ip> expõe os ramais à rede local
	@[ -f .env ] && [ -f deploy/compose/generated/turnserver.conf ] || { printf "$(Y)  ✗ falta o .env ou deploy/compose/generated/ — corre «make bootstrap»$(Z)\n"; exit 1; }
	@grep -qE '^DATA_ENCRYPTION_KEYS=.+' .env || { printf "$(Y)  ✗ o .env não tem DATA_ENCRYPTION_KEYS (o servidor já não arranca sem ela) — corre «make bootstrap»: acrescenta-a sem mexer no resto$(Z)\n"; exit 1; }
	@grep -qE '^TELEPHONY_ESL_PASSWORD=.+' .env || { printf "$(Y)  ✗ o .env não tem TELEPHONY_ESL_PASSWORD (o servidor liga-se ao Event Socket com ela) — corre «make bootstrap»: acrescenta-a sem mexer no resto$(Z)\n"; exit 1; }
	@$(IMG_LS) 2>/dev/null | grep -q "delonix-server" || { printf "$(Y)  ✗ faltam as imagens — corre «make build»$(Z)\n"; exit 1; }
	@$(IMG_LS) 2>/dev/null | grep -q "pbx-cliente" || { printf "$(Y)  ✗ faltam as imagens de voz — corre «make voice-images»$(Z)\n"; exit 1; }
	@# A central da organização (ADR-0016) precisa de dois ficheiros que um
	@# bootstrap antigo não gerou; sem eles o PBX nem arranca.
	@[ -f deploy/compose/generated/pbx-central.conf ] && grep -qE '^DATA_ENCRYPTION_KEYS=.+' .env || { printf "$(Y)  ✗ falta a conta da central ou a chave da cifra em repouso — corre «make bootstrap» (não muda os segredos que já tens)$(Z)\n"; exit 1; }
	@printf "$(C)▶ $(COMPOSE) up (simulação de produção)$(Z)\n"
	@if [ -n "$(LAN_IP)" ]; then \
	  LAN_IP=$(LAN_IP) bash scripts/compose-lan.sh > deploy/compose/generated/lan.yaml && \
	  printf "   ramais expostos na rede local em $(Y)$(LAN_IP):5070$(Z) (áudio em 20000–20100/udp)\n" && \
	  printf "   borda na rede local em $(Y)https://$(LAN_IP):8443$(Z); raiz de laboratório para o telemóvel: $(Y)http://$(LAN_IP):8080/lab-ca.crt$(Z)\n" && \
	  $(COMPOSE) up $(COMPOSE_P) -f $(ROOT)/deploy/compose/generated/lan.yaml -d; \
	else \
	  $(COMPOSE) up $(COMPOSE_P) -d; \
	fi
	@printf "$(C)▶ organização e conta de validação$(Z)\n"
	@bash scripts/seed.sh https://$(MEET_HOST):8443 || true
	@$(MAKE) --no-print-directory compose-info

# Os acessos vêm do .env desta máquina (gerados por `make bootstrap`): são de
# laboratório, e mostram-se aqui para não se andar à procura deles.
.PHONY: compose-info
compose-info: ## URLs e acessos de administração da simulação de produção
	@v() { sed -n "s/^$$1=//p" .env | head -1; }; \
	printf "\n$(G)  Delonix Meet$(Z)      $(Y)https://$(MEET_HOST):8443$(Z)\n"; \
	printf "     organização  ngolacloud\n"; \
	printf "     utilizador   admin@ngolacloud.local\n"; \
	printf "     password     %s\n" "$$(v MEET_ADMIN_PASSWORD)"; \
	printf "$(G)  Kamailio$(Z)          $(Y)https://$(MEET_HOST):8444/rpc/$(Z)   (bordo SIP — interface de gestão xhttp_rpc)\n"; \
	printf "     utilizador   admin\n"; \
	printf "     password     %s\n" "$$(v VOICE_ADMIN_PASSWORD)"; \
	printf "$(G)  PBX de cliente$(Z)    $(Y)https://$(MEET_HOST):8445/ari/api-docs/resources.json$(Z)   (Asterisk — API de administração ARI)\n"; \
	printf "     utilizador   admin\n"; \
	printf "     password     %s\n" "$$(v VOICE_ADMIN_PASSWORD)"; \
	printf "     consola      $(COMPOSE_EXEC) delonix-pbx asterisk -rvvv\n"; \
	if [ -f deploy/compose/generated/ramal-linphone.txt ] && [ -f deploy/compose/generated/lan.yaml ]; then \
	  r() { sed -n "s/^$$1=//p" deploy/compose/generated/ramal-linphone.txt; }; \
	  ip=$$(sed -n 's/.*DELONIX_EXTERNAL_IP: //p' deploy/compose/generated/lan.yaml); \
	  printf "$(G)  Ramal para um softphone$(Z) (Linphone, na mesma rede local)   ramal %s\n" "$$(r ramal)"; \
	  printf "     utilizador   %s\n" "$$(r utilizador)"; \
	  printf "     password     %s\n" "$$(r password)"; \
	  printf "     domínio      %s\n" "$$(r dominio)"; \
	  printf "     servidor     %s:5070   transporte UDP   (proxy: sip:%s:5070;transport=udp)\n" "$$ip" "$$ip"; \
	  printf "     media        SRTP obrigatório (SDES); sem ICE nem STUN\n"; \
	fi; \
	if [ -f deploy/compose/generated/sala-telefone.txt ]; then \
	  t() { sed -n "s/^$$1=//p" deploy/compose/generated/sala-telefone.txt; }; \
	  printf "$(G)  Entrar numa reunião por telefone$(Z)   sala %s\n" "$$(t sala)"; \
	  printf "     de um ramal  marcar 8000 e depois o PIN %s seguido de #\n" "$$(t pin)"; \
	  if grep -q 'PHONE_BRIDGE_SIP_BIND' compose.yaml; then \
	    printf "     no browser   https://$(MEET_HOST):8443/#/r/%s\n" "$$(t sala)"; \
	  else \
	    printf "     $(Y)ponte telefone↔sala DESLIGADA neste compose$(Z): o PIN é aceite, mas a chamada cai numa conferência\n"; \
	    printf "     SÓ de telefones e o ramal NÃO aparece na sala do browser (medido a 2026-10-04: o servidor regista\n"; \
	    printf "     «PHONE_BRIDGE_SIP_BIND não configurado»). Ligá-la exige IPs exactos que este motor muda a cada arranque.\n"; \
	    printf "     sala (sem o telefone)  https://$(MEET_HOST):8443/#/r/%s\n" "$$(t sala)"; \
	  fi; \
	fi; \
	printf "\n  Estado: make compose-ps   ·   Prova da voz: make compose-voice-check\n"
# O QR do Linphone é um URL https que o TELEMÓVEL abre; `meet.ngolacloud.local` é
# mDNS e o certificado do bootstrap é autoassinado (ver compose-tunnel.sh). O túnel
# dá um nome público e um certificado válido, com um URL NOVO a cada execução
# (Pinggy sem conta: 60 minutos). Publica a borda INTEIRA na Internet — só em
# laboratório — e NÃO resolve o registo SIP (UDP): para isso, `compose-up LAN_IP=…`.
# A ordem dos ficheiros conta: o `lan.yaml` (se existir) primeiro, o túnel por cima.
TUNNEL_FILES = $(if $(wildcard deploy/compose/generated/lan.yaml),-f $(ROOT)/deploy/compose/generated/lan.yaml)
tunnel: ## Publica a borda do compose num túnel Pinggy (URL novo, 60 min) para ler o QR do Linphone no telemóvel
	@printf "$(C)▶ túnel Pinggy para a borda do compose (publica a borda INTEIRA na Internet)$(Z)\n"
	@URL=$$(MEET_HOST=$(MEET_HOST) bash scripts/compose-tunnel.sh up) || exit 1; \
	printf "   URL do túnel: $(Y)$$URL$(Z)\n"; \
	printf "$(C)▶ $(COMPOSE) up (o servidor passa a pôr o túnel na 1.ª origem de CORS_ORIGINS)$(Z)\n"; \
	$(SERVER_RECREATE); \
	$(COMPOSE) up $(COMPOSE_P) $(TUNNEL_FILES) -f $(ROOT)/deploy/compose/generated/tunnel.yaml -d || { bash scripts/compose-tunnel.sh down; exit 1; }; \
	printf "\n   Na consola ($(Y)https://$(MEET_HOST):8443$(Z)) emite um QR novo: o URL dele já é o do túnel.\n"; \
	printf "   Só o QR e a descarga da configuração; o registo SIP (UDP 5070) não passa por aqui.\n"; \
	printf "   Acabou? $(Y)make tunnel-stop$(Z)\n"

tunnel-stop: ## Fecha o túnel e devolve o servidor às origens do compose
	@bash scripts/compose-tunnel.sh down
	@printf "$(C)▶ $(COMPOSE) up (origens do compose, sem túnel)$(Z)\n"
	@$(SERVER_RECREATE); $(COMPOSE) up $(COMPOSE_P) $(TUNNEL_FILES) -d

compose-down: ## Para a simulação de produção (mantém os volumes)
	@$(COMPOSE) down $(COMPOSE_P)
compose-ps: ## Contentores da simulação de produção
	@$(COMPOSE) ps $(COMPOSE_P)
compose-logs: ## Logs da simulação de produção (SVC=server para um só)
	@$(COMPOSE) logs $(COMPOSE_P) $(SVC)

# As imagens da voz para o cluster local. O FreeSWITCH constrói-se com docker
# (make freeswitch-image, ~15 min) e é IMPORTADO para o store do delonix, que é
# de onde o `cluster load` o lê; o PBX de cliente é um Asterisk de stock.
PBX_IMAGE ?= delonix-meet/pbx-cliente:lab
.PHONY: voice-images
voice-images: ## Imagens de voz para o cluster local: FreeSWITCH (importado) + PBX de cliente
	@printf "$(C)▶ $(PBX_IMAGE) (Asterisk de stock)$(Z)\n"
	@$(IMG_BUILD) -f voice/pbx-cliente/Containerfile -t $(PBX_IMAGE) voice/pbx-cliente
	@printf "$(C)▶ $(FS_IMAGE) → store do delonix$(Z)\n"
	@if delonix image ls 2>/dev/null | grep -q "^$(FS_IMAGE)[[:space:]]"; then \
	  printf "   já lá está\n"; \
	elif docker image inspect $(FS_IMAGE) >/dev/null 2>&1; then \
	  t=$$(mktemp --suffix=.tar); docker save $(FS_IMAGE) -o $$t && delonix image load -i $$t >/dev/null; rm -f $$t; \
	  printf "   importada do docker\n"; \
	else \
	  printf "$(Y)  ✗ falta a $(FS_IMAGE) — corre «make freeswitch-image» (~15 min) e repete$(Z)\n"; exit 1; \
	fi
	@printf "$(G)  ✓ imagens de voz prontas$(Z)\n"

.PHONY: cluster cluster-status cluster-reset-db cluster-down
cluster: ## Stack completo num cluster local + contas de validação (demo@ngolacloud.ao) — https://$(MEET_HOST)
	@CLUSTER_NAME=$(CLUSTER_NAME) MEET_HOST=$(MEET_HOST) IMAGE_TAG=$(IMAGE_TAG) bash scripts/cluster.sh up
cluster-status: ## Estado do cluster local: nós, pods, ingress e a prova de fumo
	@CLUSTER_NAME=$(CLUSTER_NAME) MEET_HOST=$(MEET_HOST) bash scripts/cluster.sh status
cluster-reset-db: ## Apaga a base de dados do cluster local (depois: make cluster para a recriar)
	@CLUSTER_NAME=$(CLUSTER_NAME) MEET_HOST=$(MEET_HOST) bash scripts/cluster.sh reset-db
cluster-down: ## Destrói o cluster local (nós, rede e kubeconfig)
	@CLUSTER_NAME=$(CLUSTER_NAME) MEET_HOST=$(MEET_HOST) bash scripts/cluster.sh down

# ============================================================
#  PRODUÇÃO — meet.ngolacloud.com (ADR-0020)
#
#  Infra PRÓPRIA do Meet, num cluster só dele. NÃO é o `make cluster`, que é
#  o laboratório local: o que as separa é o cluster e o kubeconfig.
# ============================================================
.PHONY: prod-vms prod-vms-plano prod-inventario prod-k8s prod-k8s-ensaio prod-plataforma prod-observabilidade
prod-vms-plano: ## Produção: o que o OpenTofu faria às VMs do Proxmox (LÊ ANTES de aplicar)
	@cd deploy/tofu && tofu init -input=false >/dev/null && tofu plan
prod-vms: ## Produção: cria/actualiza as VMs do cluster no Proxmox (pede confirmação)
	@cd deploy/tofu && tofu init -input=false >/dev/null && tofu apply
prod-k8s: ## Produção: instala o Kubernetes (3 control-planes) nas VMs do inventário
	@[ -f deploy/ansible/inventory-producao.ini ] || { printf "$(Y)  ✗ falta deploy/ansible/inventory-producao.ini — corre «make prod-inventario»$(Z)\n"; exit 1; }
	@cd deploy/ansible && ansible-playbook -i inventory-producao.ini producao.yml
prod-k8s-ensaio: ## Produção: o que o Ansible MUDARIA no cluster, sem mudar nada
	@cd deploy/ansible && ansible-playbook -i inventory-producao.ini producao.yml --check --diff
prod-plataforma: ## Produção: instala a plataforma do cluster (ingress, TLS, Postgres, Redis, MinIO)
	@bash deploy/k8s/plataforma/instalar.sh
prod-observabilidade: ## Produção: instala Prometheus, Grafana, Loki, Tempo e o colector OTLP
	@bash deploy/k8s/observabilidade/instalar.sh
prod-inventario: ## Produção: escreve o inventário do Ansible a partir do estado do OpenTofu
	@cd deploy/tofu && tofu output -raw inventario_ansible > ../ansible/inventory-producao.ini
	@printf "$(G)  ✓ deploy/ansible/inventory-producao.ini escrito do estado do OpenTofu$(Z)\n"

# ============================================================
#  MANUTENÇÃO
# ============================================================
.PHONY: clean
clean: ## Limpa artefactos de build (cargo + dist do frontend)
	@cd server && cargo clean
	@rm -rf web/dist $(RUNDIR)
	@printf "$(G)  ✓ limpo$(Z)\n"
