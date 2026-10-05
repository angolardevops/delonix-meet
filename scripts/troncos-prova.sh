#!/usr/bin/env bash
# ============================================================
#  Prova dos TRONCOS na configuração que corre (ADR-0009, plano de lacunas T1).
#
#  Ergue uma réplica — contentores, rede e endereços próprios, nunca o
#  laboratório; só a API do servidor é publicada, e em loopback: o servidor, o
#  FreeSWITCH do Meet arrancado pelo voice/cluster/freeswitch-entrypoint.sh
#  com os ficheiros que o compose.yaml da raiz põe em /meet, e uma operadora
#  de ENSAIO (o FreeSWITCH vanilla, com contas e um plano que atende, dá
#  ocupado ou recusa), que só se alcança da rede da réplica.
#
#  Corre no delonix (daemonless, sem root) se existir, senão no docker —
#  MOTOR=docker|delonix escolhe-o; as diferenças estão em scripts/motor.sh.
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
#    8. o servidor fala com o Event Socket do FreeSWITCH (ESL): a API vê o
#       registo do tronco; um tronco ALTERADO ou APAGADO chega ao FreeSWITCH
#       sem ninguém reiniciar nada; e o ESL recusa quem não é o servidor,
#       mesmo com a password certa;
#    9. nenhum registo ficou por entregar, e nem o segredo de voz, nem a
#       password do tronco, nem a do ESL ficam no log.
#
#  Uso:
#    bash scripts/troncos-prova.sh            ergue, mede, desmonta
#    bash scripts/troncos-prova.sh up|mede|down   um passo de cada vez
#
#    SERVER_IMAGE=<imagem>   o servidor (por omissão delonix-server:latest, a
#                            do `make image` — cada motor tem o seu store:
#                            com o delonix instalado o `make image` constrói
#                            para o dele, e o docker só a vê com
#                            `make image BUILDER=docker`);
#    SERVER_BIN=<binário>    em vez da imagem: embrulha o binário da tua árvore
#                            numa imagem descartável (base SERVER_BIN_BASE,
#                            por omissão ubuntu:24.04);
#    FS_IMAGE=<imagem>       o FreeSWITCH (por omissão delonix-meet/freeswitch:1.11.3);
#    SEM_ESL=1               o CONTROLO do passo 10: a mesma réplica com o ESL
#                            fechado. O passo passa a esperar o contrário — o
#                            tronco alterado e o apagado ficam como estavam;
#    TRONCOS_PREFIXO=<a.b.c> o /24 da réplica (por omissão 10.251.51 — o
#                            delonix só publica portas de 10.200–254.x).
#
#  O que NÃO mede:
#    - uma CENTRAL a marcar para fora: só o ramal chega ao plano de marcação.
#    - ramal para ramal, e o número de acesso às reuniões: precisam de dois
#      telefones registados (o `softphone-prova.sh par` mede-o contra a sala).
#    - uma operadora de verdade: SRTP, TLS, NAT, DTMF, identidade do chamador.
#    - a «chamada de teste» da consola (o `originate` pelo ESL).
#    - mais de um FreeSWITCH, ou mais de uma réplica do servidor.
#    - o chart e o cluster: a lista de ficheiros é a do compose.yaml.
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
ESTADO=$PWD/.troncos-prova
. scripts/motor.sh
export MOTOR                      # o softphone-prova.sh corre no mesmo motor
P=troncosprova                    # prefixo dos contentores e da rede desta réplica
PREFIXO=${TRONCOS_PREFIXO:-10.251.51}
PG_IP=$PREFIXO.10; REDIS_IP=$PREFIXO.11; SERVER_IP=$PREFIXO.12; FS_IP=$PREFIXO.13
OPERADORA=$PREFIXO.20
FS_IMG=${FS_IMAGE:-delonix-meet/freeswitch:1.11.3}
API=    # http://127.0.0.1:<porta publicada>, escolhida no `up` (env_ API_PORT)
RESCAN=10
fail=0
ok()  { printf '  ✓ %s\n' "$*"; }
bad() { printf '  ✗ %s\n' "$*"; fail=1; }
uso() { sed -n '2,66p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-2}"; }
export ESTADO

segredo() { head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n'; }
# cx <serviço> <comando…> — corre dentro de um contentor da réplica
cx() { local svc=$1; shift; m_exec "$P-$svc" "$@"; }
psql_() { cx postgres psql -v ON_ERROR_STOP=1 -U delonix -d delonix -qAt "$@"; }
fs_cli() { cx freeswitch sh -c 'P=$(sed -n "s/.*name=\"password\" value=\"\([^\"]*\)\".*/\1/p" /conf/autoload_configs/event_socket.conf.xml); /usr/local/freeswitch/bin/fs_cli -p "$P" -x "$0"' "$1" 2>/dev/null; }
fs_log() { cx freeswitch cat /usr/local/freeswitch/var/log/freeswitch/freeswitch.log 2>/dev/null; }
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
  ( SOFTPHONE_PASSWORD=$1 bash scripts/softphone-prova.sh chamada --servidor "$FS_IP:5070" --rede "$P-net" \
      --utilizador "$2" --dominio "$3" --destino 923000888 --segundos 14 > "$4" 2>&1 ) &
  for i in $(seq 1 90); do
    SOFT=$(m_nomes | grep -E '^sp[0-9]+-a$' | head -1)
    [ -n "$SOFT" ] && m_logs "$SOFT" 2>&1 | grep -q 'Call established' && return 0
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
  local img=${SERVER_IMAGE:-delonix-server:latest} f nome n=0 i porta
  mkdir -p "$ESTADO"; chmod 700 "$ESTADO"
  if [ -n "${SERVER_BIN:-}" ]; then
    [ -x "$SERVER_BIN" ] || { echo "✗ SERVER_BIN=$SERVER_BIN não é um executável"; exit 1; }
    img=delonix-server:troncos-prova
    rm -rf "$ESTADO/imagem"; mkdir -p "$ESTADO/imagem"
    cp "$SERVER_BIN" "$ESTADO/imagem/delonix-server"
    printf 'FROM %s\nCOPY delonix-server /app/delonix-server\nUSER 65532:65532\nENTRYPOINT ["/app/delonix-server"]\n' \
      "${SERVER_BIN_BASE:-ubuntu:24.04}" > "$ESTADO/imagem/Dockerfile"
    m_constroi "$img" "$ESTADO/imagem" || { echo "✗ não consegui embrulhar $SERVER_BIN numa imagem ($MOTOR)"; exit 1; }
    rm -rf "$ESTADO/imagem"
  fi
  m_imagem_existe "$img" || { echo "✗ o $MOTOR não tem a imagem do servidor $img (make image, SERVER_IMAGE=… ou SERVER_BIN=…)"; exit 1; }
  m_imagem_existe "$FS_IMG" || { echo "✗ o $MOTOR não tem a imagem do FreeSWITCH $FS_IMG (make freeswitch-image, ou FS_IMAGE=…)"; exit 1; }
  for f in postgres:17-alpine redis:7-alpine; do
    m_imagem_existe "$f" || echo "  ! o $MOTOR ainda não tem $f — vai buscá-la ao registo"
  done
  porta=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
  ( umask 077
    printf 'POSTGRES_PASSWORD=%s\nJWT_SECRET=%s\nTURN_SECRET=%s\nPROVISIONING_SECRET=%s\nVOICE_INTERNAL_SECRET=%s\nOPERADORA_PASSWORD=%s\nADMIN_PASSWORD=Pr0va-%s\n' \
      "$(segredo)" "$(segredo)" "$(segredo)" "$(segredo)" "$(segredo)" "$(segredo)" "$(segredo)" > "$ESTADO/.env"
    printf 'DATA_ENCRYPTION_KEYS=prova:%s\nESTADO=%s\nSERVER_IMAGE=%s\nRESCAN_SECS=%s\n' \
      "$(head -c 32 /dev/urandom | base64 -w0)" "$ESTADO" "$img" "$RESCAN" >> "$ESTADO/.env"
    printf 'API_PORT=%s\nTELEPHONY_ESL_PASSWORD=%s\n' "$porta" "$(segredo)" >> "$ESTADO/.env"
    # O ambiente de cada contentor, em ficheiros (nunca na linha de comandos).
    printf 'POSTGRES_USER=delonix\nPOSTGRES_DB=delonix\nPOSTGRES_PASSWORD=%s\n' "$(env_ POSTGRES_PASSWORD)" > "$ESTADO/postgres.env"
    { printf 'DATABASE_URL=postgres://delonix:%s@%s:5432/delonix\nREDIS_URL=redis://%s:6379\n' "$(env_ POSTGRES_PASSWORD)" "$PG_IP" "$REDIS_IP"
      grep -E '^(JWT_SECRET|TURN_SECRET|DATA_ENCRYPTION_KEYS|PROVISIONING_SECRET|VOICE_INTERNAL_SECRET)=' "$ESTADO/.env"
      printf 'BIND_ADDR=0.0.0.0:8180\nINTERNAL_BIND_ADDR=0.0.0.0:8181\nTURN_HOST=prova.invalid:3478\nCORS_ORIGINS=https://prova.invalid\n'
      printf 'RECORDINGS_DIR=/tmp/recordings\nREGISTRATION_MODE=open\n'
      # A API recusa — e bem — um tronco para um endereço interno (R213). A
      # operadora de ensaio vive nesta rede: é a excepção que o operador declara.
      printf 'OUTBOUND_ALLOW_HOSTS=%s\n' "$OPERADORA"
      # O Event Socket do FreeSWITCH. SEM_ESL=1 (o controlo) é a instalação
      # de antes: o servidor não avisa o FreeSWITCH de nada.
      [ -n "${SEM_ESL:-}" ] || printf 'TELEPHONY_ESL_ADDR=%s:8021\nTELEPHONY_ESL_PASSWORD=%s\n' "$FS_IP" "$(env_ TELEPHONY_ESL_PASSWORD)"; } > "$ESTADO/server.env"
    { grep -E '^VOICE_INTERNAL_SECRET=' "$ESTADO/.env"
      printf 'DELONIX_CONTROL_URL=http://%s:8181\nDELONIX_RAMAIS_SIP_PORT=5070\nDELONIX_TRUNKS_RESCAN_SECS=%s\n' "$SERVER_IP" "$RESCAN"
      # Aberto SÓ ao endereço do servidor: nem a operadora, na mesma rede, entra.
      [ -n "${SEM_ESL:-}" ] || printf 'TELEPHONY_ESL_PASSWORD=%s\nDELONIX_ESL_CIDRS=%s/32\n' "$(env_ TELEPHONY_ESL_PASSWORD)" "$SERVER_IP"; } > "$ESTADO/freeswitch.env"
    grep -E '^OPERADORA_PASSWORD=' "$ESTADO/.env" > "$ESTADO/operadora.env" )
  API=http://127.0.0.1:$porta
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
  if [ -n "${SEM_ESL:-}" ]; then f="FECHADO (controlo)"; else f="aberto só ao servidor ($SERVER_IP)"; fi
  echo "motor: $MOTOR; rede $PREFIXO.0/24; API em $API; ESL $f"
  # Não é uma rede sem saída de propósito: sem rota por omissão o FreeSWITCH
  # escolhe 127.0.0.1 para escutar, e os dois deixavam de se ver.
  m_rede_cria "$P-net" "$PREFIXO.0/24" || { echo "✗ não consegui criar a rede $PREFIXO.0/24 (TRONCOS_PREFIXO para outra)"; exit 1; }
  sobe() { local svc=$1 e; shift; e=$(m_run -d --name "$P-$svc" "$M_NET" "$P-net" "$@" 2>&1) || { echo "✗ a réplica não arrancou ($svc): $(tail -2 <<<"$e" | tr '\n' ' ')"; exit 1; }; }
  sobe postgres --ip "$PG_IP" --env-file "$ESTADO/postgres.env" postgres:17-alpine
  sobe redis --ip "$REDIS_IP" redis:7-alpine
  for i in $(seq 1 60); do cx postgres pg_isready -q -U delonix >/dev/null 2>&1 && cx redis redis-cli ping 2>/dev/null | grep -q PONG && break; sleep 1; done
  cx postgres pg_isready -q -U delonix >/dev/null 2>&1 || { echo "✗ a base da réplica não ficou pronta"; m_logs --tail 10 "$P-postgres"; exit 1; }
  sobe server --ip "$SERVER_IP" -p "127.0.0.1:$porta:8180" --env-file "$ESTADO/server.env" "$img"
  # O FreeSWITCH do Meet. Os ficheiros de /meet são os da lista do
  # compose.yaml da raiz (acima): a prova corre com o que a instalação corre.
  sobe freeswitch --ip "$FS_IP" --env-file "$ESTADO/freeswitch.env" \
    -v "$ESTADO/entrypoint:/entrypoint:ro" -v "$ESTADO/meet:/meet:ro" \
    --entrypoint /bin/sh "$FS_IMG" /entrypoint/freeswitch-entrypoint.sh
  sobe operadora --ip "$OPERADORA" --env-file "$ESTADO/operadora.env" \
    -v "$PWD/voice/troncos-prova/operadora-entrypoint.sh:/operadora/operadora-entrypoint.sh:ro" \
    -v "$PWD/voice/troncos-prova/operadora-dialplan.xml:/operadora/operadora-dialplan.xml:ro" \
    --entrypoint /bin/sh "$FS_IMG" /operadora/operadora-entrypoint.sh
  for i in $(seq 1 60); do [ "$(api GET /api/status | cut -d' ' -f1)" = 200 ] && break; sleep 2; done
  [ "$(api GET /api/status | cut -d' ' -f1)" = 200 ] && ok "servidor a responder" ||
    { bad "o servidor não respondeu em $API"; m_logs --tail 20 "$P-server"; return; }
  espera_perfil && ok "FreeSWITCH do Meet: perfil external a correr" ||
    { bad "o FreeSWITCH não ficou pronto"; m_logs --tail 30 "$P-freeswitch"; return; }
  for m in mod_json_cdr mod_xml_curl mod_hash; do
    fs_cli "module_exists $m" | grep -q true || bad "o módulo $m não está carregado"
  done
  [ "$fail" = 0 ] && ok "mod_json_cdr, mod_xml_curl e mod_hash carregados"
}

# ------------------------------------------------------------ mede
mede() {
  local r st corpo senha op trunk gw e antes v pendentes=0 SOFT= b entregues=7 i
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
  v=$(cx operadora fs_cli -x "show registrations" 2>/dev/null | grep -c '^1000,')
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
  saida=$(SOFTPHONE_PASSWORD=$pw_a bash scripts/softphone-prova.sh chamada --servidor "$FS_IP:5070" --rede "$P-net" \
            --utilizador "$ramal" --dominio "$dom_a" --destino 923000444 --espera-tom 440 --tom 1000 --segundos 4 2>&1)
  grep -q 'a: chamada estabelecida' <<<"$saida" && grep -q 'media cifrada' <<<"$saida" &&
    ok "ramal 1001 → 923000444: atendida, com a media do ramal cifrada" ||
    { bad "a chamada do ramal para 923000444 não se estabeleceu"; grep -E '✗|session closed' <<<"$saida" | head -4 | sed 's/^/       /'; }
  grep -q 'ouviu os 440 Hz' <<<"$saida" && ok "o ramal ouviu o tom da operadora (440 Hz)" ||
    bad "o ramal NÃO ouviu o tom da operadora: $(grep -E 'Hz' <<<"$saida" | head -2 | tr '\n' ' ')"
  # O sentido contrário: o que a operadora gravou tem o tom do ramal (1000 Hz).
  sleep 3; rm -f "$ESTADO/operadora-ouviu.wav"
  m_cp "$P-operadora:/tmp/operadora-ouviu-244923000444.wav" "$ESTADO/operadora-ouviu.wav" >/dev/null 2>&1
  v=$(bash scripts/softphone-prova.sh medir "$ESTADO/operadora-ouviu.wav" 1000 2>/dev/null | tail -1)
  python3 -c "import sys; sys.exit(0 if float(sys.argv[1] or 0) >= 0.03 else 1)" "${v:-0}" 2>/dev/null &&
    ok "a operadora ouviu o tom do ramal (1000 Hz, amplitude $v): áudio nos dois sentidos" ||
    bad "a operadora NÃO ouviu o tom do ramal (amplitude «${v:-sem gravação}»)"
  espera_registos $(( antes + 1 )) 25
  v=$(psql_ -F' ' -c "SELECT outcome, from_number, (cost_e4 > 0), coalesce(round(mos::numeric,1)::text,'-') FROM telephony_call_records WHERE org_id='$ORG' AND to_number LIKE '%923000444'")
  case "$v" in "answered 1001 t "*) ok "registo: atendida, de «1001» (o número curto, não o utilizador SIP), com custo e MOS ${v##* }" ;;
    *) bad "o registo da chamada do ramal ficou «$v» — esperava «answered 1001 t <MOS>»" ;; esac
  v=$(cx operadora sh -c "grep -a -c -F '$ramal' /usr/local/freeswitch/var/log/freeswitch/freeswitch.log" 2>/dev/null | tr -d '[:space:]')
  [ "${v:-1}" = 0 ] && ok "a operadora nunca viu o utilizador SIP do ramal" || bad "o utilizador SIP do ramal chegou à operadora ($v linha(s) do log dela)"

  antes=$(registos)
  saida=$(SOFTPHONE_PASSWORD=$pw_a bash scripts/softphone-prova.sh chamada --servidor "$FS_IP:5070" --rede "$P-net" \
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
    m_exec "$SOFT" sh -c "printf '/transfer sip:923000999@$dom_a\n' | nc -u -w1 127.0.0.1 55551" >/dev/null 2>&1
    wait; sleep 8
    v=$(psql_ -c "SELECT count(*) FROM telephony_call_records WHERE to_number LIKE '%923000999'")
    r=$(cx operadora sh -c "grep -a -c '923000999' /usr/local/freeswitch/var/log/freeswitch/freeswitch.log" 2>/dev/null | tr -d '[:space:]')
    [ "$v" = 0 ] && [ "${r:-1}" = 0 ] && ok "transferência cega pedida pelo ramal: recusada — nenhuma chamada saiu para o número pedido" ||
      bad "a transferência cega pedida pelo ramal fez sair uma chamada ($v registo(s); $r linha(s) no log da operadora)"
    espera_registos $(( antes + 1 )) 20
    v=$(psql_ -c "SELECT count(*) FROM telephony_call_records WHERE org_id='$ORG' AND to_number LIKE '%923000888' AND outcome='answered'")
    [ "$v" = 1 ] && ok "e a chamada em que foi pedida deixou o seu registo" || bad "a chamada em que a transferência foi pedida ficou SEM registo ($v)"
  else wait; bad "a chamada longa do ramal não se estabeleceu — a transferência não foi medida"; grep -E '✗' "$ESTADO/transferencia.out" | head -2 | sed 's/^/       /'; fi

  # Controlos negativos: o que NÃO pode sair.
  antes=$(registos)
  saida=$(SOFTPHONE_PASSWORD=$pw_a bash scripts/softphone-prova.sh chamada --servidor "$FS_IP:5070" --rede "$P-net" \
            --utilizador "$ramal" --dominio "$dom_a" --destino 0044123456 --segundos 2 2>&1)
  grep -q 'NÃO se estabeleceu' <<<"$saida" && ok "número sem regra no plano: o ramal não sai ($(grep -o 'session closed: .*' <<<"$saida" | head -1 | sed 's/session closed: //'))" ||
    bad "um número SEM regra no plano de marcação saiu a pedido de um ramal"
  saida=$(SOFTPHONE_PASSWORD=errada-de-proposito bash scripts/softphone-prova.sh chamada --servidor "$FS_IP:5070" --rede "$P-net" \
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
    saida=$(SOFTPHONE_PASSWORD=$pw_b bash scripts/softphone-prova.sh chamada --servidor "$FS_IP:5070" --rede "$P-net" \
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
  v=$(cx freeswitch sh -c 'ls /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes 2>/dev/null | wc -l' | tr -d '[:space:]')
  r=$(fs_log | grep -ac 'lua(dialin_ivr.lua)')
  if [ "${r:-0}" -ge 1 ] && [ "${v:-1}" = 0 ] && [ "$(registos)" = "$antes" ]; then ok "o IVR correu, o registo foi aceite (nada em disco) e não ficou guardado"
  else bad "chamada ao IVR: o IVR correu $r vez(es), ficaram $v registo(s) em disco e $(( $(registos) - antes )) guardado(s) — esperava ≥1, 0 e 0"; fi

  echo "8) reiniciar o FreeSWITCH não deixa a instalação sem troncos"
  m_restart "$P-freeswitch" >/dev/null 2>&1; espera_perfil
  if e=$(espera_gw "$gw" REGED $(( RESCAN * 3 + 30 ))); then ok "depois de reiniciar: $gw REGED"
  else bad "depois de reiniciar o gateway não voltou (estado: $e)"; fi
  # O servidor MORRE a meio de uma chamada de um ramal: o registo da perna do
  # tronco não tem a quem ser entregue e fica em disco. É o controlo do «nada
  # por entregar» do passo 11 — e a única maneira de LER um registo destes: a
  # perna do tronco recebe do FreeSWITCH uma cópia do SDP que o ramal ofereceu,
  # com a chave SRTP dele (`switch_m_sdp`), e o registo leva todas as variáveis.
  if chamada_longa "$pw_a" "$ramal" "$dom_a" "$ESTADO/a-meio.out"; then
    # Espera e retoma. O ramal manda um SDP novo a meio da chamada, e o
    # FreeSWITCH volta a copiá-lo para a perna do tronco (sofia_glue_pass_sdp)
    # — DEPOIS de a dial string o ter tirado na origem. A verificação que se
    # segue é o controlo: se a cópia não voltou, o registo em disco não prova
    # nada sobre a renegociação.
    # Até cinco tentativas: num host carregado a perna do tronco demorou a
    # aparecer na lista (medido uma vez, com a carga a 26).
    b=; for i in 1 2 3 4 5; do
      b=$(fs_cli "show channels" | grep -a 'sofia/external/' | head -1 | cut -d, -f1)
      [ -n "$b" ] && break; sleep 1
    done
    m_exec "$SOFT" sh -c "printf '/hold\n' | nc -u -w1 127.0.0.1 55551" >/dev/null 2>&1; sleep 2
    m_exec "$SOFT" sh -c "printf '/resume\n' | nc -u -w1 127.0.0.1 55551" >/dev/null 2>&1; sleep 2
    v=$(fs_cli "uuid_getvar ${b:-nenhuma} switch_m_sdp" | grep -ac 'inline')
    [ -n "$b" ] && [ "${v:-0}" -ge 1 ] && ok "controlo: com a espera e a retoma do ramal, a perna do tronco voltou a receber o SDP dele, com a chave" ||
      bad "a espera/retoma não levou um SDP novo à perna do tronco (perna «${b:-?}», $v linha(s) com chave) — o registo em disco não mede a renegociação"
    sleep 1; m_derruba "$P-server" >/dev/null 2>&1
    wait
  else wait; bad "a chamada longa do ramal não se estabeleceu — o registo em disco não foi medido"; m_derruba "$P-server" >/dev/null 2>&1; fi
  local i anterior=-1
  for i in $(seq 1 20); do
    sleep 4
    pendentes=$(cx freeswitch sh -c 'ls /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes 2>/dev/null | wc -l' | tr -d '[:space:]')
    [ "${pendentes:-0}" -ge 1 ] && [ "$pendentes" = "$anterior" ] && break
    anterior=$pendentes
  done
  v=$(cx freeswitch stat -c %a /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes 2>/dev/null | tr -d '[:space:]')
  r=$(cx freeswitch sh -c "grep -l -a 'sip_gateway_name' /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes/* 2>/dev/null | wc -l" | tr -d '[:space:]')
  [ "${pendentes:-0}" -ge 1 ] && [ "${r:-0}" -ge 1 ] && [ "$v" = 700 ] && ok "controlo: sem servidor, o registo da perna do tronco fica em disco ($pendentes ficheiro(s), directório 700)" ||
    bad "sem servidor ficaram ${pendentes:-0} registo(s) em disco ($r de uma perna de tronco), directório «$v» — esperava a perna do tronco, em 700"
  r=$(cx freeswitch sh -c "grep -l -a -i -e 'inline%3A' -e 'inline:' /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes/* 2>/dev/null | wc -l" | tr -d '[:space:]')
  [ "${pendentes:-0}" -ge 1 ] && [ "${r:-1}" = 0 ] && ok "o registo da perna do tronco não leva a chave SRTP do ramal" ||
    bad "$r registo(s) em disco levam uma chave SRTP (a=crypto … inline:) — a do ramal vai na perna do tronco"
  # Com o servidor EM BAIXO no arranque o perfil não consegue perguntar pelos
  # troncos: fica sem nenhum. É o ciclo de releitura que os traz quando o
  # servidor volta — sem ele, só reiniciando outra vez.
  m_restart "$P-freeswitch" >/dev/null 2>&1; espera_perfil; sleep $(( RESCAN + 3 ))
  e=$(estado_gw "$gw")
  [ -z "$e" ] && ok "controlo: com o servidor em baixo o FreeSWITCH arranca SEM o tronco" ||
    bad "com o servidor em baixo o gateway existe na mesma (estado $e) — o controlo não prova nada"
  m_start "$P-server" >/dev/null 2>&1
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
    v=$(cx operadora sh -c "grep -a -c -F '$voz' /usr/local/freeswitch/var/log/freeswitch/freeswitch.log" 2>/dev/null | tr -d '[:space:]')
    [ "${v:-1}" = 0 ] && ok "a operadora não recebeu o segredo de voz em nenhum pedido" || bad "a operadora recebeu o segredo de voz em $v linha(s) do seu log (o REGISTER leva-o)"
  elif [ "$st" = 400 ] || [ "$st" = 422 ]; then ok "a API recusa um utilizador de tronco com uma referência a variável ($st)"
  else bad "a API respondeu $st ao tronco-armadilha"; fi

  echo "10) o servidor fala com o FreeSWITCH: um tronco alterado ou apagado chega lá sem reiniciar nada"
  # Até aqui um tronco alterado ou apagado ficava como estava até alguém
  # reiniciar o FreeSWITCH: o ciclo de releitura só ACRESCENTA gateways, e
  # tirar um pede `killgw`, que o servidor manda pelo Event Socket (ESL).
  # SEM_ESL=1 corre este passo com ele fechado e espera o contrário: é o
  # controlo que diz que o que aqui se mede vem do ESL e não do ciclo.
  local esl gw2 trunk2 espera=$(( RESCAN * 2 + 12 ))
  esl=$(env_ TELEPHONY_ESL_PASSWORD)
  espera_gw "$gw" REGED $(( RESCAN * 3 + 20 )) >/dev/null || bad "o tronco não estava registado à entrada do passo 10 — o que se segue não mede nada"
  r=$(api GET "/api/orgs/$ORG/telephony/trunks/$trunk"); v=$(campo "${r#* }" status.registration)
  # op_tem <conta> — quantos registos dessa conta tem a operadora
  op_tem() { cx operadora fs_cli -x "show registrations" 2>/dev/null | grep -c "^$1,"; }
  # some_gw <gateway> <segundos> — espera que o gateway deixe de existir. Pelo
  # gateway e não pela lista: `gwlist` só traz os que estão UP.
  some_gw() { local k; for k in $(seq 1 "$2"); do fs_cli "sofia status gateway $1" | grep -q 'Invalid Gateway' && return 0; sleep 1; done; return 1; }
  # esl_da_operadora <password> — a operadora (outro contentor da mesma rede)
  # a pedir `status` ao ESL do Meet: quantas linhas «UP …» recebeu
  esl_da_operadora() { m_exec -e ESLPW="$1" "$P-operadora" sh -c 'timeout 10 fs_cli -H '"$FS_IP"' -P 8021 -p "$ESLPW" -x status 2>&1' | grep -ac '^UP '; }
  if [ -z "${SEM_ESL:-}" ]; then
    [ "$v" = registered ] && ok "a API vê no FreeSWITCH o registo do tronco (registration=registered)" ||
      bad "a API não vê o registo do tronco no FreeSWITCH (registration=«$v») — o servidor não chega ao ESL"
    # Alterado: uma password errada tem de chegar à operadora.
    r=$(api PATCH "/api/orgs/$ORG/telephony/trunks/$trunk" '{"password":"errada-de-proposito-0123456789"}')
    [ "${r%% *}" = 200 ] || bad "a API não aceitou a alteração do tronco ($r)"
    e=REGED; for i in $(seq 1 40); do e=$(estado_gw "$gw"); [ "$e" != REGED ] && [ "$(op_tem 1000)" = 0 ] && break; sleep 1; done
    [ "$e" != REGED ] && [ "$(op_tem 1000)" = 0 ] && ok "tronco alterado (password errada): o registo caiu na operadora em ${i} s, sem reiniciar nada (estado: ${e:-a recriar})" ||
      bad "alterei a password do tronco e o FreeSWITCH continua registado com a antiga (estado $e, $(op_tem 1000) registo(s) na operadora)"
    r=$(api PATCH "/api/orgs/$ORG/telephony/trunks/$trunk" "{\"password\":\"$op\"}")
    [ "${r%% *}" = 200 ] || bad "a API não aceitou repor a password do tronco ($r)"
    if e=$(espera_gw "$gw" REGED 60); then ok "password reposta: o tronco volta a registar-se sozinho"
    else bad "repus a password e o tronco não voltou a registar-se em 60 s (estado: $e)"; fi
    # Rodar a password de um tronco que ESTÁ registado. O gateway antigo
    # desregista-se e o novo regista-se, os dois com o mesmo contacto: se o
    # desregisto chegasse à operadora depois do registo novo, o FreeSWITCH
    # ficava a dizer REGED e a operadora sem registo — até o registo expirar.
    for i in $(seq 1 15); do [ "$(op_tem 1000)" -ge 1 ] && break; sleep 1; done
    r=$(api PATCH "/api/orgs/$ORG/telephony/trunks/$trunk" "{\"password\":\"$op\"}")
    [ "${r%% *}" = 200 ] || bad "a API não aceitou rodar a password do tronco ($r)"
    sleep 3; e=$(espera_gw "$gw" REGED 40) || true; sleep 6
    v=$(op_tem 1000); e=$(estado_gw "$gw")
    [ "$e" = REGED ] && [ "${v:-0}" -ge 1 ] && ok "rodar a password de um tronco registado: REGED no FreeSWITCH e registado na operadora (9 s depois)" ||
      bad "rodei a password de um tronco registado: o FreeSWITCH diz «$e» e a operadora tem $v registo(s) da conta"
    # E pela ordem certa. A operadora de ensaio é um FreeSWITCH, que apaga um
    # registo pelo Call-ID: outra, que o apague pelo contacto, ficava sem
    # registo se o desregisto do gateway antigo chegasse depois do registo do
    # novo. O antigo só sai do perfil depois de mandar o desregisto.
    v=$(fs_log | grep -a -o -F -e "Deleted gateway $gw" -e "Added gateway '$gw'" | tail -2 | cut -d' ' -f1 | tr '\n' ' ')
    [ "$v" = "Deleted Added " ] && ok "e pela ordem certa: o gateway antigo saiu (desregisto enviado) antes de o novo entrar" ||
      bad "o gateway novo entrou antes de o antigo sair (últimas linhas do log: «$v») — o desregisto do antigo segue depois do registo do novo"
    antes=$(registos); r=$(marca 923000111)
    case "$r" in +OK*) espera_registos $(( antes + 1 )) 25 && { ok "e o tronco alterado leva uma chamada, com registo"; entregues=$(( entregues + 1 )); } || bad "a chamada pelo tronco alterado saiu e não deixou registo" ;;
      *) bad "a chamada pelo tronco alterado não foi atendida ($r)" ;; esac
  else
    [ "$v" != registered ] && ok "controlo: sem ESL a API não sabe do registo (registration=«$v»)" ||
      bad "controlo: sem ESL a API diz «registered» — de onde?"
    r=$(api PATCH "/api/orgs/$ORG/telephony/trunks/$trunk" '{"password":"errada-de-proposito-0123456789"}')
    [ "${r%% *}" = 200 ] || bad "a API não aceitou a alteração do tronco ($r)"
    sleep "$espera"; e=$(estado_gw "$gw")
    [ "$e" = REGED ] && [ "$(op_tem 1000)" -ge 1 ] && ok "controlo: sem ESL, ${espera} s depois o tronco alterado continua registado com a password ANTIGA" ||
      bad "controlo: sem ESL o tronco alterado mudou (estado $e) — o passo 10 não distingue o ESL do ciclo de releitura"
    r=$(api PATCH "/api/orgs/$ORG/telephony/trunks/$trunk" "{\"password\":\"$op\"}")
  fi
  # Apagado: um segundo tronco, com outra conta na operadora, criado e apagado.
  r=$(api POST "/api/orgs/$ORG/telephony/trunks" "{\"name\":\"A apagar\",\"short_code\":\"APG\",\"host\":\"$OPERADORA\",\"port\":5060,\"transport\":\"udp\",\"srtp\":\"off\",\"register\":true,\"username\":\"1001\",\"password\":\"$op\",\"prefixes\":[],\"max_channels\":2}")
  st=${r%% *}; trunk2=$(campo "${r#* }" id); gw2="dlx-$trunk2"
  if [ "$st" = 201 ] && [ -n "$trunk2" ] && e=$(espera_gw "$gw2" REGED $(( RESCAN * 3 + 20 ))) && [ "$(op_tem 1001)" -ge 1 ]; then
    r=$(api DELETE "/api/orgs/$ORG/telephony/trunks/$trunk2")
    [ "${r%% *}" = 204 ] || bad "a API não apagou o tronco ($r)"
    if [ -z "${SEM_ESL:-}" ]; then
      some_gw "$gw2" 40 && ok "tronco apagado: o gateway saiu do FreeSWITCH sem reiniciar nada" ||
        bad "apaguei o tronco e o gateway $gw2 continua no FreeSWITCH (estado $(estado_gw "$gw2"))"
      for i in $(seq 1 20); do [ "$(op_tem 1001)" = 0 ] && break; sleep 1; done
      [ "$(op_tem 1001)" = 0 ] && ok "e desregistou-se da operadora" || bad "o tronco apagado continua registado na operadora"
    else
      sleep "$espera"
      [ "$(estado_gw "$gw2")" = REGED ] && [ "$(op_tem 1001)" -ge 1 ] && ok "controlo: sem ESL, ${espera} s depois o tronco APAGADO continua registado na operadora" ||
        bad "controlo: sem ESL o tronco apagado saiu (estado «$(estado_gw "$gw2")») — o passo 10 não distingue o ESL do ciclo de releitura"
    fi
  else bad "o segundo tronco não se criou ou não se registou ($st $(cut -c1-120 <<<"${r#* }"), estado ${e:-?}) — o apagar não foi medido"; fi
  # Quem entra no ESL manda em tudo (origina chamadas por qualquer tronco).
  v=$(esl_da_operadora "$esl"); r=$(cx operadora sh -c 'timeout 10 fs_cli -x status 2>&1' | grep -ac '^UP ')
  if [ -z "${SEM_ESL:-}" ]; then
    [ "${v:-1}" = 0 ] && [ "${r:-0}" -ge 1 ] && ok "outro contentor da mesma rede, com a password CERTA, não entra no ESL (e o mesmo comando, contra o seu próprio FreeSWITCH, entra)" ||
      bad "outro contentor entrou no ESL do Meet com a password ($v linha(s)), ou o comando de controlo não funciona ($r)"
    v=$(cx freeswitch sh -c 'timeout 10 /usr/local/freeswitch/bin/fs_cli -p ClueCon -x status 2>&1' | grep -ac '^UP ')
    r=$(fs_cli status | grep -ac '^UP ')
    [ "${v:-1}" = 0 ] && [ "${r:-0}" -ge 1 ] && ok "a password de fábrica (ClueCon) não entra, nem em loopback" ||
      bad "a password de fábrica entra no ESL ($v), ou o comando de controlo não funciona ($r)"
  else
    [ "${v:-1}" = 0 ] && [ "${r:-0}" -ge 1 ] && ok "controlo: sem ESL declarado, o 8021 não responde a outro contentor (só loopback)" ||
      bad "controlo: sem ESL declarado outro contentor falou com o 8021 ($v), ou o comando de controlo não funciona ($r)"
  fi

  echo "11) nada por entregar, e nenhum segredo no log"
  v=$(cx freeswitch sh -c 'ls /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes 2>/dev/null | wc -l' | tr -d '[:space:]')
  [ "${v:-99}" = "${pendentes:-0}" ] && ok "com o servidor de pé nenhum registo ficou por entregar (em disco só os $pendentes do controlo)" ||
    bad "ficaram $v registo(s) em disco, e o controlo só explica ${pendentes:-0}: o servidor recusou os outros"
  # Oito chamadas saíram por um tronco até ao passo 9 (quatro delas marcadas
  # pelo ramal); a que apanhou o servidor em baixo ficou em disco, as outras
  # sete chegaram. O passo 10, com o ESL, junta-lhes a do tronco alterado.
  # As pernas de quem marcou e as que não passaram por tronco nenhum não contam.
  v=$(registos)
  [ "$v" = "$entregues" ] && ok "$entregues chamadas por tronco entregues, $entregues registos — nem um a mais" || bad "a organização tem $v registos de chamada, e chegaram ao servidor $entregues chamadas por tronco"
  v=$(fs_log | grep -ac "$(env_ VOICE_INTERNAL_SECRET)")
  [ "${v:-1}" -eq 0 ] && ok "o segredo de voz não aparece no freeswitch.log" || bad "o segredo de voz aparece $v vez(es) no freeswitch.log"
  # O que ficou em disco (os registos do controlo) não leva segredos nem chaves.
  v=$(cx freeswitch sh -c "grep -rl -a -e '$voz' -e '$op' -e 'inline%3A' -e 'inline:' /usr/local/freeswitch/var/lib/freeswitch/cdr-pendentes 2>/dev/null | wc -l" | tr -d '[:space:]')
  [ "${v:-1}" = 0 ] && ok "os registos em disco não levam o segredo de voz, a password do tronco nem chaves SRTP" ||
    bad "$v registo(s) em disco levam o segredo de voz, a password do tronco ou uma chave SRTP"
  v=$(fs_log | grep -ac "$op")
  [ "${v:-1}" -eq 0 ] && ok "a password do tronco não aparece no freeswitch.log" || bad "a password do tronco aparece $v vez(es) no freeswitch.log"
  v=$(cx freeswitch sh -c "grep -rl -a '$op' /usr/local/freeswitch/var/log 2>/dev/null | wc -l")
  [ "${v:-1}" -eq 0 ] && ok "nenhum ficheiro do directório de logs traz a password do tronco" || bad "$v ficheiro(s) do directório de logs trazem a password do tronco"
  v=$(fs_log | grep -ac "$esl"); r=$(m_logs "$P-server" 2>&1 | grep -ac "$esl")
  [ "${v:-1}" -eq 0 ] && [ "${r:-1}" -eq 0 ] && ok "a password do ESL não aparece no freeswitch.log nem no log do servidor" ||
    bad "a password do ESL aparece no freeswitch.log ($v) ou no log do servidor ($r)"
  v=$(fs_log | grep -ac 'Ignoring duplicate gateway')
  echo "       ruído do ciclo de releitura: $v linha(s) «Ignoring duplicate gateway» no log desde o último arranque"
}

# Pelos nomes, e não pelo estado: desmonta mesmo sem o .env.
down() {
  local c
  for c in operadora freeswitch server redis postgres; do m_rm "$P-$c" >/dev/null 2>&1; done
  m_rede_apaga "$P-net"
  m_imagem_apaga delonix-server:troncos-prova
  rm -rf "$ESTADO"
  if m_nomes | grep -q "^$P-"; then echo "✗ ficaram contentores da réplica"; return 1; fi
  echo "réplica desmontada"
}

case "${1:-tudo}" in
  up) up ;;
  mede) [ -f "$ESTADO/.env" ] || { echo "✗ a réplica não está erguida (bash scripts/troncos-prova.sh up)"; exit 1; }
        API=http://127.0.0.1:$(env_ API_PORT); mede ;;
  down) down; exit 0 ;;
  tudo) trap down EXIT; up; [ "$fail" = 0 ] && mede ;;
  -h|--help|ajuda) uso 0 ;;
  *) uso ;;
esac
[ "$fail" = 0 ] && echo "PASSOU" || { echo "FALHOU"; exit 1; }
