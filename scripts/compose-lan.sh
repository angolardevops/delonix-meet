#!/usr/bin/env bash
# Gera o ficheiro de sobreposição do compose que expõe os RAMAIS à rede local.
#
# O compose.yaml não interpola variáveis (o `delonix compose` não o faz), e o
# IP da máquina na rede local não se pode versionar: este script escreve-o num
# ficheiro gerado (deploy/compose/generated/lan.yaml), que o `make compose-up
# LAN_IP=…` junta ao compose.yaml.
#
# Só o perfil dos ramais (5070) e uma gama pequena de portas de áudio. É um
# laboratório: os ramais autenticam por SIP Digest, mas a sinalização vai em
# claro e as chaves SRTP (SDES) vão nela — não é para uma rede em que não se confia.
set -euo pipefail
: "${LAN_IP:?uso: LAN_IP=<ip desta máquina na rede local> $0}"
[[ "$LAN_IP" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "LAN_IP não é um IPv4: $LAN_IP" >&2; exit 1; }
cat <<YAML
# Gerado por scripts/compose-lan.sh — NÃO versionar (tem o IP desta máquina).
services:
  freeswitch:
    environment:
      DELONIX_EXTERNAL_IP: ${LAN_IP}
      DELONIX_RTP_MIN: "20000"
      DELONIX_RTP_MAX: "20100"
    ports:
      - "${LAN_IP}:5070:5070/udp"
      - "${LAN_IP}:5070:5070/tcp"
      - "${LAN_IP}:20000-20100:20000-20100/udp"
YAML
