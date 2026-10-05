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
#    4. o FreeSWITCH alcança os dois listeners do servidor;
#    5. o mesmo PBX, pelo tronco da CENTRAL (TLS, fora da allowlist), entra
#       autenticado com a conta SIP da organização e a sala procura-se nela
#       (ADR-0016).
#  Não prova o telefone dentro da sala WebRTC — isso é a R222
#  (delonix-meet-telefonia): aqui a ponte para o SFU não está ligada.
# ============================================================
set -euo pipefail
cd "$(dirname "$0")/.."
: "${CLUSTER_NAME:?}" "${NS:?}" "${KUBECONFIG:?}"
BORDO="kamailio.${NS}.svc.cluster.local"

g=$'\033[1;32m'; y=$'\033[1;33m'; r=$'\033[1;31m'; z=$'\033[0m'
ok() { printf "  %s✓%s %s\n" "$g" "$z" "$1"; }
avisa() { printf "  %s!%s %s\n" "$y" "$z" "$1"; }

FS_IMAGE=delonix-meet/freeswitch:1.11.3
PBX_IMAGE=delonix-meet/pbx-cliente:lab
imagens=$(delonix image ls 2>/dev/null) || imagens=
for img in "$FS_IMAGE" "$PBX_IMAGE"; do
  if ! grep -q "^${img%%:*}:\?[[:space:]]*${img##*:}\|^${img}[[:space:]]" <<<"$imagens"; then
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
  --from-file=voice/freeswitch/autoload_configs/json_cdr.conf.xml \
  --from-file=voice/freeswitch/sip_profiles/internal.xml \
  --from-file=voice/freeswitch/scripts/dialin_ivr.lua \
  --from-file=voice/freeswitch/scripts/ramais_dial.lua
cm pbx-cliente-cfg \
  --from-file=voice/pbx-cliente/pjsip.conf \
  --from-file=voice/pbx-cliente/extensions.conf

# Certificado self-signed do bordo: gerado aqui, vive só no Secret. Leva no SAN
# o nome completo do serviço — é por ele que a central confere o certificado.
# Um Secret de antes disso (sem esse nome) é substituído.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
kubectl -n "$NS" get secret kamailio-tls -o jsonpath='{.data.tls\.crt}' 2>/dev/null | base64 -d >"$tmp/tls.crt" 2>/dev/null || true
san=$(openssl x509 -in "$tmp/tls.crt" -noout -ext subjectAltName 2>/dev/null) || san=
if ! grep -q "DNS:${BORDO}" <<<"$san"; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 825 -subj "/CN=${BORDO}" \
    -addext "subjectAltName=DNS:${BORDO},DNS:kamailio.${NS}.svc,DNS:kamailio" \
    -keyout "$tmp/tls.key" -out "$tmp/tls.crt" 2>/dev/null
  kubectl -n "$NS" create secret tls kamailio-tls --cert="$tmp/tls.crt" --key="$tmp/tls.key" \
    --dry-run=client -o yaml | kubectl apply -f - >/dev/null
fi
# O tronco da CENTRAL (ADR-0016): o modelo de voice/pbx-cliente/, com o nome do
# bordo deste cluster e a password do .env, e o certificado que a central confere.
if [ -n "${VOICE_CENTRAL_PASSWORD:-}" ]; then
  sed -e '/^;/d' -e "s/__BORDO__/${BORDO}/" -e "s/__PASSWORD__/${VOICE_CENTRAL_PASSWORD}/" \
    voice/pbx-cliente/central.conf.tmpl >"$tmp/pbx-central.conf"
else
  : >"$tmp/pbx-central.conf"
  avisa "o .env não tem VOICE_CENTRAL_PASSWORD (corre «make bootstrap»): o PBX fica sem o tronco da central"
fi
kubectl -n "$NS" create secret generic pbx-central \
  --from-file=pbx-central.conf="$tmp/pbx-central.conf" --from-file=ca.pem="$tmp/tls.crt" \
  --dry-run=client -o yaml | kubectl apply -f - >/dev/null
ok "configuração de voz aplicada a partir de voice/"

kubectl apply -f deploy/k8s/cluster/voice.yaml >/dev/null
# As imagens de voz têm tag fixa e a configuração entra por ConfigMap: sem
# reiniciar, os pods continuavam com a imagem e os ficheiros antigos.
# Por ordem: o Kamailio resolve o FreeSWITCH ao arrancar e o PBX resolve o
# Kamailio; quem arranca antes do destino fica sem ele.
for d in freeswitch kamailio pbx-cliente; do
  kubectl -n "$NS" rollout restart "deployment/$d" >/dev/null
  kubectl -n "$NS" rollout status "deployment/$d" --timeout=240s >/dev/null 2>&1 || true
done
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
if lista=$(kubectl -n "$NS" exec deploy/kamailio -- kamcmd dispatcher.list 2>/dev/null) && grep -q "FLAGS: AP" <<<"$lista"; then
  ok "bordo → FreeSWITCH: activo no dispatcher (OPTIONS respondido)"
else
  avisa "bordo → FreeSWITCH: o dispatcher NÃO vê o FreeSWITCH activo"
fi
# O Asterisk só qualifica o tronco de 15 em 15 s: dá-se-lhe tempo.
tronco=0
for _ in 1 2 3 4 5 6 7 8; do
  if contactos=$(kubectl -n "$NS" exec deploy/pbx-cliente -- asterisk -rx "pjsip show contacts" 2>/dev/null) &&
    grep -q "Avail" <<<"$contactos"; then
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
if resp=$(sonda "http://delonix-server-internal.${NS}.svc.cluster.local:8181/internal/v1/voice/ivr/validate") &&
  grep -q '"code":"unsupported_media_type"\|"code":"auth' <<<"$resp"; then
  ok "FreeSWITCH → servidor (listener interno, /internal/v1/voice/ivr/validate): responde"
else
  avisa "FreeSWITCH → servidor (listener interno): NÃO responde"
fi
if resp=$(sonda "http://delonix-server-internal.${NS}.svc.cluster.local:8181/internal/v1/voice/ivr/dialplan-did") &&
  grep -q '"code":"auth' <<<"$resp"; then
  ok "FreeSWITCH → servidor (ramais, listener interno): responde e exige o segredo"
else
  avisa "FreeSWITCH → servidor (ramais, listener interno): NÃO responde"
fi
# R286: as rotas dos ramais saíram do listener público — lá têm de dar 404.
if resp=$(sonda "http://delonix-server.${NS}.svc.cluster.local:8180/api/voice/ivr/directory") &&
  grep -q '"code":"auth' <<<"$resp"; then
  avisa "o directório dos ramais AINDA responde no listener público (/api/voice/ivr/directory)"
else
  ok "o directório dos ramais não responde no listener público"
fi
# ---- a central da organização (ADR-0016) ----
# O MESMO PBX, pelo seu outro tronco: por TLS, fora da allowlist (que só cobre
# a porta 5060 de origem), autenticado com a conta SIP da organização.
central=0
for _ in 1 2 3 4 5 6 7 8; do
  if contactos=$(kubectl -n "$NS" exec deploy/pbx-cliente -- asterisk -rx "pjsip show contacts" 2>/dev/null) &&
    grep -q "meet-central/sip:.*Avail" <<<"$contactos"; then
    central=1
    break
  fi
  sleep 5
done
if [ "$central" = 1 ]; then
  ok "central → bordo: tronco por TLS alcançável, com o certificado do bordo conferido"
else
  avisa "central → bordo: o tronco por TLS NÃO responde"
fi
# A conta SIP da organização e uma sala com PIN, pela API — como no compose,
# mas com o seu próprio ficheiro: a base de dados do cluster é outra.
sala_txt=deploy/compose/generated/sala-telefone-cluster.txt
if [ -n "${MEET_HOST:-}" ]; then
  SALA_TXT="$sala_txt" bash scripts/seed.sh "https://${MEET_HOST}" || true
fi
# `|| true`: com `set -e` e `pipefail`, um ficheiro que falta ou um `exec` que
# falha matava o script aqui, calado, antes do aviso que está mais abaixo.
sala=$(sed -n 's/^sala=//p' "$sala_txt" 2>/dev/null | head -1) || true
pin=$(sed -n 's/^pin=//p' "$sala_txt" 2>/dev/null | head -1) || true
autenticadas() { kubectl -n "$NS" exec deploy/kamailio -- kamcmd cnt.get script centrais_autenticadas 2>/dev/null | grep -oE '[0-9]+' | head -1 || true; }
entradas() { kubectl -n "$NS" exec deploy/freeswitch -- sh -c "grep -acE 'conference\($1@|\[delonix ponte\] sala=$1 ' $fs_log || true" 2>/dev/null | tail -1 || true; }
liga() { kubectl -n "$NS" exec deploy/pbx-cliente -- asterisk -rx "channel originate PJSIP/+244222000001@meet-central extension $1@prova-pin" >/dev/null 2>&1 || true; }
if [ -z "$sala" ] || [ -z "$pin" ]; then
  avisa "central: sem $sala_txt — a chamada da central fica por medir"
else
  a0=$(autenticadas); e0=$(entradas "$sala")
  liga "$pin"; sleep 16
  a1=$(autenticadas); e1=$(entradas "$sala")
  if [ "${a1:-0}" -gt "${a0:-0}" ]; then
    ok "central: o bordo autenticou-a com a conta SIP da organização (fora da allowlist)"
  else
    avisa "central: o bordo NÃO a autenticou — o «Registo SIP» está gravado?"
  fi
  if [ "${e1:-0}" -gt "${e0:-0}" ]; then
    ok "central: o PIN da sala $sala, marcado por DTMF, abriu-a — procurada na organização da central"
  else
    avisa "central: a chamada NÃO entrou na sala $sala"
  fi
  # Controlo negativo: um PIN que não é de nenhuma sala da organização.
  errado=$([ "$pin" = 000000 ] && echo 000001 || echo 000000)
  liga "$errado"; sleep 16
  a2=$(autenticadas); e2=$(entradas "$sala")
  if [ "${a2:-0}" -gt "${a1:-0}" ] && [ "${e2:-0}" -eq "${e1:-0}" ]; then
    ok "central: com um PIN errado, autenticada no bordo e recusada pelo IVR"
  else
    avisa "central: o controlo do PIN errado não se comportou como esperado (autenticadas $a1→$a2, entradas $e1→$e2)"
  fi
fi
avisa "por provar aqui: o telefone a entrar na sala WebRTC — com PIN certo entra na conferência local do FreeSWITCH, porque a ponte para o SFU não está ligada neste ambiente"
