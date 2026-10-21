#!/bin/sh
# Operadora de ENSAIO para a prova dos troncos (scripts/troncos-prova.sh).
#
# É o FreeSWITCH da imagem com a configuração vanilla quase como vem: o perfil
# `internal` (5060) autentica as contas de demonstração 1000–1019, e o plano de
# marcação é trocado pelo de operadora-dialplan.xml — atende, dá ocupado ou
# recusa conforme o número. Só serve dentro da rede da prova, sem portas
# publicadas: a vanilla não é configuração que se exponha.
set -eu
V=/usr/local/freeswitch/etc/freeswitch
: "${OPERADORA_PASSWORD:?}"
rm -f "$V"/sip_profiles/*-ipv6.xml "$V"/sip_profiles/external.xml "$V"/sip_profiles/external/*.xml
sed -i -E "s#default_password=[^\"]*#default_password=${OPERADORA_PASSWORD}#" "$V/vars.xml"
sed -i -E 's#cmd="stun-set" data="(external_rtp_ip|external_sip_ip)=stun:[^"]*"#cmd="set" data="\1=$${local_ip_v4}"#' "$V/vars.xml"
rm -rf "$V"/dialplan/*.xml "$V"/dialplan/default "$V"/dialplan/public "$V"/dialplan/skinny-patterns
cp /operadora/operadora-dialplan.xml "$V/dialplan/default.xml"
exec /usr/local/freeswitch/bin/freeswitch -nonat -nf -nc
