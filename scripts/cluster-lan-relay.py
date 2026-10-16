#!/usr/bin/env python3
"""Relé UDP do endereço da LAN para o loopback onde o cluster publica.

PORQUE EXISTE. O `delonix net ingress publish` liga SEMPRE em 127.0.0.1
(slirp4netns, sem root) — medido a 2026-10-07: tanto a 443 do ingress como as
portas de voz. Um softphone da rede local não alcança loopback, por isso o
cluster instala a voz toda e nenhum telefone se regista nele. O compose não tem
este problema porque publica por endereço (`${LAN_IP}:5070:5070/udp`).

O QUE ISTO NÃO RESOLVE: os endereços que o SIP e o SDP ANUNCIAM. Se o
FreeSWITCH do cluster puser o seu endereço interno no SDP, o telefone manda o
áudio para um sítio que não existe na rede dele — regista-se e não ouve nada.
É por isso que este relé é uma MEDIÇÃO, não uma solução dada por boa.

Laboratório apenas. Ctrl-C fecha tudo.
"""
import selectors
import socket
import sys

LAN = sys.argv[1] if len(sys.argv) > 1 else "10.3.31.15"
PORTAS = [5070] + list(range(20000, 20020))

sel = selectors.DefaultSelector()
# Por porta: o socket virado para a LAN, o socket virado para o cluster, e o
# último cliente visto. UDP não tem ligação: o caminho de volta é o último
# endereço que falou — é o que um relé simétrico pode saber.
estado = {}

for p in PORTAS:
    try:
        fora = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        fora.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        fora.bind((LAN, p))
        dentro = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        dentro.bind(("127.0.0.1", 0))
    except OSError as e:
        print(f"  ! {p}/udp não abriu: {e}", file=sys.stderr)
        continue
    estado[p] = {"fora": fora, "dentro": dentro, "cliente": None, "alvo": ("127.0.0.1", p)}
    sel.register(fora, selectors.EVENT_READ, (p, "fora"))
    sel.register(dentro, selectors.EVENT_READ, (p, "dentro"))

print(f"relé UDP: {LAN} → 127.0.0.1 em {len(estado)} portas ({', '.join(str(p) for p in list(estado)[:3])}…)")
sys.stdout.flush()

try:
    while True:
        for chave, _ in sel.select(timeout=None):
            p, lado = chave.data
            st = estado[p]
            if lado == "fora":
                dados, orig = st["fora"].recvfrom(65535)
                st["cliente"] = orig
                st["dentro"].sendto(dados, st["alvo"])
            else:
                dados, _ = st["dentro"].recvfrom(65535)
                if st["cliente"]:
                    st["fora"].sendto(dados, st["cliente"])
except KeyboardInterrupt:
    pass
