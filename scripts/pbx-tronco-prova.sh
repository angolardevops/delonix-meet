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
#                        Com --central (depois do modo `central`, que semeia as
#                        organizações): o anfitrião SAI da allowlist do bordo, e a
#                        central só entra autenticada com a conta SIP da organização
#                        A. O seed gera-se com mais PBX_MEET_DOMAIN, PBX_MEET_USER e
#                        PBX_MEET_PASSWORD (os A_* de .pbx-tronco-prova/central.env).
#    negativos           os controlos negativos do bordo: uma origem fora da
#                        allowlist leva 403; uma oferta sem SRTP leva 488.
#    longa               uma chamada de 40 s: o bordo tem de anunciar o seu
#                        endereço, senão o ACK não chega e ela cai aos 32 s (R277).
#    central             a central de uma organização FORA da allowlist (ADR-0016):
#                        entra por TLS, autenticada com a conta SIP da organização,
#                        e o IVR procura a sala NESSA organização. Dois softphones
#                        fazem de central; mede-se a entrada na sala pela ponte
#                        (ADR-0010) com áudio nos dois sentidos, e as recusas — sem
#                        credenciais, password errada, sem TLS, o PIN de outra
#                        organização, o travão por origem e um cabeçalho forjado.
#    browser             um BROWSER na sala e a central a entrar nela: um Chromium com a
#                        pilha real do cliente toca 440 Hz, a central (um softphone,
#                        autenticado como no modo `central`) toca 1000 Hz, e cada um
#                        mede o tom do outro. Precisa de `npm ci` em web/.
#    down                desmonta a réplica.
#
#  O que mede: o tronco sobre TLS com SDES, o IVR do Meet alcançado, o PIN a
#  chegar por DTMF e a ser aceite (e o errado recusado), o áudio do IVR a chegar à
#  central, e as recusas. No modo `central`, a organização de quem liga e a
#  entrada na sala do SFU. No modo `browser`, os dois sentidos do áudio entre
#  a central e um browser na sala. O que NÃO mede: uma operadora, nem a
#  qualidade do áudio (só a presença do tom).
#
#  SERVER_IMAGE=<imagem> corre a réplica com o servidor de outra árvore, sem
#  tocar na delonix-server:latest.
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
ESTADO=$PWD/.pbx-tronco-prova
COMPOSE=(docker compose --env-file "$ESTADO/.env" -f voice/pbx-tronco-prova/compose.yaml)
# A réplica desta prova vive no docker (compose). Os softphones que ela lança
# (scripts/softphone-prova.sh --rede …) têm de correr no MESMO motor, e não no
# que o scripts/motor.sh escolheria sozinho — o delonix, onde existir.
export MOTOR=docker
IMG_FREEPBX=${FREEPBX_IMAGE:-$HOME/.local/share/delonix/vm-images/freepbx_17-asterisk22-r1.qcow2}
IMG_BS=${SOFTPHONE_IMAGE:-delonix-meet/baresip:1.0.0}
NUMERO=+244222000001
PIN=123456
SALA=prova-tronco
BORDO=172.30.50.14
fail=0
ok()  { printf '  ✓ %s\n' "$*"; }
bad() { printf '  ✗ %s\n' "$*"; fail=1; }
uso() { sed -n '2,50p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-2}"; }
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
  # Uma réplica erguida antes do modo `central` não tem a chave da cifra em repouso.
  grep -q '^DATA_ENCRYPTION_KEYS=' "$ESTADO/.env" ||
    ( umask 077; printf 'DATA_ENCRYPTION_KEYS=prova:%s\n' "$(head -c 32 /dev/urandom | base64 -w0)" >> "$ESTADO/.env" )
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
  local i estado
  for i in $(seq 1 90); do estado=$(fs_cli "sofia status") && grep -Eq 'external[[:space:]]+profile.*RUNNING' <<<"$estado" && break; sleep 2; done
  estado=$(fs_cli "sofia status") && grep -Eq 'external[[:space:]]+profile.*RUNNING' <<<"$estado" && ok "FreeSWITCH do Meet: perfil external (5080) a correr" || { bad "o FreeSWITCH não ficou pronto"; "${COMPOSE[@]}" logs --tail 30 freeswitch; }
  for i in $(seq 1 45); do estado=$("${COMPOSE[@]}" exec -T kamailio kamcmd dispatcher.list 2>/dev/null) && grep -q 'FLAGS: AP' <<<"$estado" && break; sleep 2; done
  estado=$("${COMPOSE[@]}" exec -T kamailio kamcmd dispatcher.list 2>/dev/null) && grep -q 'FLAGS: AP' <<<"$estado" && ok "bordo → FreeSWITCH: activo no dispatcher" || bad "o bordo não vê o FreeSWITCH activo"
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
  local seed="" central=0 pin=$PIN sala=$SALA
  while [ $# -gt 0 ]; do case "$1" in --seed) seed=$2; shift ;; --central) central=1 ;; *) echo "✗ argumento: $1"; uso 2 ;; esac; shift; done
  [ -f "$seed" ] || { echo "✗ falta --seed <user-data gerado pelo PbxService>"; exit 2; }
  [ -f "$IMG_FREEPBX" ] || { echo "✗ falta a imagem da appliance: $IMG_FREEPBX (FREEPBX_IMAGE=…)"; exit 1; }
  grep -q 'meet-trunk' "$seed" || { echo "✗ o seed não traz o meet_trunk (gera-o com PBX_MEET_HOST)"; exit 2; }
  if [ "$central" = 1 ]; then
    [ -s "$ESTADO/central.env" ] || { echo "✗ corre primeiro o modo central: é ele que semeia as organizações"; exit 2; }
    # shellcheck disable=SC1091
    . "$ESTADO/central.env"
    grep -q 'meet-auth' "$seed" || { echo "✗ o seed não traz a conta SIP (gera-o com PBX_MEET_DOMAIN, PBX_MEET_USER e PBX_MEET_PASSWORD)"; exit 2; }
    pin=$A_PIN sala=$A_SALA
  fi
  local d="$ESTADO/freepbx"; rm -rf "$d"; mkdir -p "$d"
  # A configuração do Kind, tal e qual; a prova só ACRESCENTA o que ela não deve ter:
  # um contexto que atende uma chamada de saída, grava o que ouve e marca o PIN.
  python3 - "$seed" "$d/user-data" "$NUMERO" "$pin" <<'PY'
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
for i in $(seq 1 30); do asterisk -rx 'pjsip show contacts' | grep 'meet/.*Avail' >/dev/null && break; sleep 2; done
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
  local antes_kam antes_log antes_aut=0
  if [ "$central" = 1 ]; then
    # A central chega do endereço do anfitrião: fora da allowlist, só entra autenticada.
    cp "$ESTADO/address" "$ESTADO/address.antes"
    printf '# Prova da central autenticada: a allowlist está VAZIA.\n' > "$ESTADO/address"
    "${COMPOSE[@]}" restart kamailio >/dev/null 2>&1
    local j lista; for j in $(seq 1 45); do lista=$("${COMPOSE[@]}" exec -T kamailio kamcmd dispatcher.list 2>/dev/null) && grep -q 'FLAGS: AP' <<<"$lista" && break; sleep 2; done
    antes_aut=$(autenticadas)
  fi
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
  # Os contadores do bordo lêem-se ANTES de ele reiniciar: o reinício zera-os.
  local n_aut=0 n_kam
  n_kam=$(( $(kam_encaminhadas) - antes_kam ))
  if [ "$central" = 1 ]; then
    n_aut=$(( $(autenticadas) - antes_aut ))
    # A allowlist volta ao que era — os outros modos contam com o anfitrião nela.
    mv "$ESTADO/address.antes" "$ESTADO/address"; "${COMPOSE[@]}" restart kamailio >/dev/null 2>&1
  fi
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
  # Duas chamadas são meia dúzia de transacções (INVITE e BYE de cada). Centenas é
  # um pedido a andar em círculo no bordo até esgotar os saltos — medido: com
  # `rewrite_contact` na central, o ACK e o BYE davam ≈285 transacções.
  [ "$n_kam" -ge 2 ] && [ "$n_kam" -le 12 ] && ok "bordo: encaminhou as chamadas ao FreeSWITCH, sem pedidos em círculo ($n_kam transacções)" ||
    bad "bordo: $n_kam transacções para duas chamadas (esperava entre 2 e 12)"
  local log; log=$(fs_log | tail -n +$(( antes_log + 1 )))
  local srtp_esperado=2
  if [ "$central" = 1 ]; then
    [ "$n_aut" -eq 2 ] && ok "bordo: com a allowlist vazia, as duas chamadas da central entraram autenticadas com a conta SIP ($A_DOMINIO)" ||
      bad "bordo: $n_aut chamadas de centrais autenticadas, esperava 2"
    srtp_esperado=3   # as duas chamadas, e a perna do FreeSWITCH para a ponte da sala
  fi
  local n_ivr n_srtp n_conf n_ponte
  n_ivr=$(grep -ac 'lua(dialin_ivr.lua)' <<<"$log"); n_srtp=$(grep -ac 'Activating audio Secure RTP RECV' <<<"$log")
  n_conf=$(grep -ac "conference($sala@" <<<"$log")
  [ "$n_ivr" -eq 2 ] && ok "FreeSWITCH: o IVR do Meet correu nas duas chamadas" || bad "FreeSWITCH: o IVR correu $n_ivr vezes, esperava 2"
  [ "$n_srtp" -eq "$srtp_esperado" ] && ok "media cifrada: SRTP activo nas duas chamadas (FreeSWITCH)$([ "$central" = 1 ] && echo ' e na perna para a ponte da sala')" ||
    bad "SRTP activo em $n_srtp pernas, esperava $srtp_esperado"
  [ "$(grep -ac 'PBXPROVA chamada PIN .*srtp=1' "$d/serial.log")" -eq 2 ] && ok "media cifrada: SRTP activo nas duas chamadas (central)" || bad "a central não reporta SRTP nas duas chamadas"
  # A chamada aceite acaba porque a central desliga (BYE), não porque o FreeSWITCH
  # desistiu de esperar pelo ACK aos 32 s.
  # (um BYE e um desligar do IVR dão NORMAL_CLEARING; a falta de ACK deu NORMAL_UNSPECIFIED.)
  local causas; causas=$(grep -a "Hangup sofia/external" <<<"$log" | grep -ao '\[[A-Z_]*\]$' | sort | uniq -c | tr -s ' \n' ' ')
  [ "$(grep -a "Hangup sofia/external" <<<"$log" | grep -avc 'NORMAL_CLEARING')" -eq 0 ] &&
    ok "as duas chamadas acabaram limpas — a central desligou uma, o IVR a outra ($causas)" ||
    bad "houve chamadas que o FreeSWITCH desligou por si ($causas)"
  # Uma só entrada na conferência em duas chamadas: a do PIN certo entrou, a do errado não.
  if [ "$central" = 1 ]; then
    # A sala é a da ORGANIZAÇÃO da central, e a entrada é pela ponte do SFU.
    n_ponte=$(grep -ac "\[delonix ponte\] sala=$sala -> " <<<"$log"); n_conf=$(grep -ac 'conference(' <<<"$log")
    [ "$n_ponte" -eq 1 ] && [ "$n_conf" -eq 0 ] &&
      ok "PIN certo por DTMF aceite — a central entrou na sala $sala da sua organização, pela ponte do SFU; PIN errado (999999) recusado" ||
      bad "esperava UMA entrada pela ponte e nenhuma na conferência local; vi $n_ponte e $n_conf"
  else
  [ "$n_conf" -eq 1 ] && ok "PIN certo ($PIN) por DTMF aceite — entrou na conferência da sala $SALA; PIN errado (999999) recusado" ||
    bad "esperava UMA entrada na conferência (PIN certo sim, errado não); vi $n_conf"
  fi
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
  virt-cat -a "$d/ovl.qcow2" "/root/ouvido-$pin.wav" > "$d/ouvido-certo.wav" 2>/dev/null
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
  # Por TLS, quem não está na allowlist é desafiado a autenticar-se como central
  # (ADR-0016): sem conta, não entra. O 403 seco é de quem nem TLS traz (modo `central`).
  if grep -q 'Call established' <<<"$saida"; then bad "origem fora da allowlist (172.30.50.20), sem credenciais: ENTROU"
  elif grep -qE '40[137]' <<<"$saida"; then ok "origem fora da allowlist (172.30.50.20), sem credenciais: não entra ($(grep -oE '40[137] [A-Za-z ]*' <<<"$saida" | head -1 | sed 's/ *$//'))"
  else bad "origem fora da allowlist: não vi a recusa"; grep -E 'closed|SIP Progress|established' <<<"$saida" | head -3 | sed 's/^/       /'; fi
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
  local antes saida log desligadas
  antes=$(fs_log | wc -l)
  echo "▶ uma chamada de 40 s pelo bordo (o FreeSWITCH desiste aos 32 s se o ACK não chegar)"
  saida=$(docker run --rm --network host -v "$d/conf:/conf:ro" --entrypoint baresip "$IMG_BS" -4 -s -f /conf -t 40 \
    -e "/dial sip:${NUMERO#+}@$BORDO:5061;transport=tls" 2>&1 | sed 's/\x1b\[[0-9;]*m//g')
  log=$(fs_log | tail -n +$(( antes + 1 )))
  grep -q 'Call established' <<<"$saida" || { bad "a chamada não se estabeleceu"; return; }
  grep -aqE '^Record-Route:.*0\.0\.0\.0' <<<"$saida" &&
    bad "o bordo anuncia 0.0.0.0 no Record-Route: $(grep -am1 '^Record-Route:' <<<"$saida")" ||
    ok "o bordo anuncia o seu endereço no Record-Route ($(grep -am1 '^Record-Route:' <<<"$saida" | sed 's/.*<sip:\([^;>]*\).*/\1/'))"
  grep -aq '^ACK sip:' <<<"$saida" && ok "o softphone enviou o ACK" || bad "o softphone nunca enviou o ACK"
  desligadas=$(grep -a 'Hangup sofia/external' <<<"$log")
  grep -aq 'NORMAL_UNSPECIFIED' <<<"$desligadas" &&
    bad "o FreeSWITCH desligou a chamada por falta de ACK (NORMAL_UNSPECIFIED)" ||
    ok "a chamada passou dos 32 s: durou $(sed -n 's/.*terminated (duration: \([0-9]*\) secs).*/\1/p' <<<"$saida" | head -1) s e não foi cortada pelo FreeSWITCH"
}

# ------------------------------------------------------------ central
# A central de uma organização, de um endereço que NÃO está na allowlist.
API=http://172.30.50.12:8180
REDE=pbxtronco_voz
kam() { "${COMPOSE[@]}" exec -T kamailio kamcmd "$@" 2>/dev/null; }
autenticadas() { local n; n=$(kam cnt.get script centrais_autenticadas | grep -oE '[0-9]+' | head -1); echo "${n:-0}"; }
falhas_de() { local n; n=$(kam htable.get falhas "$1" | grep -oE 'value: *[0-9]+' | grep -oE '[0-9]+' | head -1); echo "${n:-0}"; }

# Duas organizações, pela API, como um inquilino faria: conta, sala, número,
# sala de voz (PIN) e o «Registo SIP» com a conta da central.
semear_centrais() {
  [ -s "$ESTADO/central.env" ] && return 0
  ( umask 077; python3 - "$API" > "$ESTADO/central.env" <<'PY'
import json, secrets, sys, urllib.request
api = sys.argv[1]
def call(method, path, body=None, token=None):
    req = urllib.request.Request(api + path, method=method,
                                 data=json.dumps(body).encode() if body is not None else None)
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    with urllib.request.urlopen(req, timeout=30) as r:
        t = r.read().decode()
        return json.loads(t) if t else None
for letra, did in (("A", "+244222000101"), ("B", "+244222000102")):
    l = letra.lower()
    pw = "Prova-" + secrets.token_hex(12) + "-1!"
    email = f"admin@central-{l}.example"
    call("POST", "/api/auth/register", {"org_name": f"Central {letra}", "email": email,
                                        "username": f"admin-central-{l}", "password": pw})
    tok = call("POST", "/api/auth/login", {"email": email, "password": pw})["access_token"]
    org = call("GET", "/api/orgs", token=tok)[0]["id"]
    call("POST", f"/api/orgs/{org}/voice/dids", {"e164": did, "org_scoped": True}, tok)
    code = call("POST", "/api/rooms", {"name": f"Sala da central {letra}", "topology": "sfu"}, tok)["code"]
    pin = call("POST", f"/api/orgs/{org}/voice/rooms", {"room_code": code}, tok)["pin"]
    sip_pw = secrets.token_hex(16)
    call("PUT", f"/api/orgs/{org}/telephony/sip-settings",
         {"domain": f"pbx.central-{l}.example", "transport": "tls", "srtp": "mandatory",
          "username": f"central-{l}", "password": sip_pw}, tok)
    print(f"{letra}_DOMINIO=pbx.central-{l}.example\n{letra}_UTIL=central-{l}\n{letra}_PASS={sip_pw}\n"
          f"{letra}_SALA={code}\n{letra}_PIN={pin}\n{letra}_DID={did}\n{letra}_ADMIN={email}\n{letra}_ADMIN_PW={pw}")
PY
  ) || { rm -f "$ESTADO/central.env"; return 1; }
}

# tentativa <ip> <transporte> <porto> <utilizador> <domínio> <password|-> — um
# softphone da rede da réplica, com SRTP, a marcar o número de acesso; devolve
# o que ele disse, com a sinalização.
tentativa() {
  local ip=$1 transp=$2 porto=$3 util=$4 dom=$5 pw=$6 d conta saida
  d=$(mktemp -d "$ESTADO/central/t.XXXXXX")
  printf '%s\n' 'poll_method		epoll' 'sip_listen		0.0.0.0:15090' 'audio_player		aubridge,nulo' \
    'audio_source		aubridge,nulo' 'module_path		/usr/lib/baresip/modules' 'module			g711.so' \
    'module			aubridge.so' 'module			srtp.so' 'module_tmp		account.so' 'module_app		menu.so' > "$d/config"
  conta="<sip:$util@$dom;transport=$transp>;regint=0;mediaenc=srtp-mand;audio_codecs=PCMA/8000/1"
  [ "$pw" != - ] && conta="$conta;auth_user=$util;auth_pass=$pw"
  printf '%s\n' "$conta" > "$d/accounts"; chmod 755 "$d"; chmod 644 "$d/accounts" "$d/config"
  saida=$(docker run --rm --network "$REDE" --ip "$ip" -v "$d:/conf:ro" --entrypoint baresip "$IMG_BS" -4 -s -f /conf -t "${TENTATIVA_SEG:-10}" \
    -e "/dial sip:${NUMERO#+}@$BORDO:$porto;transport=$transp" 2>&1 | sed 's/\x1b\[[0-9;]*m//g')
  rm -rf "$d"                             # a password não fica no disco do host
  printf '%s\n' "$saida"
}
estado_sip() { grep -aoE '^SIP/2\.0 [0-9]{3}' <<<"$1" | awk '{print $2}' | sort -u | tr '\n' ' '; }

# INVITE escrito à mão, com um `X-Delonix-Central` forjado; devolve o estado final.
forjar() {  # forjar <tls|udp> <host> <porto>
  python3 - "$1" "$2" "$3" "${NUMERO#+}" <<'PY'
import socket, ssl, sys, time, uuid
modo, host, porto, numero = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
eu = "172.30.50.1"
sdp = ("v=0\r\no=- 1 1 IN IP4 %s\r\ns=-\r\nc=IN IP4 %s\r\nt=0 0\r\n"
       "m=audio 40000 RTP/SAVP 8 101\r\na=rtpmap:8 PCMA/8000\r\na=rtpmap:101 telephone-event/8000\r\n"
       "a=crypto:1 AES_CM_128_HMAC_SHA1_80 inline:WnD7c1ksDGs+dIefCEo8omPg4uO8DYIinNGL5yxQ\r\n") % (eu, eu)
via = "TLS" if modo == "tls" else "UDP"
msg = ("INVITE sip:%s@%s:%d SIP/2.0\r\nVia: SIP/2.0/%s %s:45070;branch=z9hG4bK%s;rport\r\n"
       "Max-Forwards: 70\r\nFrom: <sip:forjado@%s>;tag=%s\r\nTo: <sip:%s@%s>\r\n"
       "Call-ID: %s\r\nCSeq: 1 INVITE\r\nContact: <sip:forjado@%s:45070;transport=%s>\r\n"
       "X-Delonix-Central: pbx.central-b.example\r\nContent-Type: application/sdp\r\nContent-Length: %d\r\n\r\n%s"
       ) % (numero, host, porto, via, eu, uuid.uuid4().hex, eu, uuid.uuid4().hex[:8], numero, host,
            uuid.uuid4().hex, eu, modo, len(sdp), sdp)
if modo == "tls":
    ctx = ssl.create_default_context(); ctx.check_hostname = False; ctx.verify_mode = ssl.CERT_NONE
    s = ctx.wrap_socket(socket.create_connection((host, porto), timeout=10))
    s.sendall(msg.encode())
else:
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind((eu, 45070)); s.settimeout(10)
    s.sendto(msg.encode(), (host, porto))
final, fim = "", time.time() + 12
while time.time() < fim and not final:
    try:
        d = s.recv(8192).decode(errors="replace")
    except Exception:
        break
    for l in d.split("\r\n"):
        if l.startswith("SIP/2.0 ") and not l.startswith("SIP/2.0 1"):
            final = l.split(" ", 2)[1]
print(final or "sem-resposta")
PY
}

central() {
  mkdir -p "$ESTADO/central"
  docker image inspect "$IMG_BS" >/dev/null 2>&1 || docker build -q -t "$IMG_BS" -f voice/softphone/Containerfile voice/softphone >/dev/null
  semear_centrais || { bad "não consegui semear as duas organizações pela API ($API)"; return; }
  # shellcheck disable=SC1091
  . "$ESTADO/central.env"
  ok "duas organizações semeadas pela API: A ($A_DOMINIO, sala $A_SALA) e B ($B_DOMINIO, sala $B_SALA)"
  local saida antes log n uuid i

  echo "▶ quem não está na allowlist e não é uma central"
  saida=$(tentativa 172.30.50.20 udp 5060 "$A_UTIL" "$A_DOMINIO" "$A_PASS")
  if grep -q 'Call established' <<<"$saida"; then bad "sem TLS, com a conta certa: ENTROU"
  elif grep -qE '^SIP/2\.0 403' <<<"$saida"; then ok "sem TLS (UDP), mesmo com a conta certa: 403 — as chaves do SRTP não viajam em claro"
  else bad "sem TLS: não vi o 403 (estados: $(estado_sip "$saida"))"; fi
  saida=$(tentativa 172.30.50.20 tls 5061 "$A_UTIL" "$A_DOMINIO" -)
  if grep -q 'Call established' <<<"$saida"; then bad "por TLS, sem credenciais: ENTROU"
  elif grep -qE '^SIP/2\.0 407' <<<"$saida"; then ok "por TLS, sem credenciais: desafiada (407) e não entra"
  else bad "por TLS, sem credenciais: não vi o 407 (estados: $(estado_sip "$saida"))"; fi
  antes=$(falhas_de 172.30.50.21)
  saida=$(tentativa 172.30.50.21 tls 5061 "$A_UTIL" "$A_DOMINIO" "password-errada-0000")
  n=$(( $(falhas_de 172.30.50.21) - antes ))
  if grep -q 'Call established' <<<"$saida"; then bad "password errada: ENTROU"
  elif [ "$n" -ge 1 ]; then ok "password errada: não entra, e o bordo contou $n falha(s) dessa origem"
  else bad "password errada: não entrou, mas o bordo não contou a falha (estados: $(estado_sip "$saida"))"; fi
  antes=$(falhas_de 172.30.50.21)
  saida=$(tentativa 172.30.50.21 tls 5061 "$B_UTIL" "$A_DOMINIO" "$B_PASS")
  n=$(( $(falhas_de 172.30.50.21) - antes ))
  if grep -q 'Call established' <<<"$saida"; then bad "a conta de B no domínio de A: ENTROU"
  elif [ "$n" -ge 1 ]; then ok "a conta de B apresentada no domínio de A: não entra ($n falha(s) contadas)"
  else bad "a conta de B no domínio de A: sem falha contada (estados: $(estado_sip "$saida"))"; fi

  echo "▶ a central de A, com a conta de A e o PIN de uma sala de A (dois telefones)"
  antes=$(fs_log | wc -l)
  SOFTPHONE_PASSWORD_A=$A_PASS SOFTPHONE_PASSWORD_B=$A_PASS bash scripts/softphone-prova.sh par \
    --servidor "$BORDO:5061" --transporte tls --rede "$REDE" --dominio "$A_DOMINIO" \
    --utilizador-a "$A_UTIL" --utilizador-b "$A_UTIL" --destino "${NUMERO#+}" --pin "$A_PIN" \
    --espera-pin 6 --segundos 14 | sed 's/^/    /'
  [ "${PIPESTATUS[0]}" -eq 0 ] && ok "os dois telefones da central ouvem-se um ao outro, e nenhum se ouve a si" ||
    bad "a prova de áudio dos dois telefones falhou (acima)"
  log=$(fs_log | tail -n +$(( antes + 1 )))
  n=$(grep -ac "\[delonix ponte\] sala=$A_SALA -> " <<<"$log")
  [ "$n" -eq 2 ] && ok "FreeSWITCH: as duas chamadas foram para a ponte da sala $A_SALA (ADR-0010)" ||
    bad "FreeSWITCH: $n chamadas para a ponte da sala $A_SALA, esperava 2"
  [ "$(grep -ac 'cai na conferencia local' <<<"$log")" -eq 0 ] && [ "$(grep -ac "conference($A_SALA@" <<<"$log")" -eq 0 ] &&
    ok "nenhuma caiu na conferência local do FreeSWITCH: a central está na sala do SFU" ||
    bad "houve chamadas que caíram na conferência local (a ponte não atendeu)"
  [ "$(grep -ac "\"pin\":\"$A_PIN\"" <<<"$log")" -eq 0 ] && ok "o PIN marcado não ficou no freeswitch.log" || bad "o PIN marcado ficou no freeswitch.log"

  echo "▶ o isolamento: a central de A com o PIN de uma sala de B"
  antes=$(fs_log | wc -l)
  SOFTPHONE_PASSWORD=$A_PASS bash scripts/softphone-prova.sh chamada \
    --servidor "$BORDO:5061" --transporte tls --rede "$REDE" --dominio "$A_DOMINIO" \
    --utilizador "$A_UTIL" --destino "${NUMERO#+}" --pin "$B_PIN" --espera-pin 6 --segundos 8 >/dev/null 2>&1
  log=$(fs_log | tail -n +$(( antes + 1 )))
  [ "$(grep -ac 'lua(dialin_ivr.lua)' <<<"$log")" -ge 1 ] || bad "a chamada de A com o PIN de B nem chegou ao IVR"
  [ "$(grep -ac '\[delonix ponte\]' <<<"$log")" -eq 0 ] && [ "$(grep -ac 'conference(' <<<"$log")" -eq 0 ] &&
    ok "a central de A com o PIN de uma sala de B: o IVR não a deixou entrar em sala nenhuma" ||
    bad "a central de A ENTROU com o PIN de uma sala de B"
  antes=$(fs_log | wc -l)
  SOFTPHONE_PASSWORD=$B_PASS bash scripts/softphone-prova.sh chamada \
    --servidor "$BORDO:5061" --transporte tls --rede "$REDE" --dominio "$B_DOMINIO" \
    --utilizador "$B_UTIL" --destino "${NUMERO#+}" --pin "$B_PIN" --espera-pin 6 --segundos 8 >/dev/null 2>&1
  log=$(fs_log | tail -n +$(( antes + 1 )))
  [ "$(grep -ac "\[delonix ponte\] sala=$B_SALA -> " <<<"$log")" -eq 1 ] &&
    ok "controlo positivo: o MESMO PIN, marcado pela central de B, abre a sala $B_SALA" ||
    bad "controlo positivo falhado: a central de B não entrou na sua sala com o PIN certo"

  echo "▶ um cabeçalho forjado («sou a central de B»)"
  # Pela porta do bordo, da origem que está na allowlist: o bordo tira o cabeçalho.
  antes=$(fs_log | wc -l)
  saida=$(forjar tls "$BORDO" 5061)
  sleep 2
  uuid=$(fs_cli "show channels" | grep 'forjado' | head -1 | cut -d, -f1)
  if [ "$saida" != 200 ] || [ -z "$uuid" ]; then
    bad "o INVITE forjado pelo bordo não foi atendido (estado $saida): o controlo não mediu nada"
  elif [ "$(fs_cli "uuid_getvar $uuid sip_h_X-Delonix-Central" | tr -d '[:space:]')" = "_undef_" ]; then
    ok "pelo bordo, da allowlist: a chamada entra SEM o cabeçalho — o bordo tirou-o (é dial-in por número, como sempre)"
  else
    bad "pelo bordo: o cabeçalho forjado chegou ao FreeSWITCH ($(fs_cli "uuid_getvar $uuid sip_h_X-Delonix-Central"))"
  fi
  [ -n "$uuid" ] && fs_cli "uuid_kill $uuid" >/dev/null
  # Direito ao FreeSWITCH, sem passar pelo bordo: o IVR desliga.
  saida=$(forjar udp 172.30.50.13 5080)
  log=$(fs_log | tail -n +$(( antes + 1 )))
  [ "$saida" != 200 ] && grep -aq 'que nao e o bordo' <<<"$log" &&
    ok "direito ao FreeSWITCH, sem passar pelo bordo: o IVR rejeita a chamada ($saida)" ||
    bad "direito ao FreeSWITCH com o cabeçalho forjado: estado $saida, e o IVR não disse que rejeitou"

  echo "▶ o travão por origem"
  for i in $(seq 1 12); do
    [ "$(falhas_de 172.30.50.23)" -ge 10 ] && break
    TENTATIVA_SEG=4 tentativa 172.30.50.23 tls 5061 "$A_UTIL" "$A_DOMINIO" "errada-$i" >/dev/null
  done
  n=$(falhas_de 172.30.50.23)
  saida=$(tentativa 172.30.50.23 tls 5061 "$A_UTIL" "$A_DOMINIO" "$A_PASS")
  [ "$n" -ge 10 ] && ! grep -q 'Call established' <<<"$saida" && grep -qE '^SIP/2\.0 403' <<<"$saida" &&
    ok "depois de $n falhas, a origem 172.30.50.23 leva 403 mesmo com a password certa" ||
    bad "o travão não fechou a origem ($n falhas; estados: $(estado_sip "$saida"))"
  saida=$(tentativa 172.30.50.24 tls 5061 "$A_UTIL" "$A_DOMINIO" "$A_PASS")
  grep -q 'Call established' <<<"$saida" && ok "outra origem (172.30.50.24) com a conta certa continua a entrar: o travão é por origem" ||
    bad "o travão apanhou outra origem (estados: $(estado_sip "$saida"))"
}

# ------------------------------------------------------------ browser
# Um browser na sala, e a central a entrar nela pela ponte. Os dois sentidos:
# o softphone mede os 440 Hz do browser; o browser mede os 1000 Hz da central
# (web/e2e/telefone-na-sala.mjs).
PORTO_VITE=${PBX_PROVA_VITE_PORT:-5199}
# Controlo negativo do sentido central → browser: PBX_PROVA_TOM_CENTRAL=700 põe a
# central a tocar OUTRO tom; o browser, que procura os 1000 Hz, tem de falhar.
browser() {
  command -v node >/dev/null 2>&1 && [ -d web/node_modules/@playwright ] ||
    { echo "✗ precisa de node e das dependências do frontend: (cd web && npm ci)"; exit 1; }
  mkdir -p "$ESTADO/central"
  docker image inspect "$IMG_BS" >/dev/null 2>&1 || docker build -q -t "$IMG_BS" -f voice/softphone/Containerfile voice/softphone >/dev/null
  semear_centrais || { bad "não consegui semear as duas organizações pela API ($API)"; return; }
  # shellcheck disable=SC1091
  . "$ESTADO/central.env"
  [ -n "${A_ADMIN:-}" ] || { bad "a réplica foi semeada antes de existir este modo: desce-a (down) e volta a subi-la (up)"; return; }

  echo "▶ o arnês do cliente (Vite, com a API da réplica por trás) e o browser na sala $A_SALA"
  ( cd web && API_HOST=172.30.50.12 API_PORT=8180 NO_HTTPS=1 PORT=$PORTO_VITE exec node node_modules/vite/bin/vite.js --strictPort ) \
    > "$ESTADO/vite.log" 2>&1 &
  local vite=$! nav i
  for i in $(seq 1 60); do
    [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 "http://localhost:$PORTO_VITE/e2e/harness.html")" = 200 ] && break
    kill -0 "$vite" 2>/dev/null || break
    sleep 2
  done
  [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 "http://localhost:$PORTO_VITE/e2e/harness.html")" = 200 ] ||
    { bad "o Vite não serviu o arnês em localhost:$PORTO_VITE"; tail -5 "$ESTADO/vite.log" | sed 's/^/       /'; kill "$vite" 2>/dev/null; return; }
  ( cd web && API="$API" APP="http://localhost:$PORTO_VITE" EMAIL="$A_ADMIN" PASSWORD="$A_ADMIN_PW" SALA="$A_SALA" \
      node e2e/telefone-na-sala.mjs ) > "$ESTADO/browser.log" 2>&1 &
  nav=$!
  for i in $(seq 1 150); do
    grep -q '^BROWSER-NA-SALA' "$ESTADO/browser.log" 2>/dev/null && break
    kill -0 "$nav" 2>/dev/null || break
    sleep 2
  done
  if ! grep -q '^BROWSER-NA-SALA' "$ESTADO/browser.log" 2>/dev/null; then
    bad "o browser não chegou a entrar na sala"
    tail -8 "$ESTADO/browser.log" | sed 's/^/       /'
    kill "$nav" "$vite" 2>/dev/null; return
  fi
  ok "o browser está na sala $A_SALA, a tocar 440 Hz"

  echo "▶ a central liga, autentica-se no bordo, marca o PIN e toca 1000 Hz"
  local antes log sp nv
  antes=$(fs_log | wc -l)
  SOFTPHONE_PASSWORD=$A_PASS bash scripts/softphone-prova.sh chamada \
    --servidor "$BORDO:5061" --transporte tls --rede "$REDE" --dominio "$A_DOMINIO" \
    --utilizador "$A_UTIL" --destino "${NUMERO#+}" --pin "$A_PIN" \
    --espera-pin 6 --segundos 30 --tom "${PBX_PROVA_TOM_CENTRAL:-1000}" --espera-tom 440 | sed 's/^/    /'
  sp=${PIPESTATUS[0]}
  wait "$nav"; nv=$?
  kill "$vite" 2>/dev/null
  grep -v '^BROWSER-NA-SALA' "$ESTADO/browser.log" | sed 's/^/    /'

  log=$(fs_log | tail -n +$(( antes + 1 )))
  [ "$(grep -ac "\[delonix ponte\] sala=$A_SALA -> " <<<"$log")" -eq 1 ] && [ "$(grep -ac 'cai na conferencia local' <<<"$log")" -eq 0 ] &&
    ok "a central entrou na sala $A_SALA pela ponte do SFU (ADR-0010), sem recuo para a conferência local" ||
    bad "a chamada da central não foi para a ponte da sala $A_SALA, ou a ponte recusou-a"
  [ "$sp" -eq 0 ] && ok "browser → central: a central ouviu os 440 Hz do browser, e não o seu próprio tom" ||
    bad "browser → central: a central não ouviu o tom do browser (acima)"
  [ "$nv" -eq 0 ] && ok "central → browser: o browser descodificou o áudio da central, e é o tom dela (1000 Hz)" ||
    bad "central → browser: o browser não ouviu a central (acima)"
}

down() { "${COMPOSE[@]}" down -v >/dev/null 2>&1; rm -f "$ESTADO/central.env"; rm -rf "$ESTADO/central"; echo "réplica desmontada"; }

[ $# -ge 1 ] || uso 2
modo=$1; shift
case "$modo" in
  up) up ;;
  freepbx) freepbx "$@" ;;
  negativos) negativos ;;
  longa) longa ;;
  central) central ;;
  browser) browser ;;
  down) down ;;
  -h|--help) uso 0 ;;
  *) echo "✗ modo: $modo"; uso 2 ;;
esac
[ "$modo" = down ] && exit 0
[ "$fail" -eq 0 ] && echo "✓ prova do tronco ($modo): tudo medido" || echo "✗ prova do tronco ($modo): há falhas acima"
exit "$fail"
