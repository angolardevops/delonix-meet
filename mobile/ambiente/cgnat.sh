#!/usr/bin/env bash
# CGNAT emulado, SEM root e sem tocar na rede do host: tudo vive num user namespace.
#   [telefone 10.64.0.2] -- veth -- [caixa CGNAT: nftables masquerade + conntrack curto] -- veth -- [servidor 198.51.100.10]
# Prova o que o CGNAT faz a um cliente SIP: o mapeamento UDP expira se o cliente se calar
# (RNF-45/RNF-03). Uso: cgnat.sh [timeout_udp_s=6] [keepalive_s=3]
set -euo pipefail
TO=${1:-6}; KA=${2:-3}
if [ -z "${DP_INNER:-}" ]; then DP_INNER=1 exec unshare -rmn --kill-child "$0" "$TO" "$KA"; fi
ip link set lo up
unshare -n sleep 300 & PH=$!          # telefone
unshare -n sleep 300 & SV=$!          # servidor
trap 'kill $PH $SV 2>/dev/null || true' EXIT
sleep 0.3
ip link add v_ph type veth peer name v_ph_in;  ip link set v_ph_in netns $PH
ip link add v_sv type veth peer name v_sv_in;  ip link set v_sv_in netns $SV
ip addr add 10.64.0.1/24 dev v_ph;       ip link set v_ph up
ip addr add 198.51.100.1/24 dev v_sv;    ip link set v_sv up
nsenter -t $PH -n bash -c 'ip link set lo up; ip addr add 10.64.0.2/24 dev v_ph_in; ip link set v_ph_in up; ip route add default via 10.64.0.1'
nsenter -t $SV -n bash -c 'ip link set lo up; ip addr add 198.51.100.10/24 dev v_sv_in; ip link set v_sv_in up; ip route add default via 198.51.100.1'
sysctl -qw net.ipv4.ip_forward=1
nft -f - <<N
table ip cgnat {
  chain post {
    type nat hook postrouting priority 100;
    oifname "v_sv" masquerade random
  }
}
N
# o temporizador de UDP do CGNAT: pequeno, como em operadoras agressivas
sysctl -qw net.netfilter.nf_conntrack_udp_timeout=$TO net.netfilter.nf_conntrack_udp_timeout_stream=$TO 2>/dev/null \
  || echo "AVISO: sem sysctl de conntrack neste namespace; o timeout fica o do kernel"
cat > /tmp/dp_srv.py <<'P'
import socket,sys,time
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.bind(("0.0.0.0",5060)); s.settimeout(30)
_,a=s.recvfrom(64); print("servidor: viu o telefone como",a[0]+":"+str(a[1]),flush=True)
s.sendto(b"OK",a)                      # resposta imediata: passa
time.sleep(float(sys.argv[1]))        # fica calado
s.sendto(b"INVITE",a)                  # chamada a entrar, depois do silencio
P
cat > /tmp/dp_cli.py <<'P'
import socket,sys,time
ka=float(sys.argv[1]); total=float(sys.argv[2])
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.settimeout(0.2)
dst=("198.51.100.10",5060); s.sendto(b"REGISTER",dst)
t0=time.time(); nxt=ka; invite=False
while time.time()-t0<total:
    if ka>0 and time.time()-t0>=nxt: s.sendto(b"KEEPALIVE",dst); nxt+=ka
    try:
        d,_=s.recvfrom(64); print("telefone: recebeu %s aos %.1fs"%(d.decode(),time.time()-t0),flush=True)
        invite|=d==b"INVITE"
    except socket.timeout: pass
print("RESULTADO: INVITE chegou ao telefone" if invite else "RESULTADO: INVITE PERDIDO (o mapeamento CGNAT expirou)")
sys.exit(0 if invite else 3)
P
SILENCE=$((TO+3))
echo "== CGNAT udp_timeout=${TO}s · keepalive=${KA}s (0 = sem) · silencio do servidor=${SILENCE}s"
nsenter -t $SV -n python3 /tmp/dp_srv.py $SILENCE & S=$!
sleep 0.5
nsenter -t $PH -n python3 /tmp/dp_cli.py "$KA" $((SILENCE+3))
wait $S 2>/dev/null || true
