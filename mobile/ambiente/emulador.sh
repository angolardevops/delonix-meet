#!/usr/bin/env bash
# emulador.sh [pixel|lowend] [start|stop]  — AVD sem janela, com KVM (RNF-41: lowend = 2 GB, API 26)
. "$(dirname "$0")/env.sh"
AVD=dp_pixel_api35; [ "${1:-pixel}" = lowend ] && AVD=dp_lowend_api26
case "${2:-start}" in
  start)
    mkdir -p "$HOME/development/logs"
    nohup emulator -avd "$AVD" -no-window -no-audio -no-snapshot -gpu swiftshader_indirect > "$HOME/development/logs/$AVD.log" 2>&1 &
    adb wait-for-device
    until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do sleep 3; done
    adb devices | sed -n 2p ;;
  stop) adb emu kill ;;
esac
