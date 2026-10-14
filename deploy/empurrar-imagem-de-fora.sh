#!/usr/bin/env bash
# ============================================================
#  Publica uma imagem no registo da instalação ESTANDO FORA DA LAN.
#
#  Uso:
#    deploy/empurrar-imagem-de-fora.sh IMAGEM [IMAGEM...]
#    deploy/empurrar-imagem-de-fora.sh --verificar
#
#    IMAGEM       etiqueta LOCAL completa, já com o registo no nome
#                 (p.ex. 192.168.1.11:5000/delonix-web:9b44edb9)
#    --verificar  só diz se o caminho está de pé, sem empurrar nada
#
#  PORQUE ESTE SCRIPT EXISTE
#
#  O registo da instalação (`192.168.1.11:5000`) só responde na LAN do cluster,
#  e esta máquina muda de rede: no escritório da KAESO é `10.3.31.15` e a Wi-Fi
#  «delonix» nem aparece. O `allow_to` do WireGuard do `vertical` NÃO encaminha
#  a porta 5000 — mas encaminha a **22 do `192.168.1.11`**, que é o host onde o
#  registo corre. Logo não é preciso abrir porta nenhuma: leva-se a imagem por
#  SSH e empurra-se DE LÁ, onde o registo é local.
#
#  Medido a 2026-10-14: 1,95 MB/s pelo túnel (10 MB em 5,1 s), ou seja ~1 min
#  para a imagem do servidor (116 MB) e ~2,6 min para a da web (304 MB). O
#  `cp-1` tinha 51 GB livres e o seu docker já traz
#  `insecure-registries: ["192.168.1.11:5000"]`.
#
#  O QUE ESTE SCRIPT **NÃO** FAZ: não constrói (usa `deploy/build-images.sh` ou
#  `podman build`), não faz `helm upgrade`, e NÃO APAGA nada do registo — o
#  registo recusa DELETE (405, medido), por isso o que subir fica lá. Confirma
#  a etiqueta antes de a mandar.
# ============================================================
set -euo pipefail

VERTICAL="${VERTICAL:-root@197.216.1.11}"   # a porta de entrada (WireGuard para a LAN)
REGISTO_HOST="${REGISTO_HOST:-192.168.1.11}"
REGISTO_PORTA="${REGISTO_PORTA:-5000}"
SSH_UTILIZADOR="${SSH_UTILIZADOR:-delonix}" # o `vm_user` do inventário; root não entra nas VMs
PORTA_LOCAL="${PORTA_LOCAL:-2211}"          # a ponta local do túnel para o cp-1:22

VERIFICAR=0
[ "${1:-}" = "--verificar" ] && { VERIFICAR=1; shift; }
if [ "$VERIFICAR" = 0 ] && [ $# -eq 0 ]; then
  sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi

# O cliente local: podman ou docker, o que houver.
LOCAL=""
for c in podman docker; do command -v "$c" >/dev/null 2>&1 && { LOCAL="$c"; break; }; done
[ -n "$LOCAL" ] || { echo "ERRO: nem podman nem docker nesta máquina." >&2; exit 1; }

TUNEL_PID=""
limpar() {
  # Fecha o túnel pelo PID que GUARDÁMOS. Nunca por um padrão de pgrep/pkill no
  # mesmo comando que o kill: `pkill -f` já matou o shell de quem escreveu isto.
  [ -n "$TUNEL_PID" ] && kill "$TUNEL_PID" 2>/dev/null || true
  [ -n "${TAR:-}" ] && rm -f "$TAR" 2>/dev/null || true
}
trap limpar EXIT INT TERM HUP PIPE

remoto() {
  ssh -o ConnectTimeout=10 -o BatchMode=yes -o StrictHostKeyChecking=accept-new \
      -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR \
      -p "$PORTA_LOCAL" "$SSH_UTILIZADOR@127.0.0.1" "$@"
}

# REUTILIZA um túnel que já esteja de pé nesta porta, em vez de falhar com
# «Address already in use»: numa sessão de trabalho é normal já haver um aberto
# à mão. E nesse caso NÃO o fecha no fim — não foi este script que o abriu.
if (exec 3<>/dev/tcp/127.0.0.1/"$PORTA_LOCAL") 2>/dev/null; then
  echo "→ túnel: já havia um em 127.0.0.1:$PORTA_LOCAL — reutilizado (não será fechado)"
else
  echo "→ túnel: 127.0.0.1:$PORTA_LOCAL → $REGISTO_HOST:22, por $VERTICAL"
  ssh -4 -f -N -o ExitOnForwardFailure=yes -o ServerAliveInterval=20 -o ConnectTimeout=15 \
      -L "127.0.0.1:$PORTA_LOCAL:$REGISTO_HOST:22" "$VERTICAL"
  # O `-f` põe o ssh em fundo DEPOIS de abrir o encaminhamento, por isso aqui
  # ele já existe e o PID é o do processo que ficou com o nosso -L.
  sleep 1
  TUNEL_PID="$(ps -eo pid,cmd | grep -F "127.0.0.1:$PORTA_LOCAL:$REGISTO_HOST:22" | grep -v grep | awk '{print $1}' | head -1)"
  [ -n "$TUNEL_PID" ] || { echo "ERRO: o túnel não abriu." >&2; exit 1; }
fi

ANFITRIAO="$(remoto hostname)" || { echo "ERRO: o SSH pelo túnel falhou." >&2; exit 1; }
echo "→ no outro lado: $ANFITRIAO"
remoto "sudo -n docker ps >/dev/null" || {
  echo "ERRO: o utilizador «$SSH_UTILIZADOR» não pode usar o docker em $ANFITRIAO (sudo -n)." >&2
  exit 1; }

if [ "$VERIFICAR" = 1 ]; then
  echo "→ catálogo do registo, visto de lá:"
  remoto "curl -s http://127.0.0.1:$REGISTO_PORTA/v2/_catalog" | sed 's/^/   /'
  echo "✓ caminho de pé: túnel, SSH, sudo docker e registo a responder."
  exit 0
fi

for IMAGEM in "$@"; do
  case "$IMAGEM" in
    "$REGISTO_HOST:$REGISTO_PORTA/"*) ;;
    *) echo "ERRO: «$IMAGEM» não começa por $REGISTO_HOST:$REGISTO_PORTA/ — o" \
            "nome tem de ser o do DESTINO, senão o push vai para outro sítio." >&2; exit 1;;
  esac
  REPO_TAG="${IMAGEM#"$REGISTO_HOST:$REGISTO_PORTA/"}"
  echo "→ $IMAGEM"

  # O `Accept` leva os QUATRO tipos de manifesto. O registo devolve OCI para o
  # que o PODMAN empurrou e docker v2 para o que o DOCKER empurrou; com só um
  # deles o outro dá 404 e o salto nunca dispara. Medido a 2026-10-14: o
  # `delonix-whisper:small-0ed16001` está lá como
  # `application/vnd.oci.image.manifest.v1+json` e dava 404 com o Accept só de
  # docker.
  #
  # PRIMEIRO o registo, só DEPOIS a cópia local — e a ordem é uma correcção:
  # ao contrário, uma etiqueta que JÁ está publicada mas que não existe nesta
  # máquina (porque se libertou disco) dava erro em vez do salto, que é o
  # resultado certo. Apanhado a testar, 2026-10-14.
  ACEITA='application/vnd.oci.image.manifest.v1+json'
  ACEITA="$ACEITA, application/vnd.docker.distribution.manifest.v2+json"
  ACEITA="$ACEITA, application/vnd.oci.image.index.v1+json"
  ACEITA="$ACEITA, application/vnd.docker.distribution.manifest.list.v2+json"
  if remoto "curl -sf -H 'Accept: $ACEITA' \
             http://127.0.0.1:$REGISTO_PORTA/v2/${REPO_TAG%%:*}/manifests/${REPO_TAG##*:} >/dev/null"; then
    echo "   JÁ ESTÁ no registo — o registo recusa DELETE, logo não se reescreve. A saltar."
    continue
  fi

  "$LOCAL" image exists "$IMAGEM" 2>/dev/null || "$LOCAL" inspect "$IMAGEM" >/dev/null 2>&1 || {
    echo "ERRO: a imagem «$IMAGEM» não existe localmente nem está no registo." \
         "Constrói-a primeiro (deploy/build-images.sh ou podman build)." >&2; exit 1; }

  TAR="$(mktemp -t delonix-imagem-XXXXXX.tar)"
  echo "   a gravar o arquivo…"
  "$LOCAL" save -o "$TAR" "$IMAGEM"
  TAM="$(du -m "$TAR" | cut -f1)"
  echo "   $TAM MB a caminho (a 1,95 MB/s medidos, ~$(( TAM * 100 / 195 / 60 + 1 )) min)"
  # Stream directo: não deixa o arquivo no cp-1, que tem 77 GB de disco-raiz.
  < "$TAR" remoto "sudo -n docker load" | sed 's/^/   /'
  rm -f "$TAR"; TAR=""
  echo "   a empurrar para o registo (local, do lado de lá)…"
  remoto "sudo -n docker push '$IMAGEM'" 2>&1 | tail -2 | sed 's/^/   /'
  remoto "sudo -n docker rmi '$IMAGEM' >/dev/null 2>&1" || true   # não deixar lixo no cp-1
  echo "   ✓ no registo"
done

echo "→ catálogo depois:"
remoto "curl -s http://127.0.0.1:$REGISTO_PORTA/v2/_catalog" | sed 's/^/   /'
