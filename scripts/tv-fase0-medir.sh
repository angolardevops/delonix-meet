#!/usr/bin/env bash
# ============================================================
#  Fase 0 da RFC-0002 — medir o que só o hardware responde.
#
#  PORQUE EXISTE: três decisões do desenho de TV por captura HDMI dependem de
#  factos que nenhuma linha de Rust antecipa — o descodificador protege a saída
#  (HDCP) ou não, responde a comandos de sintonia ou não, e quantas entradas o
#  barramento USB aguenta. Este script mede-os, e escreve um relatório que se
#  guarda em docs/tv/. Sem esse relatório, as Fases 1–4 não arrancam.
#
#  O QUE ELE NÃO FAZ, de propósito: não decide por ti. Um veredicto que depende
#  de um limiar (o que é «preto», o que é «silêncio») sai com o NÚMERO ao lado,
#  e uma ferramenta em falta sai como «não medido» — nunca como «passou».
#
#  Uso:
#    bash scripts/tv-fase0-medir.sh                      # todos os passos
#    bash scripts/tv-fase0-medir.sh ambiente inventario  # só alguns
#    SEGUNDOS=60 bash scripts/tv-fase0-medir.sh          # amostra mais longa
#
#  Passos: ambiente · inventario · modos · sinal · carga · sintonia · relatorio
#  Saída:  $SAIDA (por omissão ~/.cache/delonix-tv-fase0) — as amostras de
#          vídeo ficam FORA do repositório, de propósito.
#  Guião humano (o que ligar, em que ordem, o que registar):
#          docs/tv/fase0-guiao.md
# ============================================================
set -uo pipefail

SEGUNDOS="${SEGUNDOS:-20}"
SAIDA="${SAIDA:-${XDG_CACHE_HOME:-$HOME/.cache}/delonix-tv-fase0}"
PRETO_YAVG="${PRETO_YAVG:-16}"      # 0–255; abaixo disto a imagem conta como preta
SILENCIO_DB="${SILENCIO_DB:--70}"   # RMS abaixo disto conta como silêncio
mkdir -p "$SAIDA"
ESTADO="$SAIDA/entradas.tsv"        # caminho_fisico<TAB>video<TAB>audio<TAB>fps_sozinho
RELATO="$SAIDA/relatorio.md"

azul() { printf '\n\033[1m== %s\033[0m\n' "$1"; }
nota() { printf '  %s\n' "$1"; }
reg()  { printf '%s\n' "$1" >> "$RELATO"; }

tem() { command -v "$1" >/dev/null 2>&1; }

# O caminho físico da PORTA, sem a interface USB final: é o que emparelha o nó
# de vídeo com o de áudio do MESMO capturador, e é a chave estável do ADR-0031.
porta_de() { printf '%s' "${1%:*}"; }

prop() { # prop <nó> <chave>
  local linha
  linha=$(udevadm info -q property -n "$1" 2>/dev/null | /bin/grep "^$2=" | head -1)
  printf '%s' "${linha#*=}"
}

# ---------------------------------------------------------------- ambiente
passo_ambiente() {
  azul "Ambiente"
  reg "## Ambiente"
  reg ''
  reg "- Data: $(date -Is)"
  reg "- Kernel: $(uname -sr)"
  reg "- Arranque desta máquina: $(uptime -s 2>/dev/null || echo 'desconhecido')"
  reg "- Carga no momento da medida: \`$(cat /proc/loadavg)\`"
  reg "- Amostra por entrada: ${SEGUNDOS}s · limiares: preto YAVG < $PRETO_YAVG, silêncio RMS < ${SILENCIO_DB} dB"
  reg ''
  local faltam=()
  for t in v4l2-ctl ffmpeg ffprobe arecord udevadm cec-client; do
    tem "$t" || faltam+=("$t")
  done
  if [ ${#faltam[@]} -gt 0 ]; then
    reg "**Ferramentas em falta:** \`${faltam[*]}\` — os passos que dependem delas saem como «não medido»."
    reg "Instalar: \`sudo apt install v4l-utils cec-utils ffmpeg alsa-utils\`."
    nota "em falta: ${faltam[*]}"
  else
    reg 'Ferramentas: todas presentes.'
  fi
  reg ''
  reg '### Controladores USB (`lsusb -t`)'
  reg ''
  reg '```'
  lsusb -t >> "$RELATO" 2>&1 || reg '(lsusb falhou)'
  reg '```'
  reg ''
  nota "carga: $(cut -d' ' -f1-3 /proc/loadavg)"
}

# -------------------------------------------------------------- inventario
passo_inventario() {
  azul "Inventário de entradas"
  reg '## Inventário'
  reg ''
  : > "$ESTADO"
  shopt -s nullglob
  local nos=(/dev/video*)
  shopt -u nullglob
  if [ ${#nos[@]} -eq 0 ]; then
    reg '**Nenhum `/dev/video*`.** Sem capturador ligado não há nada a medir.'
    reg ''
    nota 'nenhum nó de vídeo'
    return
  fi

  # Os cartões ALSA, indexados pela porta física.
  declare -A audio_na_porta=()
  shopt -s nullglob
  for ctl in /dev/snd/controlC*; do
    local cp cid
    cp=$(prop "$ctl" ID_PATH); [ -n "$cp" ] || continue
    cid=${ctl#/dev/snd/controlC}
    audio_na_porta[$(porta_de "$cp")]="hw:$cid"
  done
  shopt -u nullglob

  reg '| Nó | Caminho físico (chave estável) | Vendor:Product | Série | Áudio do mesmo capturador |'
  reg '|---|---|---|---|---|'
  local n
  for n in "${nos[@]}"; do
    local caps vp ip mp serie porta aud
    caps=$(prop "$n" ID_V4L_CAPABILITIES)
    # Um dispositivo UVC expõe mais de um nó; só o de captura interessa.
    case "$caps" in *:capture:*) ;; *) continue ;; esac
    vp=$(prop "$n" ID_PATH)
    ip=$(prop "$n" ID_VENDOR_ID); mp=$(prop "$n" ID_MODEL_ID)
    serie=$(prop "$n" ID_SERIAL_SHORT)
    porta=$(porta_de "$vp")
    aud="${audio_na_porta[$porta]:-}"
    reg "| \`$n\` | \`${vp:-?}\` | ${ip:-?}:${mp:-?} | ${serie:-**sem série**} | ${aud:-**nenhum**} |"
    printf '%s\t%s\t%s\t\n' "${vp:-$n}" "$n" "$aud" >> "$ESTADO"
    nota "$n → ${vp:-?} ${aud:+(áudio $aud)}"
  done
  reg ''
  reg 'Duas leituras a tirar desta tabela, e são as do ADR-0031 §2 e §3:'
  reg 'se a coluna **Série** estiver vazia em dois capturadores do mesmo modelo, o'
  reg '`vendor:product` não os distingue e a chave **tem** de ser o caminho físico;'
  reg 'se a coluna do áudio disser «nenhum», o par vídeo+áudio não se forma e o canal fica mudo.'
  reg ''
  reg '**Prova da estabilidade (passo manual):** trocar os dois capturadores de porta,'
  reg 'correr este passo outra vez, e confirmar que o caminho físico acompanhou a PORTA'
  reg 'e não o aparelho.'
  reg ''
}

# ------------------------------------------------------------------- modos
passo_modos() {
  azul "Modos declarados"
  reg '## Modos que cada entrada declara'
  reg ''
  [ -s "$ESTADO" ] || { reg '(sem entradas no inventário)'; reg ''; return; }
  local vp n aud
  while IFS=$'\t' read -r vp n aud _; do
    reg "### \`$n\` (\`$vp\`)"
    reg ''
    reg '```'
    if tem v4l2-ctl; then
      v4l2-ctl -d "$n" --list-formats-ext >> "$RELATO" 2>&1
    else
      # Sem v4l-utils o ffmpeg ainda enumera os formatos — menos detalhe, mas mede.
      ffmpeg -hide_banner -f v4l2 -list_formats all -i "$n" >> "$RELATO" 2>&1
      reg '(sem v4l2-ctl: lista do ffmpeg, sem a grelha de fps por resolução)'
    fi
    reg '```'
    reg ''
    nota "modos de $n registados"
  done < "$ESTADO"
}

# ------------------------------------------------------------------- sinal
# Grava a amostra e devolve, no ficheiro de estado, o fps realmente entregue.
passo_sinal() {
  azul "Sinal: imagem, áudio e fps entregue"
  reg '## Sinal por entrada'
  reg ''
  [ -s "$ESTADO" ] || { reg '(sem entradas no inventário)'; reg ''; return; }
  local tmp="$SAIDA/entradas.novo"; : > "$tmp"
  local vp n aud
  while IFS=$'\t' read -r vp n aud _; do
    local base="$SAIDA/amostra-$(printf '%s' "$n" | tr -c 'a-zA-Z0-9' '_')"
    local mp4="$base.mp4" log="$base.ffmpeg.txt"
    nota "a gravar ${SEGUNDOS}s de $n ${aud:+(+ $aud)}…"
    if [ -n "$aud" ]; then
      ffmpeg -hide_banner -y -f v4l2 -i "$n" -f alsa -i "$aud" -t "$SEGUNDOS" \
             -c:v libx264 -preset veryfast -c:a aac "$mp4" > "$log" 2>&1
    else
      ffmpeg -hide_banner -y -f v4l2 -i "$n" -t "$SEGUNDOS" \
             -c:v libx264 -preset veryfast "$mp4" > "$log" 2>&1
    fi
    local rc=$?
    reg "### \`$n\`"
    reg ''
    if [ $rc -ne 0 ] || [ ! -s "$mp4" ]; then
      reg "**Não gravou** (ffmpeg saiu $rc). Fim do registo:"
      reg ''
      reg '```'
      tail -12 "$log" >> "$RELATO" 2>/dev/null
      reg '```'
      reg ''
      printf '%s\t%s\t%s\t-\n' "$vp" "$n" "$aud" >> "$tmp"
      continue
    fi

    # Luminância média: é o que separa «imagem» de «preto».
    local yavg
    yavg=$(ffmpeg -hide_banner -nostats -i "$mp4" -vf signalstats,metadata=print:key=lavfi.signalstats.YAVG \
                  -f null - 2>&1 | /bin/sed -n 's/.*YAVG=\([0-9.]*\).*/\1/p' \
           | awk '{s+=$1; n++} END {if (n) printf "%.1f", s/n}')
    # RMS do áudio: é o que separa «preto com som» (HDCP) de «sem sinal».
    local rms='n/a'
    if [ -n "$aud" ]; then
      rms=$(ffmpeg -hide_banner -nostats -i "$mp4" -af astats=metadata=1 -f null - 2>&1 \
            | /bin/sed -n 's/.*RMS level dB: *\(-*[0-9.inf]*\).*/\1/p' | head -1)
      [ -n "$rms" ] || rms='n/a'
    fi
    local fps dur frames
    fps=$(ffprobe -v error -select_streams v:0 -show_entries stream=avg_frame_rate -of csv=p=0 "$mp4" 2>/dev/null)
    dur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$mp4" 2>/dev/null)
    frames=$(ffprobe -v error -select_streams v:0 -count_frames -show_entries stream=nb_read_frames -of csv=p=0 "$mp4" 2>/dev/null)
    local fps_real
    fps_real=$(awk -v f="${frames:-0}" -v d="${dur:-0}" 'BEGIN {if (d>0) printf "%.1f", f/d; else printf "?"}')

    local veredicto
    local preto; preto=$(awk -v y="${yavg:-999}" -v l="$PRETO_YAVG" 'BEGIN {print (y<l) ? "sim" : "nao"}')
    local mudo='nao'
    case "$rms" in
      n/a) mudo='n/a' ;;
      *) mudo=$(awk -v r="$rms" -v l="$SILENCIO_DB" 'BEGIN {print (r+0 < l+0) ? "sim" : "nao"}') ;;
    esac
    if [ "$preto" = "sim" ] && [ "$mudo" = "nao" ]; then
      veredicto='**imagem preta COM áudio** → é o padrão do HDCP; estado `protected` (RFC-0002 §12.1)'
    elif [ "$preto" = "sim" ]; then
      veredicto='**imagem preta e sem áudio** → «sem sinal»: cabo, entrada errada, ou aparelho desligado'
    else
      veredicto='**imagem presente** → esta entrada serve'
    fi

    reg "| Medida | Valor |"
    reg "|---|---|"
    reg "| Luminância média (YAVG, 0–255) | ${yavg:-?} |"
    reg "| RMS do áudio (dB) | ${rms} |"
    reg "| fps declarado / entregue | ${fps:-?} / ${fps_real} (${frames:-?} frames em ${dur:-?}s) |"
    reg "| Amostra | \`$mp4\` |"
    reg ''
    reg "Veredicto: $veredicto"
    reg ''
    reg '**Confirmar com os olhos.** Um YAVG baixo pode ser uma cena escura; abrir a amostra fecha a dúvida.'
    reg ''
    nota "$n: YAVG=${yavg:-?} rms=${rms} fps=${fps_real}"
    printf '%s\t%s\t%s\t%s\n' "$vp" "$n" "$aud" "$fps_real" >> "$tmp"
  done < "$ESTADO"
  mv "$tmp" "$ESTADO"
}

# ------------------------------------------------------------------- carga
passo_carga() {
  azul "Tecto do barramento"
  reg '## Tecto do barramento: todas as entradas ao mesmo tempo'
  reg ''
  local linhas; linhas=$(wc -l < "$ESTADO" 2>/dev/null || echo 0)
  if [ "${linhas:-0}" -lt 2 ]; then
    reg "Só ${linhas:-0} entrada(s): **não medido**. O tecto só aparece com duas ou mais a gravar em simultâneo."
    reg ''
    nota 'menos de duas entradas: passo saltado'
    return
  fi
  local pids=() nomes=() vp n aud fps_sozinho
  while IFS=$'\t' read -r vp n aud fps_sozinho; do
    local base="$SAIDA/carga-$(printf '%s' "$n" | tr -c 'a-zA-Z0-9' '_')"
    if [ -n "$aud" ]; then
      ffmpeg -hide_banner -y -f v4l2 -i "$n" -f alsa -i "$aud" -t "$SEGUNDOS" \
             -c:v libx264 -preset veryfast -c:a aac "$base.mp4" > "$base.txt" 2>&1 &
    else
      ffmpeg -hide_banner -y -f v4l2 -i "$n" -t "$SEGUNDOS" \
             -c:v libx264 -preset veryfast "$base.mp4" > "$base.txt" 2>&1 &
    fi
    pids+=($!); nomes+=("$n|$base|${fps_sozinho:-?}")
    nota "arrancou $n em paralelo"
  done < "$ESTADO"
  local p; for p in "${pids[@]}"; do wait "$p"; done

  reg '| Entrada | fps sozinha | fps em simultâneo | Queda |'
  reg '|---|---|---|---|'
  local item
  for item in "${nomes[@]}"; do
    IFS='|' read -r n base fps_sozinho <<< "$item"
    local d f fr
    d=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$base.mp4" 2>/dev/null)
    fr=$(ffprobe -v error -select_streams v:0 -count_frames -show_entries stream=nb_read_frames -of csv=p=0 "$base.mp4" 2>/dev/null)
    f=$(awk -v a="${fr:-0}" -v b="${d:-0}" 'BEGIN {if (b>0) printf "%.1f", a/b; else printf "?"}')
    local queda
    queda=$(awk -v s="$fps_sozinho" -v j="$f" 'BEGIN {
      if (s=="?" || j=="?" || s+0<=0) {print "?"} else {printf "%.0f%%", (1-(j+0)/(s+0))*100}}')
    reg "| \`$n\` | ${fps_sozinho} | ${f} | ${queda} |"
    nota "$n: ${fps_sozinho} → ${f} (${queda})"
  done
  reg ''
  reg 'Leitura: uma queda acima de ~5 % em qualquer entrada significa que o **controlador** saturou,'
  reg 'não que faltem portas. Repetir com os capturadores espalhados por controladores diferentes'
  reg '(ver o `lsusb -t` do Ambiente) antes de concluir qual é o tecto por caixa.'
  reg ''
  reg "**Carga do host durante a medida:** \`$(cat /proc/loadavg)\` — esta máquina é partilhada, e um"
  reg 'número medido com o host saturado não é o tecto do USB.'
  reg ''
}

# ---------------------------------------------------------------- sintonia
passo_sintonia() {
  azul "Sintonia (CEC e IR)"
  reg '## Sintonia'
  reg ''
  reg '### Caminho CEC'
  reg ''
  shopt -s nullglob
  local cecs=(/dev/cec*)
  shopt -u nullglob
  if ! tem cec-client; then
    reg '**Não medido:** falta o `cec-client` (`sudo apt install cec-utils`).'
  elif [ ${#cecs[@]} -eq 0 ]; then
    reg '**Nenhum `/dev/cec*`.** Um dongle de captura UVC **não** expõe a linha CEC — é preciso um'
    reg 'adaptador USB-CEC (RFC-0002 §6.2). Sem ele este caminho não existe nesta máquina.'
  else
    reg '```'
    cec-client -l >> "$RELATO" 2>&1
    printf 'scan\n' | timeout 25 cec-client -s -d 1 >> "$RELATO" 2>&1
    reg '```'
    reg ''
    reg 'O que procurar na varredura: um dispositivo de tipo **Tuner/Recorder** com endereço lógico.'
    reg 'Se o descodificador não aparecer, ele não responde a CEC e o zapping vai por IR — e isso é'
    reg 'uma resposta válida da Fase 0, não uma falha do teste.'
    reg ''
    reg 'Para tentar um canal acima, com `N` = endereço lógico do descodificador:'
    reg '`echo "tx 1N:44:30" | cec-client -s -d 1` (0x44 = *User Control Pressed*, 0x30 = *Channel Up*;'
    reg 'dígitos são 0x20–0x29). Registar aqui o que o aparelho fez — ou não fez.'
  fi
  reg ''
  reg '### Caminho IR (Broadlink)'
  reg ''
  if python3 -c 'import broadlink' 2>/dev/null; then
    reg '```'
    timeout 20 python3 - >> "$RELATO" 2>&1 <<'PY'
import broadlink
achados = broadlink.discover(timeout=8)
if not achados:
    print("nenhum dispositivo Broadlink na LAN (o RM4 mini é 2,4 GHz)")
for d in achados:
    print(f"{type(d).__name__} host={d.host[0]} mac={d.mac.hex()}")
PY
    reg '```'
    reg ''
    reg 'Com o aparelho encontrado: aprender o código do telecomando do descodificador'
    reg '(`broadlink_cli --learn`) e reproduzi-lo (`--send`). A prova desta fase é o canal mudar'
    reg '**no aparelho**; filmar com o telemóvel chega.'
  else
    reg '**Não medido:** falta o módulo `broadlink` (`pip install broadlink` ou `apt install python3-broadlink`).'
  fi
  reg ''
}

# --------------------------------------------------------------- relatorio
passo_relatorio() {
  azul "Relatório"
  reg '## As cinco respostas da Fase 0'
  reg ''
  reg 'Preencher à mão, com o que ficou acima — e deixar em branco o que não foi medido:'
  reg ''
  reg '| Pergunta | Resposta | Onde está a prova |'
  reg '|---|---|---|'
  reg '| Marca e modelo do descodificador | | |'
  reg '| Dá imagem, ou HDCP? | | §Sinal, YAVG + áudio |'
  reg '| Responde à sintonia, e por que caminho? | | §Sintonia |'
  reg '| Modos reais do capturador | | §Modos |'
  reg '| Quantas entradas antes de perder frames | | §Tecto do barramento |'
  reg ''
  reg 'Depois: copiar este ficheiro para `docs/tv/fase0-resultado-AAAA-MM-DD.md` e abrir PR.'
  reg 'É esse relatório que faz o ADR-0031 passar de **Proposto** a **Aceite** — ou que o manda mudar.'
  reg ''
  printf '\nRelatório: %s\n' "$RELATO"
}

# ------------------------------------------------------------------- ciclo
PASSOS=("$@")
[ ${#PASSOS[@]} -gt 0 ] || PASSOS=(ambiente inventario modos sinal carga sintonia relatorio)

: > "$RELATO"
reg "# Fase 0 da RFC-0002 — medição de $(date +%F)"
reg ''
reg 'Gerado por `scripts/tv-fase0-medir.sh`. Os números são desta máquina e desta hora;'
reg 'o que a ferramenta não deu fica dito como **não medido**.'
reg ''

for p in "${PASSOS[@]}"; do
  case "$p" in
    ambiente|inventario|modos|sinal|carga|sintonia|relatorio) "passo_$p" ;;
    *) printf 'passo desconhecido: %s\n' "$p" >&2; exit 2 ;;
  esac
done
