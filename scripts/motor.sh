# shellcheck shell=bash
# ============================================================
#  O motor de contentores das provas — para `source`, não para correr.
#
#  delonix (daemonless, sem root) se existir, senão docker. MOTOR=docker ou
#  MOTOR=delonix escolhe-o à mão. As provas não usam compose: o `delonix
#  compose` não interpola variáveis nem tem `exec`, e uma prova com dois
#  caminhos mede duas coisas. Usam contentores soltos numa rede sua.
#
#  O que os dois motores NÃO têm igual, medido com o delonix 4.5.0:
#    - a rede escolhe-se com `--net` (delonix) ou `--network` (docker): $M_NET;
#    - um argumento começado por `-` logo a seguir à imagem é lido pelo delonix
#      como opção sua (`-c` era `--cpus`); `--` separa, e o docker passava-o ao
#      comando. Quem chama escreve `IMAGEM "${M_SEP[@]}" -c '…'`;
#    - o anfitrião NÃO chega aos endereços dos contentores do delonix (sem
#      root, a ponte não está no espaço de rede do anfitrião): o que a prova
#      precisa de alcançar publica-se em loopback (`-p 127.0.0.1:P:p`);
#    - o delonix só publica portas de contentores com endereço em 10.200–254.x;
#    - um contentor COM porta publicada, morto com `kill`, não volta a
#      arrancar no delonix («did not restart inside the network»); com `stop`
#      volta. Daí o m_derruba;
#    - não há `create`: o que se copiava para dentro antes de arrancar passa a
#      ser um contentor que espera pela ordem (m_cria, m_cp, m_arranca);
#    - o `rm -f` falha de vez em quando e deixa o contentor em «Dead»: o m_rm
#      confere e repete.
# ============================================================
if [ -z "${MOTOR:-}" ]; then
  if command -v delonix >/dev/null 2>&1; then MOTOR=delonix; else MOTOR=docker; fi
fi
case "$MOTOR" in
  delonix) M_CT=(delonix container); M_NET=--net; M_SEP=(--) ;;
  docker)  M_CT=(docker); M_NET=--network; M_SEP=() ;;
  *) echo "✗ MOTOR=$MOTOR — é delonix ou docker" >&2; exit 2 ;;
esac
command -v "$MOTOR" >/dev/null 2>&1 || { echo "✗ precisa de $MOTOR (as provas correm em contentores)" >&2; exit 1; }

m_run()     { "${M_CT[@]}" run "$@"; }
m_exec()    { "${M_CT[@]}" exec "$@"; }
m_logs()    { "${M_CT[@]}" logs "$@"; }
m_cp()      { "${M_CT[@]}" cp "$@"; }
# m_rm <contentor…> — tira-os, e confere. O `rm -f` do delonix 4.5.0 deixa de
# vez em quando um contentor em «Dead» (medido duas vezes num dia, com e sem
# porta publicada), que sai à segunda: a rede dele ficava de pé, e a prova
# seguinte não conseguia criar a mesma sub-rede.
m_rm() {
  local t c resta
  "${M_CT[@]}" rm -f "$@"
  for t in 1 2 3; do
    resta=()
    for c in "$@"; do m_nomes | grep -qxF -- "$c" && resta+=("$c"); done
    [ "${#resta[@]}" -eq 0 ] && return 0
    sleep 1; "${M_CT[@]}" rm -f "${resta[@]}" >/dev/null 2>&1
  done
  return 0
}
m_start()   { "${M_CT[@]}" start "$@"; }
m_restart() { "${M_CT[@]}" restart "$@"; }
# m_derruba <contentor…> — SIGTERM, um segundo, SIGKILL.
m_derruba() { "${M_CT[@]}" stop -t 1 "$@"; }

# m_nomes — os nomes de todos os contentores, a correr ou não, um por linha
m_nomes() {
  case "$MOTOR" in
    docker)  docker ps -a --format '{{.Names}}' 2>/dev/null ;;
    delonix) delonix container ps -a -o json 2>/dev/null |
               python3 -c 'import json,sys
for c in json.load(sys.stdin): print(c.get("name") or "")' 2>/dev/null ;;
  esac
}
# m_a_correr <contentor> — 0 se está a correr
m_a_correr() {
  case "$MOTOR" in
    docker)  [ "$(docker inspect -f '{{.State.Running}}' "$1" 2>/dev/null)" = true ] ;;
    delonix) delonix container inspect "$1" 2>/dev/null |
               python3 -c 'import json,sys
d=json.load(sys.stdin); d=d[0] if isinstance(d,list) else d
sys.exit(0 if d.get("status")=="Running" else 1)' 2>/dev/null ;;
  esac
}

# m_rede_cria <nome> <sub-rede> · m_rede_apaga <nome>
m_rede_cria()  { "$MOTOR" network create --subnet "$2" "$1" >/dev/null; }
m_rede_apaga() { "$MOTOR" network rm "$1" >/dev/null 2>&1; }

# m_imagem_existe <repositório:etiqueta>
m_imagem_existe() {
  case "$MOTOR" in
    docker)  docker image inspect "$1" >/dev/null 2>&1 ;;
    delonix) delonix image ls 2>/dev/null | awk 'NR>1{print $1}' |
               grep -qxF -e "$1" -e "docker.io/library/$1" -e "docker.io/$1" ;;
  esac
}
# m_constroi <etiqueta> <contexto> [ficheiro]
m_constroi() {
  case "$MOTOR" in
    docker)  docker build -q -t "$1" ${3:+-f "$3"} "$2" >/dev/null ;;
    delonix) delonix build -t "$1" ${3:+-f "$3"} "$2" >/dev/null 2>&1 ;;
  esac
}
m_imagem_apaga() {
  case "$MOTOR" in
    docker)  docker image rm "$1" >/dev/null 2>&1 ;;
    delonix) delonix image remove "$1" >/dev/null 2>&1 ;;
  esac
}

# m_cria <nome> <opções de run…> -- <imagem> <comando sh> — um contentor que só
# corre o comando depois de m_arranca: o tempo de lhe copiar ficheiros para
# dentro (m_cp) sem os montar do anfitrião. O comando corre por `sh -c`.
M_PORTAO='until [ -e /.arranca ]; do sleep 0.2; done; '
m_cria() {
  local nome=$1 opts=() img cmd; shift
  while [ $# -gt 0 ] && [ "$1" != -- ]; do opts+=("$1"); shift; done
  shift; img=$1; cmd=$2
  m_run -d --name "$nome" "${opts[@]}" --entrypoint /bin/sh "$img" "${M_SEP[@]}" -c "$M_PORTAO$cmd" >/dev/null
}
m_arranca() { m_exec "$1" /bin/sh -c ': > /.arranca'; }
