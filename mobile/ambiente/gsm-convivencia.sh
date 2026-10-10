#!/usr/bin/env bash
# RF-25: uma chamada GSM a entrar no emulador (sem SIM real). Prova que o ambiente consegue
# gerar o evento que a app tem de tratar. mCallState: 0 repouso · 1 a tocar · 2 em curso.
. "$(dirname "$0")/env.sh"
N=${1:-244912345678}
estado(){ adb shell dumpsys telephony.registry | grep -m1 -oE 'mCallState=[0-9]' | cut -d= -f2; }
adb emu gsm call "$N" >/dev/null
for i in $(seq 1 10); do [ "$(estado)" = 1 ] && break; sleep 1; done
[ "$(estado)" = 1 ] && echo "PASS  GSM a tocar" || { echo "FAIL  GSM não tocou"; exit 1; }
adb emu gsm accept "$N" >/dev/null; sleep 2
[ "$(estado)" = 2 ] && echo "PASS  GSM atendida (OFFHOOK)" || echo "WARN  estado após atender: $(estado)"
adb emu gsm cancel "$N" >/dev/null; sleep 2
[ "$(estado)" = 0 ] && echo "PASS  GSM terminada" || { echo "FAIL  GSM ficou presa"; exit 1; }
