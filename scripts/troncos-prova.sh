#!/usr/bin/env bash
# ============================================================
#  Prova dos TRONCOS na configuração que corre (ADR-0009, plano de lacunas T1).
#
#  Ergue uma réplica (voice/troncos-prova/compose.yaml — projecto, rede e
#  endereços próprios, sem portas publicadas): o servidor, o FreeSWITCH do
#  Meet arrancado pelo voice/cluster/freeswitch-entrypoint.sh com os ficheiros
#  que o compose.yaml da raiz põe em /meet, e uma operadora de ENSAIO (o
#  FreeSWITCH vanilla, com contas e um plano que atende, dá ocupado ou recusa).
#
#  Depois faz o caminho de um administrador, pela API, e mede o FreeSWITCH:
#
#    1. um tronco criado na consola aparece e REGISTA-SE na operadora sem
#       ninguém reiniciar nada;
#    2. uma chamada pelo plano de marcação sai por esse tronco, e o registo
#       dela chega ao servidor com duração, custo e qualidade (MOS);
#    3. ocupado e emergência (112: nunca gravada, sem limite de canais);
#       e um RAMAL a sério — um softphone autenticado, com SRTP — marca para a
#       rede pública e para o 112, com áudio medido nos dois sentidos; um
#       número sem regra, uma password errada e o ramal de outra organização
#       não saem;
#    4. um número sem regra não sai, e não deixa registo;
#    5. uma chamada que não é de tronco (o IVR do dial-in) não deixa registo
#       nem fica em disco à espera;
#    6. reiniciar o FreeSWITCH — e reiniciá-lo com o servidor EM BAIXO — não
#       deixa a instalação sem troncos;
#    7. o utilizador de um tronco não consegue ler as variáveis do FreeSWITCH
#       (o segredo de voz) e mandá-las para o seu servidor SIP;
#    8. nenhum registo ficou por entregar, e nem o segredo de voz nem a
#       password do tronco ficam no log.
#
#  Uso:
#    bash scripts/troncos-prova.sh            ergue, mede, desmonta
#    bash scripts/troncos-prova.sh up|mede|down   um passo de cada vez
#
#    SERVER_IMAGE=<imagem>   o servidor (por omissão delonix-server:latest, a
#                            do `make image BUILDER=docker` — com o delonix
#                            instalado, o `make image` constrói para o store
#                            dele e o docker não a vê);
#    SERVER_BIN=<binário>    em vez da imagem: embrulha o binário da tua árvore
#                            numa imagem descartável (base SERVER_BIN_BASE,
#                            por omissão ubuntu:24.04);
#    FS_IMAGE=<imagem>       o FreeSWITCH (por omissão delonix-meet/freeswitch:1.11.3).
#
#  O que NÃO mede:
#    - uma CENTRAL a marcar para fora: só o ramal chega ao plano de marcação.
#    - ramal para ramal, e o número de acesso às reuniões: precisam de dois
#      telefones registados (o `softphone-prova.sh par` mede-o contra a sala).
#    - uma operadora de verdade: SRTP, TLS, NAT, DTMF, identidade do chamador.
#    - um tronco ALTERADO ou APAGADO: precisa de `killgw`, que o servidor manda
#      pelo ESL, fechado nesta configuração (T11).
#    - o estado do registo na consola e a «chamada de teste», pela mesma razão.
#    - o chart e o cluster: a lista de ficheiros é a do compose.yaml.
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
ESTADO=$PWD/.troncos-prova
COMPOSE=(docker compose --env-file "$ESTADO/.env" -f voice/troncos-prova/compose.yaml)
API=http://172.30.51.12:8180
OPERADORA=172.30.51.20
RESCAN=10
fail=0
ok()  { printf '  ✓ %s\n' "$*"; }
bad() { printf '  ✗ %s\n' "$*"; fail=1; }
uso() { sed -n '2,54p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-2}"; }
export ESTADO

segredo() { head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n'; }
psql_() { "${COMPOSE[@]}" exec -T postgres psql -v ON_ERROR_STOP=1 -U delonix -d delonix -qAt "$@"; }
fs_cli() { "${COMPOSE[@]}" exec -T freeswitch sh -c 'P=$(sed -n "s/.*name=\"password\" value=\"\([^\"]*\)\".*/\1/p" /conf/autoload_configs/event_socket.conf.xml); /usr/local/freeswitch/bin/fs_cli -p "$P" -x "$0"' "$1" 2>/dev/null; }
fs_log() { "${COMPOSE[@]}" exec -T freeswitch cat /usr/local/freeswitch/var/log/freeswitch/freeswitch.log 2>/dev/null; }
env_() { sed -n "s/^$1=//p" "$ESTADO/.env"; }

# api <método> <caminho> [corpo JSON] — imprime «<estado> <corpo>»; o token vem de $TOKEN
api() {
  python3 - "$API" "$1" "$2" "${3:-}" "${TOKEN:-}" <<'PY'
import json, sys, urllib.request, urllib.error
base, metodo, caminho, corpo, token = sys.argv[1:6]
req = urllib.request.Request(base + caminho, method=metodo, data=corpo.encode() if corpo else None)
if corpo: req.add_header("Content-Type", "application/json")
if token: req.add_header("Authorization", "Bearer " + token)
try:
    with urllib.request.urlopen(req, timeout=15) as r:
        print(r.status, r.read().decode() or "null")
except urllib.error.HTTPError as e:
    print(e.code, e.read().decode() or "null")
except Exception as e:
    print(0, json.dumps(str(e)))
PY
}
# campo <json> <caminho.com.pontos>
campo() { python3 -c 'import json,sys
v=json.loads(sys.argv[1])
for k in sys.argv[2].split("."):
    v = v[int(k)] if isinstance(v, list) else v.get(k) if isinstance(v, dict) else None
    if v is None: break
print("" if v is None else v)' "$1" "$2"; }

# estado_gw <gateway> — o estado do registo (REGED, TRYING, FAIL_WAIT, NOREG…), vazio se não existe
estado_gw() { fs_cli "sofia status gateway $1" | sed -n 's/^State[[:space:]]\{1,\}\([A-Z_]\{1,\}\).*/\1/p' | head -1; }
# espera_gw <gateway> <estado> <segundos>
espera_gw() {
  local i e=""
  for i in $(seq 1 "$3"); do e=$(estado_gw "$1"); [ "$e" = "$2" ] && return 0; sleep 1; done
  echo "${e:-não existe}"; return 1
}
# registos — quantos registos de chamada tem a organização
registos() { psql_ -c "SELECT count(*) FROM telephony_call_records WHERE org_id='$ORG'"; }
# espera_registos <n> <segundos>
espera_registos() { local i; for i in $(seq 1 "$2"); do [ "$(registos)" -ge "$1" ] && return 0; sleep 1; done; return 1; }
# marca <número> — uma chamada pelo plano de marcação da organização. Nasce num
# canal `loopback`, que não tem telefone: sem fixar o codec e o levar até à
# perna do tronco (`export_vars`), ela oferecia L16, que operadora nenhuma aceita.
marca() {
  fs_cli "originate {origination_caller_id_number=244222000000,delonix_org_id=$ORG,delonix_cdr_skip=true,originate_timeout=20,absolute_codec_string=PCMA,export_vars=absolute_codec_string}loopback/$1/delonix-outbound &park()" | tr -d '\r' | grep -a -E '^[+-](OK|ERR)' | tail -1
}
# chamada_longa <password> <utilizador> <domínio> <ficheiro de saída> — um
# softphone a ligar para 923000888 (a operadora atende 25 s), em fundo. Devolve
# em $SOFT o contentor dele quando a chamada está estabelecida (vazio se não).
chamada_longa() {
  local i
  SOFT=
  ( SOFTPHONE_PASSWORD=$1 bash scripts/softphone-prova.sh chamada --servidor 172.30.51.13:5070 --rede troncosprova_troncos \
      --utilizador "$2" --dominio "$3" --destino 923000888 --segundos 14 > "$4" 2>&1 ) &
  for i in $(seq 1 90); do
    SOFT=$(docker ps --format '{{.Names}}' | grep -E '^sp[0-9]+-a$' | head -1)
    [ -n "$SOFT" ] && docker logs "$SOFT" 2>&1 | grep -q 'Call established' && return 0
    sleep 0.5
  done
  SOFT=; return 1
}
espera_perfil() {
  local i
  for i in $(seq 1 90); do fs_cli "sofia status" | grep -Eq 'external[[:space:]]+profile.*RUNNING' && return 0; sleep 2; done
  return 1
}

# ------------------------------------------------------------ up
up() {
  local img=${SERVER_IMAGE:-delonix-server:latest} f nome n=0 i
  mkdir -p "$ESTADO"; chmod 700 "$ESTADO"
  if [ -n "${SERVER_BIN:-}" ]; then
    [ -x "$SERVER_BIN" ] || { echo "✗ SERVER_BIN=$SERVER_BIN não é um executável"; exit 1; }
    img=delonix-server:troncos-prova
    rm -rf "$ESTADO/imagem"; mkdir -p "$ESTADO/imagem"
    cp "$SERVER_BIN" "$ESTADO/imagem/delonix-server"
    printf 'FROM %s\nCOPY delonix-server /app/delonix-server\nUSER 65532:65532\nENTRYPOINT ["/app/delonix-server"]\n' \
      "${SERVER_BIN_BASE:-ubuntu:24.04}" > "$ESTADO/imagem/Dockerfile"
    docker build -q -t "$img" "$ESTADO/imagem" >/dev/null || { echo "✗ não consegui embrulhar $SERVER_BIN numa imagem"; exit 1; }
    rm -rf "$ESTADO/imagem"
  fi
  docker image inspect "$img" >/dev/null 2>&1 || { echo "✗ o docker não tem a imagem do servidor $img (make image BUILDER=docker, SERVER_IMAGE=… ou SERVER_BIN=…)"; exit 1; }
  docker image inspect "${FS_IMAGE:-delonix-meet/freeswitch:1.11.3}" >/dev/null 2>&1 ||
    { echo "✗ falta a imagem do FreeSWITCH (make freeswitch-image, ou FS_IMAGE=…)"; exit 1; }
  ( umask 077
    printf 'POSTGRES_PASSWORD=%s\nJWT_SECRET=%s\nTURN_SECRET=%s\nPROVISIONING_SECRET=%s\nVOICE_INTERNAL_SECRET=%s\nOPERADORA_PASSWORD=%s\nADMIN_PASSWORD=Pr0va-%s\n' \
      "$(segredo)" "$(segredo)" "$(segredo)" "$(segredo)" "$(segredo)" "$(segredo)" "$(segredo)" > "$ESTADO/.env"
    printf 'DATA_ENCRYPTION_KEYS=prova:%s\nESTADO=%s\nSERVER_IMAGE=%s\nRESCAN_SECS=%s\n' \
      "$(head -c 32 /dev/urandom | base64 -w0)" "$ESTADO" "$img" "$RESCAN" >> "$ESTADO/.env" )
  # Os ficheiros de /meet: os que o compose.yaml da raiz monta, e mais nenhum.
  rm -rf "$ESTADO/meet" "$ESTADO/entrypoint"; mkdir -p "$ESTADO/meet" "$ESTADO/entrypoint"
  while read -r f nome; do
    [ -f "$f" ] || { echo "✗ o compose.yaml monta $f, e o ficheiro não existe"; exit 1; }
    cp "$f" "$ESTADO/meet/$nome"; n=$(( n + 1 ))
  done < <(sed -n 's#^ *- \./\(voice/[^:]*\):/meet/\([^:]*\)\(:ro\)\{0,1\} *$#\1 \2#p' compose.yaml)
  [ "$n" -gt 0 ] || { echo "✗ não encontrei os ficheiros que o compose.yaml monta em /meet"; exit 1; }
  cp voice/cluster/freeswitch-entrypoint.sh "$ESTADO/entrypoint/"
  chmod -R a+rX "$ESTADO/meet" "$ESTADO/entrypoint"
  echo "configuração: a que o voice/cluster/freeswitch-entrypoint.sh monta, com os $n ficheiros que o compose.yaml põe em /meet"
  echo "servidor: $img"
  "${COMPOSE[@]}" up -d >/dev/null 2>&1 || { echo "✗ a réplica não arrancou"; "${COMPOSE[@]}" up -d; exit 1; }
  for i in $(seq 1 60); do [ "$(api GET /api/status | cut -d' ' -f1)" = 200 ] && break; sleep 2; done
  [ "$(api GET /api/status | cut -d' ' -f1)" = 200 ] && ok "servidor a responder" ||
    { bad "o servidor não respondeu em $API"; "${COMPOSE[@]}" logs --tail 20 server; return; }
  espera_perfil && ok "FreeSWITCH do Meet: perfil external a correr" ||
    { bad "o FreeSWITCH não ficou pronto"; "${COMPOSE[@]}" logs --tail 30 freeswitch; return; }
  for m in mod_json_cdr mod_xml_curl mod_hash; do
    fs_cli "module_exists $m" | grep -q true || bad "o módulo $m não está carregado"
  done
  [ "$fail" = 0 ] && ok "mod_json_cdr, mod_xml_curl e mod_hash carregados"
}

# ------------------------------------------------------------ mede
mede() {
  local r st corpo senha op trunk gw e antes v pendentes=0 SOFT=
  senha=$(env_ ADMIN_PASSWORD); op=$(env_ OPERADORA_PASSWORD)

  echo "1) o administrador cria a organização, o tronco e o plano de marcação — pela API"
  r=$(api POST /api/auth/register "{\"org_name\":\"Prova dos troncos\",\"email\":\"admin@troncos.invalid\",\"username\":\"admin-troncos\",\"password\":\"$senha\"}")
  r=$(api POST /api/auth/login "{\"email\":\"admin@troncos.invalid\",\"password\":\"$senha\"}")
  TOKEN=$(campo "${r#* }" token); [ -n "$TOKEN" ] || TOKEN=$(campo "${r#* }" access_token)
  [ -n "$TOKEN" ] || { bad "não consegui entrar como administrador (${r%% *})"; return; }
  r=$(api GET /api/orgs); ORG=$(campo "${r#* }" 0.id)
  [ -n "$ORG" ] || { bad "a conta não tem organização"; return; }
  # Antes de haver tronco: o perfil não tem gateways da plataforma.
  antes=$(fs_cli "sofia profile external gwlist" | grep -c 'dlx-')
  r=$(api POST "/api/orgs/$ORG/telephony/trunks" "{\"name\":\"Operadora de ensaio\",\"short_code\":\"ENS\",\"host\":\"$OPERADORA\",\"port\":5060,\"transport\":\"udp\",\"srtp\":\"off\",\"register\":true,\"username\":\"1000\",\"password\":\"$op\",\"max_channels\":2,\"price_per_min\":{\"amount\":\"9.40\",\"currency\":\"AOA\"}}")
  st=${r%% *}; corpo=${r#* }; trunk=$(campo "$corpo" id)
  [ "$st" = 201 ] && [ -n "$trunk" ] || { bad "a API não criou o tronco ($st $corpo)"; return; }
  gw="dlx-$trunk"
  r=$(api PUT "/api/orgs/$ORG/telephony/dial-plan" "{\"rules\":[{\"pattern\":\"9XXXXXXXX\",\"description\":\"Móvel nacional\",\"action\":\"external\",\"trunk_id\":\"$trunk\"},{\"pattern\":\"112\",\"description\":\"Emergência\",\"action\":\"external\",\"trunk_id\":\"$trunk\",\"emergency\":true}]}")
  [ "${r%% *}" = 200 ] || { bad "a API não aceitou o plano de marcação ($r)"; return; }
  ok "organização, tronco ($gw, com registo na operadora) e plano de marcação criados; o perfil tinha $antes gateway(s) da plataforma"

  echo "2) o tronco aparece no FreeSWITCH e regista-se na operadora, sem ninguém reiniciar nada"
  if e=$(espera_gw "$gw" REGED $(( RESCAN * 3 + 20 ))); then ok "gateway $gw: REGED (o ciclo volta a ler os troncos de $RESCAN em $RESCAN s)"
  else bad "gateway $gw não ficou registado em $(( RESCAN * 3 + 20 )) s (estado: $e)"; fs_log | grep -a "$gw" | tail -5 | sed 's/^/       /'; return; fi
  v=$("${COMPOSE[@]}" exec -T operadora fs_cli -x "show registrations" 2>/dev/null | grep -c '^1000,')
  [ "${v:-0}" -ge 1 ] && ok "a operadora de ensaio tem o registo da conta 1000" || bad "a operadora não tem o registo do tronco"

  echo "3) uma chamada pelo plano de marcação sai pelo tronco, e o registo chega com duração, custo e qualidade"
  r=$(marca 923000111)
  case "$r" in +OK*) ok "chamada para 923000111 atendida pela operadora" ;; *) bad "a chamada para 923000111 não foi atendida ($r)"; fs_log | grep -a 'Originate Failed\|Hangup sofia' | tail -4 | sed 's/^/       /' ;; esac
  espera_registos 1 25 || bad "o registo da chamada não chegou ao servidor em 25 s"
  r=$(psql_ -F'|' -c "SELECT outcome, trunk_id, to_number, billsec, coalesce(cost_e4::text,'-'), coalesce(cost_currency,'-'), coalesce(round(mos::numeric,2)::text,'-'), coalesce(round(jitter_ms::numeric,2)::text,'-'), coalesce(round(loss_pct::numeric,2)::text,'-'), direction, recorded, emergency FROM telephony_call_records WHERE org_id='$ORG' ORDER BY started_at DESC LIMIT 1")
  IFS='|' read -r c_out c_trunk c_to c_bill c_custo c_moeda c_mos c_jit c_perda c_dir c_grav c_emerg <<<"$r"
  echo "       registo: resultado=$c_out destino=$c_to facturados=${c_bill}s custo=$c_custo/10000 $c_moeda MOS=$c_mos jitter=${c_jit}ms perda=${c_perda}% sentido=$c_dir"
  [ "$c_out" = answered ] && [ "$c_trunk" = "$trunk" ] && [ "$c_dir" = outbound ] && ok "registo: atendida, de saída, atribuída ao tronco" ||
    bad "o registo não diz «atendida, de saída, por este tronco» ($r)"
  [ "${c_bill:-0}" -ge 3 ] && ok "duração facturável: ${c_bill} s (a operadora toca 5 s)" || bad "duração facturável de ${c_bill:-?} s — esperava perto de 5"
  # 9,40 AOA por minuto (94000 décimos de milésimo), ao segundo ou ao minuto:
  # tem de ser maior que zero e não passar do preço de um minuto.
  if [ "$c_custo" != - ] && [ "$c_custo" -gt 0 ] && [ "$c_custo" -le 94000 ] && [ "$c_moeda" = AOA ]; then
    ok "custo congelado ao preço do tronco: $(python3 -c "print('%.4f' % ($c_custo / 10000))") AOA"
  else bad "custo «$c_custo/10000 $c_moeda» — esperava mais de 0 e até 9,40 AOA"; fi
  [ "$c_mos" != - ] && ok "qualidade medida na perna do tronco: MOS $c_mos" || bad "o registo não traz o MOS"
  # A API devolve o mesmo que a base — é por ela que a consola o lê.
  r=$(api GET "/api/orgs/$ORG/telephony/call-records")
  [ "${r%% *}" = 200 ] && [ "$(campo "${r#* }" items.0.outcome)" = answered ] && ok "GET …/telephony/call-records mostra a chamada ao administrador" ||
    bad "a API não mostra o registo (${r%% *})"

  echo "4) ocupado e emergência"
  antes=$(registos); r=$(marca 923000000)
  espera_registos $(( antes + 1 )) 20
  v=$(psql_ -c "SELECT outcome || ' ' || coalesce(cost_e4::text,'-') FROM telephony_call_records WHERE org_id='$ORG' AND to_number LIKE '%923000000'")
  case "$v" in busy*) ok "número ocupado: registo «$v» (sem custo)" ;; *) bad "o ocupado ficou registado como «$v» (o originate disse: $r)" ;; esac
  antes=$(registos); r=$(marca 112)
  espera_registos $(( antes + 1 )) 25
  v=$(psql_ -F' ' -c "SELECT outcome, emergency, recorded FROM telephony_call_records WHERE org_id='$ORG' AND to_number='112'")
  [ "$v" = "answered t f" ] && ok "112: atendida, marcada como emergência e não gravada" ||
    bad "o 112 ficou «$v» — esperava «answered t f» (o originate disse: $r)"
  v=$(fs_log | grep -ac 'record_session.*delonix-')
  [ "${v:-0}" -eq 0 ] && ok "nenhuma gravação começou" || bad "o FreeSWITCH começou $v gravação(ões)"

  echo "5) controlo negativo: um número sem regra não sai, e não deixa registo"
  antes=$(registos); r=$(marca 0044123456)
  sleep 4
  case "$r" in +OK*) bad "um número sem regra foi ATENDIDO ($r)" ;; *) ok "número sem regra: recusado (${r#-ERR })" ;; esac
  [ "$(registos)" = "$antes" ] && ok "e não ficou registo de chamada" || bad "ficou um registo de uma chamada que não saiu"

  echo "6) um ramal marca para a rede pública — um softphone a sério, autenticado, com SRTP"
  # Até aqui as chamadas nasceram dentro do FreeSWITCH. Agora marca um
  # telefone: um ramal criado pela API, que se autentica por digest no perfil
  # dos ramais. Quem decide que o número sai é o servidor, pelo plano de
  # marcação da organização do ramal AUTENTICADO (R292).
  local ramal dom_a pw_a ramal_b dom_b pw_b saida TOKEN_A=$TOKEN ORG_A=$ORG ORG_B
  r=$(api POST "/api/orgs/$ORG/extensions" '{"extension":"1001","label":"Recepção"}')
  ramal=$(campo "${r#* }" sip_username); dom_a=$(campo "${r#* }" sip_domain); pw_a=$(campo "${r#* }" sip_password)
  [ -n "$ramal" ] && [ -n "$pw_a" ] || { bad "a API não criou o ramal (${r%% *})"; return; }
  antes=$(registos)
  saida=$(SOFTPHONE_PASSWORD=$pw_a bash scripts/softphone-prova.sh chamada --servidor 172.30.51.13:5070 --rede troncosprova_troncos \
            --utilizador "$ramal" --dominio "$dom_a" --destino 923000444 --espera-tom 440 --tom 1000 --segundos 4 2>&1)
  grep -q 'a: chamada estabelecida' <<<"$saida" && grep -q 'media cifrada' <<<"$saida" &&
    ok "ramal 1001 → 923000444: atendida, com a media do ramal cifrada" ||
    { bad "a chamada do ramal para 923000444 não se estabeleceu"; grep -E '✗|session closed' <<<"$saida" | head -4 | sed 's/^/       /'; }
  grep -q 'ouviu os 440 Hz' <<<"$saida" && ok "o ramal ouviu o tom da operadora (440 Hz)" ||
    bad "o ramal NÃO ouviu o tom da operadora: $(grep -E 'Hz' <<<"$saida" | head -2 | tr '\n' ' ')"
  # O sentido contrário: o que a operadora gravou tem o tom do ramal (1000 Hz).
  sleep 3; rm -f "$ESTADO/operadora-ouviu.wav"
  docker cp "$("${COMPOSE[@]}" ps -q operadora)":/tmp/operadora-ouviu-244923000444.wav "$ESTADO/operadora-ouviu.wav" >/dev/null 2>&1
  v=$(bash scripts/softphone-prova.sh medir "$ESTADO/operadora-ouviu.wav" 1000 2>/dev/null | tail -1)
  python3 -c "import sys; sys.exit(0 if float(sys.argv[1] or 0) >= 0.03 else 1)" "${v:-0}" 2>/dev/null &&
    ok "a operadora ouviu o tom do ramal (1000 Hz, amplitude $v): áudio nos dois sentidos" ||
    bad "a operadora NÃO ouviu o tom do ramal (amplitude «${v:-sem gravação}»)"
  espera_registos $(( antes + 1 )) 25
  v=$(psql_ -F' ' -c "SELECT outcome, from_number, (cost_e4 > 0), coalesce(round(mos::numeric,1)::text,'-') FROM telephony_call_records WHERE org_id='$ORG' AND to_number LIKE '%923000444'")
  case "$v" in "answered 1001 t "*) ok "registo: atendida, de «1001» (o número curto, não o utilizador SIP), com custo e MOS ${v##* }" ;;
    *) bad "o registo da chamada do ramal ficou «$v» — esperava «answered 1001 t <MOS>»" ;; esac
  v=$("${COMPOSE[@]}" exec -T operadora sh -c "grep -a -c -F '$ramal' /usr/local/freeswitch/var/log/freeswitch/freeswitch.log" 2>/dev/null | tr -d '[:space:]')
  [ "${v:-1}" = 0 ] && ok "a operadora nunca viu o utilizador SIP do ramal" || bad "o utilizador SIP do ramal chegou à operadora ($v linha(s) do log dela)"

  antes=$(registos)
  saida=$(SOFTPHONE_PASSWORD=$pw_a bash scripts/softphone-prova.sh chamada --servidor 172.30.51.13:5070 --rede troncosprova_troncos \
            --utilizador "$ramal" --dominio "$dom_a" --destino 112 --espera-tom 440 --segundos 3 2>&1)
  espera_registos $(( antes + 1 )) 25
  v=$(psql_ -F' ' -c "SELECT count(*), bool_and(emergency), bool_or(recorded) FROM telephony_call_records WHERE org_id='$ORG' AND to_number='112' AND from_number='1001'")
  grep -q 'a: chamada estabelecida' <<<"$saida" && [ "$v" = "1 t f" ] && ok "ramal 1001 → 112: sai, marcada como emergência, não gravada" ||
    bad "o 112 marcado pelo ramal: registo «$v», softphone: $(grep -E '✗' <<<"$saida" | head -1)"

  # Uma transferência cega pedida pelo ramal a meio da chamada. Aceite, a
  # perna do TRONCO voltava a passar pelo plano de marcação: saía uma segunda
  # chamada para onde o ramal mandasse, e a primeira ficava marcada para o
  # servidor ignorar — cobrável e sem registo.
  antes=$(registos)
  if chamada_longa "$pw_a" "$ramal" "$dom_a" "$ESTADO/transferencia.out"; then
    sleep 2
    docker exec "$SOFT" sh -c "printf '/transfer sip:923000999@$dom_a\n' | nc -u -w1 127.0.0.1 55551" >/dev/null 2>&1
    wait; sleep 8
    v=$(psql_ -c "SELECT count(*) FROM telephony_call_records WHERE to_number LIKE '%923000999'")
    r=$("${COMPOSE[@]}" exec -T operadora sh -c "grep -a -c '923000999' /usr/local/freeswitch/var/log/freeswitch/freeswitch.log" 2>/dev/null | tr -d '[:space:]')
    [ "$v" = 0 ] && [ "${r:-1}" = 0 ] && ok "transferência cega pedida pelo ramal: recusada — nenhuma chamada saiu para o número pedido" ||
      bad "a transferência cega pedida pelo ramal fez sair uma chamada ($v registo(s); $r linha(s) no log da operadora)"
    espera_registos $(( antes + 1 )) 20
    v=$(psql_ -c "SELECT count(*) FROM telephony_call_records WHERE org_id='$ORG' AND to_number LIKE '%923000888' AND outcome='answered'")
    [ "$v" = 1 ] && ok "e a chamada em que foi pedida deixou o seu registo" || bad "a chamada em que a transferência foi pedida ficou SEM registo ($v)"
  else wait; bad "a chamada longa do ramal não se estabeleceu — a transferência não foi medida"; grep -E '✗' "$ESTADO/transferencia.out" | head -2 | sed 's/^/       /'; fi

  # Controlos negativos: o que NÃO pode sair.
  antes=$(registos)
  saida=$(SOFTPHONE_PASSWORD=$pw_a bash scripts/softphone-prova.sh chamada --servidor 172.30.51.13:5070 --rede troncosprova_troncos \
            --utilizador "$ramal" --dominio "$dom_a" --destino 0044123456 --segundos 2 2>&1)
  grep -q 'NÃO se estabeleceu' <<<"$saida" && ok "número sem regra no plano: o ramal não sai ($(grep -o 'session closed: .*' <<<"$saida" | head -1 | sed 's/session closed: //'))" ||
    bad "um número SEM regra no plano de marcação saiu a pedido de um ramal"
  saida=$(SOFTPHONE_PASSWORD=errada-de-proposito bash scripts/softphone-prova.sh chamada --servidor 172.30.51.13:5070 --rede troncosprova_troncos \
            --utilizador "$ramal" --dominio "$dom_a" --destino 923000555 --segundos 2 2>&1)
  grep -q 'NÃO se estabeleceu' <<<"$saida" && ok "password errada: o ramal não sai ($(grep -o 'session closed: .*' <<<"$saida" | head -1 | sed 's/session closed: //'))" ||
    bad "um ramal com a password ERRADA fez uma chamada para fora"
  # Outra organização, sem troncos nem plano: o ramal dela não sai pelos de A.
  r=$(api POST /api/auth/register "{\"org_name\":\"Vizinha\",\"email\":\"admin@vizinha.invalid\",\"username\":\"admin-vizinha\",\"password\":\"$senha\"}")
  r=$(api POST /api/auth/login "{\"email\":\"admin@vizinha.invalid\",\"password\":\"$senha\"}")
  TOKEN=$(campo "${r#* }" access_token)
  r=$(api GET /api/orgs); ORG_B=$(campo "${r#* }" 0.id)
  r=$(api POST "/api/orgs/$ORG_B/extensions" '{"extension":"1001","label":"Recepção"}')
  ramal_b=$(campo "${r#* }" sip_username); dom_b=$(campo "${r#* }" sip_domain); pw_b=$(campo "${r#* }" sip_password)
  TOKEN=$TOKEN_A
  if [ -n "$ORG_B" ] && [ "$ORG_B" != "$ORG_A" ] && [ -n "$pw_b" ]; then
    saida=$(SOFTPHONE_PASSWORD=$pw_b bash scripts/softphone-prova.sh chamada --servidor 172.30.51.13:5070 --rede troncosprova_troncos \
              --utilizador "$ramal_b" --dominio "$dom_b" --destino 923000666 --segundos 2 2>&1)
    v=$(psql_ -c "SELECT count(*) FROM telephony_call_records WHERE to_number LIKE '%923000666'")
    grep -q 'NÃO se estabeleceu' <<<"$saida" && [ "$v" = 0 ] && ok "o ramal de OUTRA organização, sem plano, não sai pelos troncos desta" ||
      bad "o ramal de outra organização saiu pelos troncos desta ($v registo(s))"
  else bad "não consegui criar a segunda organização e o ramal dela — o isolamento não foi medido"; fi
  [ "$(registos)" = "$antes" ] && ok "e nenhuma das três recusas deixou registo de chamada" || bad "uma chamada recusada deixou registo"

  echo "7) uma chamada que NÃO é de tronco — o IVR do dial-in — é entregue, aceite e ignorada"
  # Só a perna de um tronco pede registo; a do IVR desliga-o. Sobra a perna
  # que nasce dentro do FreeSWITCH, sem organização nem tronco: um servidor que
  # a recusasse (422) fazia o módulo tentar outra vez e guardá-la em disco.
  antes=$(registos)
  fs_cli "originate {originate_timeout=10,absolute_codec_string=PCMA}loopback/244923000000/public &park()" >/dev/null
  sleep 2; fs_cli "hupall" >/dev/null; sleep 10
  v=$("${COMPOSE[@]}" exec -T freeswitch sh -c 'ls /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes 2>/dev/null | wc -l' | tr -d '[:space:]')
  r=$(fs_log | grep -ac 'lua(dialin_ivr.lua)')
  if [ "${r:-0}" -ge 1 ] && [ "${v:-1}" = 0 ] && [ "$(registos)" = "$antes" ]; then ok "o IVR correu, o registo foi aceite (nada em disco) e não ficou guardado"
  else bad "chamada ao IVR: o IVR correu $r vez(es), ficaram $v registo(s) em disco e $(( $(registos) - antes )) guardado(s) — esperava ≥1, 0 e 0"; fi

  echo "8) reiniciar o FreeSWITCH não deixa a instalação sem troncos"
  "${COMPOSE[@]}" restart freeswitch >/dev/null 2>&1; espera_perfil
  if e=$(espera_gw "$gw" REGED $(( RESCAN * 3 + 30 ))); then ok "depois de reiniciar: $gw REGED"
  else bad "depois de reiniciar o gateway não voltou (estado: $e)"; fi
  # O servidor MORRE a meio de uma chamada de um ramal: o registo da perna do
  # tronco não tem a quem ser entregue e fica em disco. É o controlo do «nada
  # por entregar» do passo 10 — e a única maneira de LER um registo destes: a
  # perna do tronco recebe do FreeSWITCH uma cópia do SDP que o ramal ofereceu,
  # com a chave SRTP dele (`switch_m_sdp`), e o registo leva todas as variáveis.
  if chamada_longa "$pw_a" "$ramal" "$dom_a" "$ESTADO/a-meio.out"; then
    sleep 1; "${COMPOSE[@]}" kill server >/dev/null 2>&1
    wait
  else wait; bad "a chamada longa do ramal não se estabeleceu — o registo em disco não foi medido"; "${COMPOSE[@]}" kill server >/dev/null 2>&1; fi
  local i anterior=-1
  for i in $(seq 1 20); do
    sleep 4
    pendentes=$("${COMPOSE[@]}" exec -T freeswitch sh -c 'ls /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes 2>/dev/null | wc -l' | tr -d '[:space:]')
    [ "${pendentes:-0}" -ge 1 ] && [ "$pendentes" = "$anterior" ] && break
    anterior=$pendentes
  done
  v=$("${COMPOSE[@]}" exec -T freeswitch stat -c %a /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes 2>/dev/null | tr -d '[:space:]')
  r=$("${COMPOSE[@]}" exec -T freeswitch sh -c "grep -l -a 'sip_gateway_name' /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes/* 2>/dev/null | wc -l" | tr -d '[:space:]')
  [ "${pendentes:-0}" -ge 1 ] && [ "${r:-0}" -ge 1 ] && [ "$v" = 700 ] && ok "controlo: sem servidor, o registo da perna do tronco fica em disco ($pendentes ficheiro(s), directório 700)" ||
    bad "sem servidor ficaram ${pendentes:-0} registo(s) em disco ($r de uma perna de tronco), directório «$v» — esperava a perna do tronco, em 700"
  r=$("${COMPOSE[@]}" exec -T freeswitch sh -c "grep -l -a -i -e 'inline%3A' -e 'inline:' /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes/* 2>/dev/null | wc -l" | tr -d '[:space:]')
  [ "${pendentes:-0}" -ge 1 ] && [ "${r:-1}" = 0 ] && ok "o registo da perna do tronco não leva a chave SRTP do ramal" ||
    bad "$r registo(s) em disco levam uma chave SRTP (a=crypto … inline:) — a do ramal vai na perna do tronco"
  # Com o servidor EM BAIXO no arranque o perfil não consegue perguntar pelos
  # troncos: fica sem nenhum. É o ciclo de releitura que os traz quando o
  # servidor volta — sem ele, só reiniciando outra vez.
  "${COMPOSE[@]}" restart freeswitch >/dev/null 2>&1; espera_perfil; sleep $(( RESCAN + 3 ))
  e=$(estado_gw "$gw")
  [ -z "$e" ] && ok "controlo: com o servidor em baixo o FreeSWITCH arranca SEM o tronco" ||
    bad "com o servidor em baixo o gateway existe na mesma (estado $e) — o controlo não prova nada"
  "${COMPOSE[@]}" start server >/dev/null 2>&1
  if e=$(espera_gw "$gw" REGED $(( RESCAN * 4 + 40 ))); then ok "o servidor volta, e o tronco regista-se sozinho"
  else bad "o servidor voltou e o tronco não apareceu (estado: $e)"; fi
  r=$(marca 923000222)
  case "$r" in +OK*) ok "e a chamada seguinte sai por ele" ;; *) bad "depois de tudo isto a chamada não saiu ($r)" ;; esac
  sleep 8

  echo "9) um tronco não consegue ler as variáveis do FreeSWITCH"
  # O FreeSWITCH passa a resposta do mod_xml_curl pelo pré-processador, que
  # troca \$\${nome} pelo valor da variável global — e o segredo de voz é uma.
  # Um administrador de QUALQUER organização escreve o utilizador do seu
  # tronco: se o texto chegasse cru, o FreeSWITCH registava-se no servidor
  # SIP dele com o segredo de voz da plataforma no `From`.
  local armadilha='$''${delonix_voice_secret}' gwa voz
  voz=$(env_ VOICE_INTERNAL_SECRET)
  r=$(api POST "/api/orgs/$ORG/telephony/trunks" "{\"name\":\"Armadilha\",\"short_code\":\"ARM\",\"host\":\"$OPERADORA\",\"port\":5060,\"transport\":\"udp\",\"srtp\":\"off\",\"register\":true,\"username\":\"$armadilha\",\"password\":\"$armadilha\",\"max_channels\":1}")
  st=${r%% *}; gwa="dlx-$(campo "${r#* }" id)"
  if [ "$st" = 201 ]; then
    for i in $(seq 1 $(( RESCAN * 3 + 20 ))); do [ -n "$(estado_gw "$gwa")" ] && break; sleep 1; done
    sleep 6   # o tempo de o FreeSWITCH tentar registar-se
    v=$(fs_cli "sofia status gateway $gwa")
    if [ -z "$v" ]; then bad "o tronco-armadilha não chegou ao FreeSWITCH — a verificação não mede nada"
    elif grep -qF "$voz" <<<"$v"; then bad "o FreeSWITCH EXPANDIU a variável: o gateway tem o segredo de voz como utilizador"
    elif grep -qF "$armadilha" <<<"$v"; then ok "o utilizador do tronco chega ao FreeSWITCH como texto, sem ser expandido"
    else bad "o utilizador do tronco-armadilha não é o segredo, mas também não é o texto que a API recebeu: $(sed -n 's/^Username[[:space:]]*//p' <<<"$v" | head -1)"; fi
    v=$("${COMPOSE[@]}" exec -T operadora sh -c "grep -a -c -F '$voz' /usr/local/freeswitch/var/log/freeswitch/freeswitch.log" 2>/dev/null | tr -d '[:space:]')
    [ "${v:-1}" = 0 ] && ok "a operadora não recebeu o segredo de voz em nenhum pedido" || bad "a operadora recebeu o segredo de voz em $v linha(s) do seu log (o REGISTER leva-o)"
  elif [ "$st" = 400 ] || [ "$st" = 422 ]; then ok "a API recusa um utilizador de tronco com uma referência a variável ($st)"
  else bad "a API respondeu $st ao tronco-armadilha"; fi

  echo "10) nada por entregar, e nenhum segredo no log"
  v=$("${COMPOSE[@]}" exec -T freeswitch sh -c 'ls /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes 2>/dev/null | wc -l' | tr -d '[:space:]')
  [ "${v:-99}" = "${pendentes:-0}" ] && ok "com o servidor de pé nenhum registo ficou por entregar (em disco só os $pendentes do controlo)" ||
    bad "ficaram $v registo(s) em disco, e o controlo só explica ${pendentes:-0}: o servidor recusou os outros"
  # Oito chamadas saíram por um tronco (quatro delas marcadas pelo ramal); a
  # que apanhou o servidor em baixo ficou em disco, as outras sete chegaram.
  # As pernas de quem marcou e as que não passaram por tronco nenhum não contam.
  v=$(registos)
  [ "$v" = 7 ] && ok "sete chamadas por tronco entregues, sete registos — nem um a mais" || bad "a organização tem $v registos de chamada, e chegaram ao servidor 7 chamadas por tronco"
  v=$(fs_log | grep -ac "$(env_ VOICE_INTERNAL_SECRET)")
  [ "${v:-1}" -eq 0 ] && ok "o segredo de voz não aparece no freeswitch.log" || bad "o segredo de voz aparece $v vez(es) no freeswitch.log"
  # O que ficou em disco (os registos do controlo) não leva segredos nem chaves.
  v=$("${COMPOSE[@]}" exec -T freeswitch sh -c "grep -rl -a -e '$voz' -e '$op' -e 'inline%3A' -e 'inline:' /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes 2>/dev/null | wc -l" | tr -d '[:space:]')
  [ "${v:-1}" = 0 ] && ok "os registos em disco não levam o segredo de voz, a password do tronco nem chaves SRTP" ||
    bad "$v registo(s) em disco levam o segredo de voz, a password do tronco ou uma chave SRTP"
  v=$(fs_log | grep -ac "$op")
  [ "${v:-1}" -eq 0 ] && ok "a password do tronco não aparece no freeswitch.log" || bad "a password do tronco aparece $v vez(es) no freeswitch.log"
  v=$("${COMPOSE[@]}" exec -T freeswitch sh -c "grep -rl -a '$op' /usr/local/freeswitch/var/log 2>/dev/null | wc -l")
  [ "${v:-1}" -eq 0 ] && ok "nenhum ficheiro do directório de logs traz a password do tronco" || bad "$v ficheiro(s) do directório de logs trazem a password do tronco"
  v=$(fs_log | grep -ac 'Ignoring duplicate gateway')
  echo "       ruído do ciclo de releitura: $v linha(s) «Ignoring duplicate gateway» no log desde o último arranque"
}

# Pelo nome do projecto, e não pelo ficheiro: desmonta mesmo sem o .env.
down() {
  docker compose -p troncosprova down -v >/dev/null 2>&1
  docker image rm delonix-server:troncos-prova >/dev/null 2>&1
  rm -rf "$ESTADO"
  if [ -n "$(docker ps -aq --filter label=com.docker.compose.project=troncosprova)" ]; then echo "✗ ficaram contentores da réplica"; return 1; fi
  echo "réplica desmontada"
}

case "${1:-tudo}" in
  up) up ;;
  mede) [ -f "$ESTADO/.env" ] || { echo "✗ a réplica não está erguida (bash scripts/troncos-prova.sh up)"; exit 1; }; mede ;;
  down) down; exit 0 ;;
  tudo) trap down EXIT; up; [ "$fail" = 0 ] && mede ;;
  -h|--help|ajuda) uso 0 ;;
  *) uso ;;
esac
[ "$fail" = 0 ] && echo "PASSOU" || { echo "FALHOU"; exit 1; }
