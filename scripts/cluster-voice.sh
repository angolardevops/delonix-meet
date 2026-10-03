#!/usr/bin/env bash
# ============================================================
#  A voz no cluster local — chamado pelo scripts/cluster.sh.
#
#  Carrega as imagens de voz nos nós, cria os ConfigMaps a partir dos
#  ficheiros do repo (voice/), aplica deploy/k8s/cluster/voice.yaml e mede a
#  integração de SINALIZAÇÃO:
#    1. o bordo (Kamailio) vê o FreeSWITCH activo no dispatcher;
#    2. o PBX de cliente alcança o bordo (OPTIONS de vida);
#    3. uma chamada do PBX atravessa o bordo e chega ao IVR do Meet;
#    4. o FreeSWITCH alcança os dois listeners do servidor.
#  Não prova uma chamada com áudio nem um PIN aceite — isso é a R222
#  (delonix-meet-telefonia), e a imagem do FreeSWITCH não traz os sons do IVR.
# ============================================================
set -euo pipefail
cd "$(dirname "$0")/.."
: "${CLUSTER_NAME:?}" "${NS:?}" "${KUBECONFIG:?}"

g=$'\033[1;32m'; y=$'\033[1;33m'; r=$'\033[1;31m'; z=$'\033[0m'
ok() { printf "  %s✓%s %s\n" "$g" "$z" "$1"; }
avisa() { printf "  %s!%s %s\n" "$y" "$z" "$1"; }

FS_IMAGE=delonix-meet/freeswitch:1.11.3
PBX_IMAGE=delonix-meet/pbx-cliente:lab
for img in "$FS_IMAGE" "$PBX_IMAGE"; do
  if ! delonix image ls 2>/dev/null | grep -q "^${img%%:*}:\?[[:space:]]*${img##*:}\|^${img}[[:space:]]"; then
    avisa "falta a imagem ${img} — corre «make voice-images». A voz fica por subir."
    exit 0
  fi
done
delonix cluster load "$FS_IMAGE" "$PBX_IMAGE" --name "$CLUSTER_NAME" >/dev/null
ok "imagens de voz carregadas nos nós"

# O servidor passa a ter listener interno (rotas /internal/v1/*), só no cluster.
kubectl -n "$NS" patch configmap delonix-config --type merge \
  -p '{"data":{"INTERNAL_BIND_ADDR":"0.0.0.0:8181"}}' >/dev/null

cm() { kubectl -n "$NS" create configmap "$@" --dry-run=client -o yaml | kubectl apply -f - >/dev/null; }
cm kamailio-cfg \
  --from-file=kamailio.cfg=voice/kamailio/kamailio.cfg \
  --from-file=dispatcher.list=voice/cluster/dispatcher.list \
  --from-file=address=voice/cluster/address \
  --from-file=tls.cfg=voice/cluster/tls.cfg
cm freeswitch-entrypoint --from-file=voice/cluster/freeswitch-entrypoint.sh
cm freeswitch-meet \
  --from-file=voice/freeswitch/dialplan/public/00_delonix_dialin.xml \
  --from-file=voice/freeswitch/dialplan/default/00_delonix_extensions.xml \
  --from-file=voice/freeswitch/autoload_configs/conference.conf.xml \
  --from-file=voice/freeswitch/autoload_configs/xml_curl.conf.xml \
  --from-file=voice/freeswitch/sip_profiles/internal.xml \
  --from-file=voice/freeswitch/scripts/dialin_ivr.lua \
  --from-file=voice/freeswitch/scripts/ramais_dial.lua
cm pbx-cliente-cfg \
  --from-file=voice/pbx-cliente/pjsip.conf \
  --from-file=voice/pbx-cliente/extensions.conf

# Certificado self-signed do bordo: gerado aqui, vive só no Secret.
if ! kubectl -n "$NS" get secret kamailio-tls >/dev/null 2>&1; then
  tmp=$(mktemp -d)
  openssl req -x509 -newkey rsa:2048 -nodes -days 825 -subj "/CN=kamailio.${NS}.svc" \
    -keyout "$tmp/tls.key" -out "$tmp/tls.crt" 2>/dev/null
  kubectl -n "$NS" create secret tls kamailio-tls --cert="$tmp/tls.crt" --key="$tmp/tls.key" >/dev/null
  rm -rf "$tmp"
fi
ok "configuração de voz aplicada a partir de voice/"

kubectl apply -f deploy/k8s/cluster/voice.yaml >/dev/null
# O servidor relê a configuração (listener interno).
kubectl -n "$NS" rollout restart deployment/delonix-server >/dev/null
kubectl -n "$NS" rollout status deployment/delonix-server --timeout=300s >/dev/null
falhou=0
for d in freeswitch kamailio pbx-cliente; do
  if kubectl -n "$NS" rollout status "deployment/$d" --timeout=240s >/dev/null 2>&1; then
    ok "$d a correr"
  else
    avisa "$d não ficou pronto — kubectl -n $NS logs deploy/$d"
    falhou=1
  fi
done
[ "$falhou" = 0 ] || exit 0

# ---- integração de sinalização, medida ----
if kubectl -n "$NS" exec deploy/kamailio -- kamcmd dispatcher.list 2>/dev/null | grep -q "FLAGS: AP"; then
  ok "bordo → FreeSWITCH: activo no dispatcher (OPTIONS respondido)"
else
  avisa "bordo → FreeSWITCH: o dispatcher NÃO vê o FreeSWITCH activo"
fi
# O Asterisk só qualifica o tronco de 15 em 15 s: dá-se-lhe tempo.
tronco=0
for _ in 1 2 3 4 5 6 7 8; do
  if kubectl -n "$NS" exec deploy/pbx-cliente -- asterisk -rx "pjsip show contacts" 2>/dev/null | grep -q "Avail"; then
    tronco=1
    break
  fi
  sleep 5
done
if [ "$tronco" = 1 ]; then
  ok "PBX de cliente → bordo: tronco alcançável (OPTIONS respondido)"
else
  avisa "PBX de cliente → bordo: o tronco NÃO responde"
fi

fs_log=/usr/local/freeswitch/var/log/freeswitch/freeswitch.log
antes=$(kubectl -n "$NS" exec deploy/freeswitch -- sh -c "grep -ac 'lua(dialin_ivr.lua)' $fs_log || true" 2>/dev/null | tail -1)
kubectl -n "$NS" exec deploy/pbx-cliente -- asterisk -rx \
  "channel originate PJSIP/+244222000001@meet application Wait 4" >/dev/null 2>&1 || true
sleep 6
depois=$(kubectl -n "$NS" exec deploy/freeswitch -- sh -c "grep -ac 'lua(dialin_ivr.lua)' $fs_log || true" 2>/dev/null | tail -1)
if [ "${depois:-0}" -gt "${antes:-0}" ]; then
  ok "chamada de prova: PBX → bordo → FreeSWITCH → IVR do Meet (dialin_ivr.lua correu)"
else
  avisa "chamada de prova: NÃO chegou ao IVR do Meet"
fi

# 415 (falta o JSON) e 401 (falta o segredo) são respostas DO SERVIDOR: a rota
# existe e o listener está de pé. Sem resposta, ou 404, é que seria falha.
sonda() {
  kubectl -n "$NS" exec deploy/freeswitch -- sh -c \
    'P=$(sed -n "s/.*name=\"password\" value=\"\([^\"]*\)\".*/\1/p" /conf/autoload_configs/event_socket.conf.xml); /usr/local/freeswitch/bin/fs_cli -p "$P" -x "curl '"$1"' post {}"' 2>/dev/null
}
if sonda "http://delonix-server-internal.${NS}.svc.cluster.local:8181/internal/v1/voice/ivr/validate" | grep -q '"code":"unsupported_media_type"\|"code":"auth'; then
  ok "FreeSWITCH → servidor (listener interno, /internal/v1/voice/ivr/validate): responde"
else
  avisa "FreeSWITCH → servidor (listener interno): NÃO responde"
fi
if sonda "http://delonix-server.${NS}.svc.cluster.local:8180/api/voice/ivr/dialplan-did" | grep -q '"code":"auth'; then
  ok "FreeSWITCH → servidor (listener público, /api/voice/ivr/*): responde e exige o segredo"
else
  avisa "FreeSWITCH → servidor (listener público): NÃO responde"
fi
avisa "por provar: PIN aceite e áudio — a imagem do FreeSWITCH não traz os sons do IVR"
