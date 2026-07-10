#!/usr/bin/env bash
# ============================================================
#  Constrói (e opcionalmente publica) as imagens do Delonix Meet.
#
#  Uso:
#    deploy/build-images.sh [--push] [--rootless] [REGISTRY] [TAG]
#
#    --rootless   força um builder ROOTLESS (podman/buildah/nerdctl), sem
#                 daemon nem root. Por omissão deteta o melhor disponível.
#    --push       publica as imagens após o build.
#    REGISTRY     prefixo do registo (default: ghcr.io/OWNER)
#    TAG          etiqueta (default: git short SHA, ou 'latest')
#
#  As imagens correm SEMPRE como não-root (distroless:nonroot / nginx-unprivileged),
#  independentemente do builder usado.
# ============================================================
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

PUSH=0; FORCE_ROOTLESS=0; POS=()
for a in "$@"; do case "$a" in
  --push) PUSH=1;;
  --rootless) FORCE_ROOTLESS=1;;
  *) POS+=("$a");;
esac; done
REGISTRY="${POS[0]:-ghcr.io/OWNER}"
TAG="${POS[1]:-$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo latest)}"

# Escolhe o builder. Rootless: podman > buildah > nerdctl. Senão: docker buildx.
pick_builder() {
  if [ "$FORCE_ROOTLESS" = 1 ]; then
    for b in podman nerdctl buildah; do command -v "$b" >/dev/null && { echo "$b"; return; }; done
    echo "ERRO: --rootless pedido mas nenhum de podman/nerdctl/buildah está instalado" >&2; exit 1
  fi
  if command -v docker >/dev/null && docker buildx version >/dev/null 2>&1; then echo "docker"; return; fi
  for b in podman nerdctl buildah docker; do command -v "$b" >/dev/null && { echo "$b"; return; }; done
  echo "ERRO: nenhum builder de contentores encontrado" >&2; exit 1
}
BUILDER="$(pick_builder)"
echo "▶ builder: $BUILDER   registo: $REGISTRY   tag: $TAG   push: $PUSH"

build_one() {
  local dockerfile="$1" image="$2"
  local ref="$REGISTRY/$image:$TAG"
  echo "▶ a construir $ref"
  case "$BUILDER" in
    docker)  docker buildx build --load -f "$ROOT/$dockerfile" -t "$ref" "$ROOT";;
    podman|nerdctl) "$BUILDER" build -f "$ROOT/$dockerfile" -t "$ref" "$ROOT";;
    buildah) buildah bud -f "$ROOT/$dockerfile" -t "$ref" "$ROOT";;
  esac
  if [ "$PUSH" = 1 ]; then
    echo "▶ a publicar $ref"
    "$BUILDER" push "$ref"
  fi
}

build_one Dockerfile.server    delonix-server
build_one Dockerfile.web       delonix-web
# Worker de transcrição em GPU (opcional — imagem grande com CUDA). Só quando
# BUILD_AI_WORKER=1, para não obrigar toda a gente a puxar a base CUDA.
if [ "${BUILD_AI_WORKER:-0}" = 1 ]; then
  build_one ai-worker/Dockerfile delonix-ai-worker
fi
echo "✔ imagens prontas: $REGISTRY/delonix-{server,web}:$TAG"
echo "  Apontar a kustomization:  ( cd deploy/k8s && kustomize edit set image \\"
echo "    ghcr.io/OWNER/delonix-server=$REGISTRY/delonix-server:$TAG \\"
echo "    ghcr.io/OWNER/delonix-web=$REGISTRY/delonix-web:$TAG )"
