#!/usr/bin/env bash
# ============================================================
#  Prova do tronco de uma central FreePBX até ao bordo do Meet
#  (o `meet_trunk` do `kind: PbxService`, delonix-paas, ADR 0063 I4).
#
#  Corre contra uma RÉPLICA do bordo de voz (voice/pbx-tronco-prova/compose.yaml:
#  Kamailio, FreeSWITCH, servidor, Postgres, Redis — projecto, rede e nomes
#  próprios), nunca contra o laboratório do compose.yaml da raiz.
#
#    up                  ergue a réplica, semeia uma sala de prova (número de
#                        acesso +244222000001, PIN 123456) e imprime o caminho da
#                        CA do bordo — com ela se gera a configuração da central:
#                          PBX_MEET_HOST=172.30.50.14 PBX_MEET_CA_FILE=<ca> \
#                          PBX_SEED_OUT=<ficheiro> cargo test -p delonix-orchestrator \
#                            --lib dump_the_generated_user_data_when_asked   (delonix-paas)
#    freepbx --seed F    arranca a appliance FreePBX com ESSA configuração (QEMU),
#                        e da central faz duas chamadas ao número de acesso: PIN
#                        certo e PIN errado. Mede nos dois lados.
#    negativos           os controlos negativos do bordo: uma origem fora da
#                        allowlist leva 403; uma oferta sem SRTP leva 488.
#    longa               uma chamada de 40 s: o bordo tem de anunciar o seu
#                        endereço, senão o ACK não chega e ela cai aos 32 s (R277).
#    down                desmonta a réplica.
#
#  O que mede: o tronco sobre TLS com SDES, o IVR do Meet alcançado, o PIN a
#  chegar por DTMF e a ser aceite (e o errado recusado), o áudio do IVR a chegar à
#  central, e as duas recusas. O que NÃO mede: o telefone dentro da sala WebRTC (a
#  ponte para o SFU, ADR-0010, não está ligada na réplica: com o PIN certo a
#  chamada entra na conferência local do FreeSWITCH), nem uma operadora.
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
ESTADO=$PWD/.pbx-tronco-prova
COMPOSE=(docker compose --env-file "$ESTADO/.env" -f voice/pbx-tronco-prova/compose.yaml)
IMG_FREEPBX=${FREEPBX_IMAGE:-$HOME/.local/share/delonix/vm-images/freepbx_17-asterisk22-r1.qcow2}
IMG_BS=${SOFTPHONE_IMAGE:-delonix-meet/baresip:1.0.0}
NUMERO=+244222000001
PIN=123456
SALA=prova-tronco
BORDO=172.30.50.14
fail=0
ok()  { printf '  ✓ %s\n' "$*"; }
bad() { printf '  ✗ %s\n' "$*"; fail=1; }
uso() { sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-2}"; }
export ESTADO

segredo() { head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n'; }
psql_() { "${COMPOSE[@]}" exec -T postgres psql -v ON_ERROR_STOP=1 -U delonix -d delonix -qAt "$@"; }
fs_cli() { "${COMPOSE[@]}" exec -T freeswitch sh -c 'P=$(sed -n "s/.*name=\"password\" value=\"\([^\"]*\)\".*/\1/p" /conf/autoload_configs/event_socket.conf.xml); /usr/local/freeswitch/bin/fs_cli -p "$P" -x "'"$1"'"' 2>/dev/null; }
fs_log() { "${COMPOSE[@]}" exec -T freeswitch cat /usr/local/freeswitch/var/log/freeswitch/freeswitch.log 2>/dev/null; }

# ------------------------------------------------------------ up
up() {
  mkdir -p "$ESTADO/tls"; chmod 700 "$ESTADO"
  if [ ! -f "$ESTADO/.env" ]; then
    ( umask 077
      printf 'POSTGRES_PASSWORD=%s\nJWT_SECRET=%s\nTURN_SECRET=%s\nPROVISIONING_SECRET=%s\nVOICE_INTERNAL_SECRET=%s\nESTADO=%s\n' \
        "$(segredo)" "$(segredo)" "$(segredo)" "$(segredo)" "$(segredo)" "$ESTADO" > "$ESTADO/.env" )
  fi
  # A CA da prova e o certificado do bordo, com o IP no SAN: a central verifica-o.
  if [ ! -f "$ESTADO/ca.pem" ]; then
    openssl req -x509 -newkey rsa:2048 -nodes -days 30 -subj "/CN=pbx-tronco-prova CA" \
      -keyout "$ESTADO/ca.key" -out "$ESTADO/ca.pem" 2>/dev/null
    openssl req -newkey rsa:2048 -nodes -subj "/CN=$BORDO" -keyout "$ESTADO/tls/tls.key" -out "$ESTADO/bordo.csr" 2>/dev/null
    printf 'subjectAltName=IP:%s\nextendedKeyUsage=serverAuth\n' "$BORDO" > "$ESTADO/san.ext"
    openssl x509 -req -in "$ESTADO/bordo.csr" -CA "$ESTADO/ca.pem" -CAkey "$ESTADO/ca.key" -CAcreateserial \
      -days 30 -extfile "$ESTADO/san.ext" -out "$ESTADO/tls/tls.crt" 2>/dev/null
    chmod 644 "$ESTADO/tls/tls.key" "$ESTADO/tls/tls.crt"
  fi
  # A allowlist: SÓ o endereço do anfitrião nesta rede (de onde a VM chega).
  printf '# Prova do tronco FreePBX: a única origem é o anfitrião na rede da réplica.\n1 172.30.50.1 32 0 central-freepbx\n' > "$ESTADO/address"
  printf '1 sip:172.30.50.13:5080 0 0 weight=100\n' > "$ESTADO/dispatcher.list"
  "${COMPOSE[@]}" up -d >/dev/null 2>&1 || { echo "✗ a réplica não arrancou"; "${COMPOSE[@]}" up -d; exit 1; }
  local i
  for i in $(seq 1 90); do fs_cli "sofia status" | grep -Eq 'external[[:space:]]+profile.*RUNNING' && break; sleep 2; done
  fs_cli "sofia status" | grep -Eq 'external[[:space:]]+profile.*RUNNING' && ok "FreeSWITCH do Meet: perfil external (5080) a correr" || { bad "o FreeSWITCH não ficou pronto"; "${COMPOSE[@]}" logs --tail 30 freeswitch; }
  for i in $(seq 1 45); do "${COMPOSE[@]}" exec -T kamailio kamcmd dispatcher.list 2>/dev/null | grep -q 'FLAGS: AP' && break; sleep 2; done
  "${COMPOSE[@]}" exec -T kamailio kamcmd dispatcher.list 2>/dev/null | grep -q 'FLAGS: AP' && ok "bordo → FreeSWITCH: activo no dispatcher" || bad "o bordo não vê o FreeSWITCH activo"
  for i in $(seq 1 60); do psql_ -c "SELECT 1 FROM organizations LIMIT 0" >/dev/null 2>&1 && break; sleep 2; done
  # A sala de prova, direito na base: um número de acesso e uma sala com PIN.
  psql_ <<SQL >/dev/null || bad "não consegui semear a sala de prova"
INSERT INTO users (email, username, password_hash) VALUES ('prova@tronco.invalid', 'prova-tronco', 'x') ON CONFLICT DO NOTHING;
INSERT INTO organizations (name, slug, created_by) SELECT 'Prova do tronco', 'prova-tronco', id FROM users WHERE email='prova@tronco.invalid' ON CONFLICT DO NOTHING;
INSERT INTO voice_did (org_id, e164) SELECT id, '$NUMERO' FROM organizations WHERE slug='prova-tronco' ON CONFLICT DO NOTHING;
INSERT INTO voice_room (org_id, room_code, pin, did_id, created_by)
  SELECT o.id, '$SALA', '$PIN', d.id, o.created_by FROM organizations o, voice_did d
   WHERE o.slug='prova-tronco' AND d.e164='$NUMERO'
     AND NOT EXISTS (SELECT 1 FROM voice_room WHERE room_code='$SALA');
SQL
  [ "$(psql_ -c "SELECT count(*) FROM voice_room WHERE room_code='$SALA' AND status='active'")" = 1 ] &&
    ok "sala de prova: $NUMERO com PIN $PIN" || bad "a sala de prova não ficou na base"
  echo "  CA do bordo (para a configuração da central): $ESTADO/ca.pem"
}

# ------------------------------------------------------------ freepbx
freepbx() {
  local seed=""
  while [ $# -gt 0 ]; do case "$1" in --seed) seed=$2; shift ;; *) echo "✗ argumento: $1"; uso 2 ;; esac; shift; done
  [ -f "$seed" ] || { echo "✗ falta --seed <user-data gerado pelo PbxService>"; exit 2; }
  [ -f "$IMG_FREEPBX" ] || { echo "✗ falta a imagem da appliance: $IMG_FREEPBX (FREEPBX_IMAGE=…)"; exit 1; }
  grep -q 'meet-trunk' "$seed" || { echo "✗ o seed não traz o meet_trunk (gera-o com PBX_MEET_HOST)"; exit 2; }
  local d="$ESTADO/freepbx"; rm -rf "$d"; mkdir -p "$d"
  # A configuração do Kind, tal e qual; a prova só ACRESCENTA o que ela não deve ter:
  # um contexto que atende uma chamada de saída, grava o que ouve e marca o PIN.
  python3 - "$seed" "$d/user-data" "$NUMERO" "$PIN" <<'PY'
import sys, yaml
seed, out, numero, pin = sys.argv[1:5]
d = yaml.safe_load(open(seed).read().split("\n", 1)[1]) if open(seed).readline().startswith("#cloud-config") else yaml.safe_load(open(seed))
d.setdefault("write_files", []).append({"path": "/root/prova-pin.conf", "permissions": "0644", "content":
"""
; A central fala desde o início (silêncio, mas em pacotes): atrás de NAT, o
; FreeSWITCH só sabe para onde mandar a voz do IVR depois do primeiro pacote da
; central. Medido: com um `Wait` no lugar disto a central gravava silêncio. Numa
; chamada real o telefone manda áudio desde que atende.
[prova-pin]
exten => _X.,1,MixMonitor(/tmp/prova-total.wav,r(/tmp/prova-ouvido.wav))
 same => n,System(echo ${CHANNEL(rtp,secure,audio)} > /tmp/prova-srtp.txt)
 same => n,Playback(silence/5)
 same => n,SendDTMF(${EXTEN}#,250,200)
 same => n,Playback(silence/10)
 same => n,Playback(silence/5)
 same => n,Hangup()
"""})
run = f"""say(){{ echo "PBXPROVA $1" > /dev/console; }}
for i in $(seq 1 240); do grep -q '^exit=' /var/log/ngolacloud-pbx.log 2>/dev/null && break; sleep 3; done
say "setup: $(tail -1 /var/log/ngolacloud-pbx.log 2>/dev/null)"
grep -vE '^\\s*$' /var/log/ngolacloud-pbx.log | tail -4 | while read l; do say "  | $l"; done
for i in $(seq 1 30); do asterisk -rx 'pjsip show contacts' | grep -q 'meet/.*Avail' && break; sleep 2; done
say "contacto: $(asterisk -rx 'pjsip show contacts' | grep -E 'meet/' | tr -s ' ' | head -1)"
say "transporte: $(asterisk -rx 'pjsip show endpoint meet' | grep -E '^ *transport +:' | tr -s ' ' | head -1)"
cat /root/prova-pin.conf >> /etc/asterisk/extensions_custom.conf
asterisk -rx 'dialplan reload' >/dev/null
for p in {pin} 999999; do
  rm -f /tmp/prova-ouvido.wav /tmp/prova-srtp.txt
  asterisk -rx "channel originate PJSIP/{numero}@meet extension $p@prova-pin" >/dev/null
  sleep 28
  cp /tmp/prova-ouvido.wav /root/ouvido-$p.wav 2>/dev/null
  say "chamada PIN $p: gravação $(stat -c %s /root/ouvido-$p.wav 2>/dev/null || echo 0) bytes, srtp=$(cat /tmp/prova-srtp.txt 2>/dev/null)"
done
say DONE
sync; poweroff"""
d.setdefault("runcmd", []).append(run)
open(out, "w").write("#cloud-config\n" + yaml.safe_dump(d, sort_keys=False))
PY
  printf 'instance-id: pbxprova-%s\nlocal-hostname: pbx\n' "$$" > "$d/meta-data"
  cloud-localds "$d/seed.iso" "$d/user-data" "$d/meta-data" || { echo "✗ cloud-localds"; exit 1; }
  qemu-img create -q -f qcow2 -b "$IMG_FREEPBX" -F qcow2 "$d/ovl.qcow2"
  qemu-img create -q -f qcow2 "$d/dados.qcow2" 2G
  # O que já lá está ANTES (os controlos negativos também acordam o IVR e o bordo):
  # tudo o que se mede a seguir é a diferença.
  kam_encaminhadas() { "${COMPOSE[@]}" exec -T kamailio kamcmd tm.stats 2>/dev/null | awk '/\ttotal:/{t=$2} /total_local:/{l=$2} END{print t-l}'; }
  local antes_kam antes_log
  antes_kam=$(kam_encaminhadas)
  antes_log=$(fs_log | wc -l)
  # As ligações TLS abertas no bordo, amostradas enquanto a central corre.
  ( m=0; while [ ! -f "$d/fim" ]; do
      n=$("${COMPOSE[@]}" exec -T kamailio kamcmd tls.info 2>/dev/null | sed -n 's/.*opened_connections: *//p' | head -1)
      [ "${n:-0}" -gt "$m" ] && m=$n; echo "$m" > "$d/tls-max"; sleep 5
    done ) &
  local amostra=$!
  # Num host carregado o primeiro arranque da appliance leva mais de dez minutos
  # (medido com a carga a 70): PBX_PROVA_TIMEOUT alarga o limite.
  local limite=${PBX_PROVA_TIMEOUT:-1500}
  echo "▶ a central arranca (QEMU, até $(( limite / 60 )) min): primeiro arranque da appliance, guião do Kind, duas chamadas"
  timeout "$limite" qemu-system-x86_64 -enable-kvm -cpu host -m 4096 -smp 2 \
    -drive file="$d/ovl.qcow2",if=virtio,format=qcow2 -drive file="$d/dados.qcow2",if=virtio,format=qcow2 \
    -drive file="$d/seed.iso",if=virtio,format=raw,readonly=on \
    -netdev user,id=n0 -device virtio-net-pci,netdev=n0 -display none -serial file:"$d/serial.log" -no-reboot
  touch "$d/fim"; wait "$amostra" 2>/dev/null
  tr -d '\r' < "$d/serial.log" | grep -a 'PBXPROVA' | sed 's/^.*PBXPROVA /    central | /'
  grep -aq 'PBXPROVA DONE' "$d/serial.log" || { bad "a central não chegou ao fim da prova"; return; }

  echo "▶ o que mediu cada lado"
  grep -aq 'PBXPROVA setup: exit=0' "$d/serial.log" && ok "central: o guião do Kind acabou (exit=0) com o tronco carregado" || bad "central: o guião do Kind não acabou bem"
  grep -aqE 'PBXPROVA contacto: .*meet/sip:172\.30\.50\.14:5061 .*Avail' "$d/serial.log" &&
    ok "central → bordo: tronco alcançável na porta TLS do bordo (OPTIONS respondido)" ||
    bad "central → bordo: o tronco não ficou Avail"
  grep -aqE 'PBXPROVA transporte: .*transport : meet-tls' "$d/serial.log" &&
    ok "central: o tronco usa o transporte TLS (meet-tls, certificado do bordo verificado)" ||
    bad "central: o tronco não está no transporte TLS"
  local tls; tls=$(cat "$d/tls-max" 2>/dev/null || echo 0)
  [ "${tls:-0}" -ge 1 ] && ok "bordo: ligação TLS aberta pela central enquanto correu (máximo $tls)" || bad "bordo: nunca viu uma ligação TLS aberta"
  local n_kam; n_kam=$(( $(kam_encaminhadas) - antes_kam ))
  # Duas chamadas são meia dúzia de transacções (INVITE e BYE de cada). Centenas é
  # um pedido a andar em círculo no bordo até esgotar os saltos — medido: com
  # `rewrite_contact` na central, o ACK e o BYE davam ≈285 transacções.
  [ "$n_kam" -ge 2 ] && [ "$n_kam" -le 12 ] && ok "bordo: encaminhou as chamadas ao FreeSWITCH, sem pedidos em círculo ($n_kam transacções)" ||
    bad "bordo: $n_kam transacções para duas chamadas (esperava entre 2 e 12)"
  local log; log=$(fs_log | tail -n +$(( antes_log + 1 )))
  local n_ivr n_srtp n_conf
  n_ivr=$(grep -ac 'lua(dialin_ivr.lua)' <<<"$log"); n_srtp=$(grep -ac 'Activating audio Secure RTP RECV' <<<"$log")
  n_conf=$(grep -ac "conference($SALA@" <<<"$log")
  [ "$n_ivr" -eq 2 ] && ok "FreeSWITCH: o IVR do Meet correu nas duas chamadas" || bad "FreeSWITCH: o IVR correu $n_ivr vezes, esperava 2"
  [ "$n_srtp" -eq 2 ] && ok "media cifrada: SRTP activo nas duas chamadas (FreeSWITCH)" || bad "SRTP activo em $n_srtp chamadas, esperava 2"
  [ "$(grep -ac 'PBXPROVA chamada PIN .*srtp=1' "$d/serial.log")" -eq 2 ] && ok "media cifrada: SRTP activo nas duas chamadas (central)" || bad "a central não reporta SRTP nas duas chamadas"
  # A chamada aceite acaba porque a central desliga (BYE), não porque o FreeSWITCH
  # desistiu de esperar pelo ACK aos 32 s.
  # (um BYE e um desligar do IVR dão NORMAL_CLEARING; a falta de ACK deu NORMAL_UNSPECIFIED.)
  local causas; causas=$(grep -a "Hangup sofia/external" <<<"$log" | grep -ao '\[[A-Z_]*\]$' | sort | uniq -c | tr -s ' \n' ' ')
  [ "$(grep -a "Hangup sofia/external" <<<"$log" | grep -avc 'NORMAL_CLEARING')" -eq 0 ] &&
    ok "as duas chamadas acabaram limpas — a central desligou uma, o IVR a outra ($causas)" ||
    bad "houve chamadas que o FreeSWITCH desligou por si ($causas)"
  # Uma só entrada na conferência em duas chamadas: a do PIN certo entrou, a do errado não.
  [ "$n_conf" -eq 1 ] && ok "PIN certo ($PIN) por DTMF aceite — entrou na conferência da sala $SALA; PIN errado (999999) recusado" ||
    bad "esperava UMA entrada na conferência (PIN certo sim, errado não); vi $n_conf"
  # O áudio no outro sentido: o IVR fala e a central grava o que OUVE.
  export LIBGUESTFS_BACKEND=direct
  if [ -z "${SUPERMIN_KERNEL:-}" ] && [ ! -r "/boot/vmlinuz-$(uname -r)" ]; then
    for k in $(ls -1r /boot/vmlinuz-*); do v=${k#/boot/vmlinuz-}; [ -r "$k" ] && [ -d "/lib/modules/$v" ] && { export SUPERMIN_KERNEL=$k SUPERMIN_MODULES=/lib/modules/$v; break; }; done
  fi
  # Medido por janelas de 0,5 s e não pela média: a voz do IVR dura poucos segundos
  # numa chamada de vinte, e o silêncio digital dá exactamente 0,000 — a média
  # afogava a voz (0,0045) e parecia silêncio.
  janelas() {  # janelas <wav> <de s> <até s> — quantas janelas de 0,5 s têm voz (RMS > 0,004)
    python3 - "$1" "$2" "$3" <<'PY'
import sys, wave, struct, math
try:
    w = wave.open(sys.argv[1], 'rb'); sr = w.getframerate(); ch = w.getnchannels()
    raw = w.readframes(w.getnframes()); w.close()
    s = struct.unpack('<%dh' % (len(raw) // 2), raw[: len(raw) // 2 * 2])[::ch]
    a, b, win = int(float(sys.argv[2]) * sr), int(float(sys.argv[3]) * sr), sr // 2
    s = s[a:b]
    print(sum(1 for i in range(0, len(s) - win + 1, win)
              if math.sqrt(sum(v * v for v in s[i:i + win]) / win) / 32768 > 0.004))
except Exception:
    print(0)
PY
  }
  local certo errado
  virt-cat -a "$d/ovl.qcow2" "/root/ouvido-$PIN.wav" > "$d/ouvido-certo.wav" 2>/dev/null
  virt-cat -a "$d/ovl.qcow2" "/root/ouvido-999999.wav" > "$d/ouvido-errado.wav" 2>/dev/null
  certo=$(janelas "$d/ouvido-certo.wav" 0 5)
  [ "${certo:-0}" -ge 4 ] && ok "áudio Meet → central: a central ouviu o IVR pedir o PIN ($certo de 10 janelas com voz nos primeiros 5 s)" ||
    bad "áudio Meet → central: a central não ouviu o IVR ($certo de 10 janelas com voz nos primeiros 5 s)"
  # Depois do PIN: aceite, a sala fica calada (a central está sozinha); recusado, o IVR volta a falar.
  certo=$(janelas "$d/ouvido-certo.wav" 10 22); errado=$(janelas "$d/ouvido-errado.wav" 10 22)
  [ "${errado:-0}" -gt "${certo:-0}" ] && [ "${errado:-0}" -ge 6 ] &&
    ok "depois do PIN: recusado, o IVR voltou a falar ($errado janelas com voz); aceite, a sala ficou calada ($certo)" ||
    bad "depois do PIN: esperava mais voz na chamada recusada (certo $certo, errado $errado)"
}

# ------------------------------------------------------------ negativos
negativos() {
  docker image inspect "$IMG_BS" >/dev/null 2>&1 || docker build -q -t "$IMG_BS" -f voice/softphone/Containerfile voice/softphone >/dev/null
  local d="$ESTADO/neg"; rm -rf "$d"; mkdir -p "$d/conf"
  # Uma oferta sem SRTP, por TLS, da ORIGEM permitida (a rede do anfitrião).
  cat > "$d/conf/config" <<EOF
poll_method		epoll
sip_listen		0.0.0.0:15080
audio_player		aubridge,nulo
audio_source		aubridge,nulo
module_path		/usr/lib/baresip/modules
module			g711.so
module			aubridge.so
module_tmp		account.so
module_app		menu.so
EOF
  echo "<sip:prova@$BORDO:5061;transport=tls>;regint=0;audio_codecs=PCMA/8000/1" > "$d/conf/accounts"
  local saida
  saida=$(docker run --rm --network host -v "$d/conf:/conf:ro" --entrypoint baresip "$IMG_BS" -4 -f /conf -t 12 \
    -e "/dial sip:${NUMERO#+}@$BORDO:5061;transport=tls" 2>&1 | sed 's/\x1b\[[0-9;]*m//g')
  grep -q '488' <<<"$saida" && ok "oferta sem SRTP (origem permitida): recusada com 488" ||
    { bad "oferta sem SRTP: não vi o 488"; grep -E 'closed|SIP Progress|established' <<<"$saida" | head -3 | sed 's/^/       /'; }
  # A mesma chamada de uma origem que NÃO está na allowlist (um contentor da rede da réplica).
  saida=$(docker run --rm --network pbxtronco_voz --ip 172.30.50.20 -v "$d/conf:/conf:ro" --entrypoint baresip "$IMG_BS" -4 -f /conf -t 12 \
    -e "/dial sip:${NUMERO#+}@$BORDO:5061;transport=tls" 2>&1 | sed 's/\x1b\[[0-9;]*m//g')
  grep -q '403' <<<"$saida" && ok "origem fora da allowlist (172.30.50.20): recusada pelo bordo com 403" ||
    { bad "origem fora da allowlist: não vi o 403"; grep -E 'closed|SIP Progress|established' <<<"$saida" | head -3 | sed 's/^/       /'; }
}

# ------------------------------------------------------------ longa
# Uma chamada que dura MAIS do que os 32 s ao fim dos quais o FreeSWITCH desiste
# de esperar pelo ACK (R277). Um softphone, por TLS e com SRTP, da origem permitida.
longa() {
  docker image inspect "$IMG_BS" >/dev/null 2>&1 || docker build -q -t "$IMG_BS" -f voice/softphone/Containerfile voice/softphone >/dev/null
  local d="$ESTADO/longa"; rm -rf "$d"; mkdir -p "$d/conf"
  cat > "$d/conf/config" <<EOF
poll_method		epoll
sip_listen		0.0.0.0:15090
audio_player		aubridge,nulo
audio_source		aubridge,nulo
module_path		/usr/lib/baresip/modules
module			g711.so
module			aubridge.so
module			srtp.so
module_tmp		account.so
module_app		menu.so
EOF
  echo "<sip:prova@$BORDO:5061;transport=tls>;regint=0;mediaenc=srtp-mand;audio_codecs=PCMA/8000/1" > "$d/conf/accounts"
  local antes saida log
  antes=$(fs_log | wc -l)
  echo "▶ uma chamada de 40 s pelo bordo (o FreeSWITCH desiste aos 32 s se o ACK não chegar)"
  saida=$(docker run --rm --network host -v "$d/conf:/conf:ro" --entrypoint baresip "$IMG_BS" -4 -s -f /conf -t 40 \
    -e "/dial sip:${NUMERO#+}@$BORDO:5061;transport=tls" 2>&1 | sed 's/\x1b\[[0-9;]*m//g')
  log=$(fs_log | tail -n +$(( antes + 1 )))
  grep -q 'Call established' <<<"$saida" || { bad "a chamada não se estabeleceu"; return; }
  grep -aE '^Record-Route:' <<<"$saida" | grep -q '0\.0\.0\.0' &&
    bad "o bordo anuncia 0.0.0.0 no Record-Route: $(grep -am1 '^Record-Route:' <<<"$saida")" ||
    ok "o bordo anuncia o seu endereço no Record-Route ($(grep -am1 '^Record-Route:' <<<"$saida" | sed 's/.*<sip:\([^;>]*\).*/\1/'))"
  grep -aq '^ACK sip:' <<<"$saida" && ok "o softphone enviou o ACK" || bad "o softphone nunca enviou o ACK"
  grep -a 'Hangup sofia/external' <<<"$log" | grep -aq 'NORMAL_UNSPECIFIED' &&
    bad "o FreeSWITCH desligou a chamada por falta de ACK (NORMAL_UNSPECIFIED)" ||
    ok "a chamada passou dos 32 s: durou $(sed -n 's/.*terminated (duration: \([0-9]*\) secs).*/\1/p' <<<"$saida" | head -1) s e não foi cortada pelo FreeSWITCH"
}

down() { "${COMPOSE[@]}" down -v >/dev/null 2>&1; echo "réplica desmontada"; }

[ $# -ge 1 ] || uso 2
modo=$1; shift
case "$modo" in
  up) up ;;
  freepbx) freepbx "$@" ;;
  negativos) negativos ;;
  longa) longa ;;
  down) down ;;
  -h|--help) uso 0 ;;
  *) echo "✗ modo: $modo"; uso 2 ;;
esac
[ "$modo" = down ] && exit 0
[ "$fail" -eq 0 ] && echo "✓ prova do tronco ($modo): tudo medido" || echo "✗ prova do tronco ($modo): há falhas acima"
exit "$fail"
