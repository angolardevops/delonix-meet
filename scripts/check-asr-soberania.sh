#!/usr/bin/env bash
# ============================================================
#  Fitness function: o motor que ENVIA ÁUDIO PARA FORA não se alcança
#  sozinho, e a política que o proíbe não desaparece.
#
#  PORQUE EXISTE
#
#  Medido a 2026-10-18 no commit 049948388: o `legendasPref.ts` documentava a
#  ordem «1. servidor · 2. wasm · 3. webspeech» e implementava outra. O ramo
#  `wasm` só era alcançável sem Web Speech no browser (Firefox, Safari) ou com
#  o modo local pedido à mão. Em Chrome, a degradação que mais acontece — o
#  `/asr` a cair a meio da reunião — recuava para a Web Speech, ou seja,
#  começava a enviar o áudio aos servidores da Google NO MEIO de uma sessão
#  que começara soberana. E estava TRANCADO POR TESTE.
#
#  Os testes voltaram do lado certo (`legendasPref.test.ts`,
#  `transcriberRecuo.test.ts`). O que eles não cobrem, e este portão cobre, é
#  o que uma sessão FUTURA pode fazer sem reparar:
#
#    1. instanciar um reconhecedor de voz do browser NOUTRO ficheiro, por
#       fora da decisão do `motorDaTranscricao` — um ecrã novo, um widget,
#       uma página de testes. A decisão passava a ser contornável e nenhum
#       teste do `legendasPref` daria por isso;
#    2. apagar a perna do SERVIDOR (a coluna, a leitura, ou a recusa da
#       frase) e deixar a política a viver só no `localStorage` do cliente —
#       que é exactamente o estado de onde se veio.
#
#  O QUE ESTE PORTÃO NÃO PROVA, e não finjas que prova: que o áudio não vai à
#  Google. Um cliente hostil instancia o reconhecedor que quiser e mente no
#  campo `engine`. Isto prova que o PRODUTO não o faz sozinho e que a
#  organização tem como recusar o caminho honesto. Quem precisa de garantia
#  corta o acesso do browser à Google na rede.
#
#  Uso:  bash scripts/check-asr-soberania.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0

# O único sítio onde o reconhecedor do browser pode ser construído. A decisão
# de o construir é do `motorDaTranscricao`, e é lá que os três sins se exigem.
SITIO_ABENCOADO='web/src/media.ts'
# `new Ctor()` (como o `media.ts` o faz, por uma referência guardada) ou um
# `new webkitSpeechRecognition()` directo.
PADRAO_CTOR='new (Ctor|webkitSpeechRecognition|SpeechRecognition)\('

# ---- 1. CONTROLO POSITIVO ----------------------------------------------
# Um grep que não encontra nada é indistinguível de um grep que procurou no
# sítio errado. Antes de afirmar «ninguém mais o instancia», prova que acertas
# no que SABES estar lá.
if ! grep -nEq "$PADRAO_CTOR" "$SITIO_ABENCOADO"; then
  echo "✗ asr: controlo positivo FALHOU — não encontrei a instanciação do reconhecedor em $SITIO_ABENCOADO."
  echo "     O padrão deixou de acertar (refactor?), e com ele a verificação 2 passaria vazia."
  echo "     Corrige o PADRAO_CTOR aqui; não deixes o portão passar cego."
  fail=1
fi

# ---- 2. mais ninguém o instancia --------------------------------------
# Ficheiros de TESTE ficam de fora: a bateria do recuo precisa de construir um
# reconhecedor falso para provar que ele NÃO é construído pelo produto.
intrusos=$(grep -rlnE "$PADRAO_CTOR" web/src \
             --include='*.ts' --include='*.tsx' 2>/dev/null \
           | grep -vE '\.test\.ts$' \
           | grep -vxF "$SITIO_ABENCOADO" || true)
if [ -n "$intrusos" ]; then
  echo "✗ asr: o reconhecedor de voz do browser é instanciado FORA do $SITIO_ABENCOADO:"
  echo "$intrusos" | sed 's/^/     /'
  echo "     Esse motor envia o áudio aos servidores da Google e só é alcançável com três sins"
  echo "     (o browser tem, a organização permite, a pessoa aceitou) — ver web/src/legendasPref.ts."
  echo "     Passa pelo motorDaTranscricao em vez de o construir aqui."
  fail=1
fi

# ---- 3. a perna do SERVIDOR continua lá -------------------------------
# Três peças, e nenhuma serve sem as outras duas: a coluna, a leitura e a
# recusa. Cada linha é «descrição|ficheiros|padrão».
PECAS=(
  'a coluna da política|server/migrations|allow_web_speech\b'
  'a leitura da política (fail-closed)|server/src/signaling.rs|fn web_speech_forbidden\b'
  'a recusa da frase do motor proibido|server/src/signaling.rs|fn recusa_motor\b'
  'a entrega da política ao cliente|server/src/signaling.rs|CaptionPolicy\b'
  'a medida da recusa|server/src/metrics.rs|caption_engine_refused_total\b'
  'a escrita da política pela API|server/src/org.rs|allow_web_speech\b'
)
for peca in "${PECAS[@]}"; do
  IFS='|' read -r desc alvo padrao <<< "$peca"
  if ! grep -rqE "$padrao" "$alvo" 2>/dev/null; then
    echo "✗ asr: falta $desc — '$padrao' não aparece em $alvo."
    echo "     Sem ela a política da organização volta a viver só no localStorage do browser,"
    echo "     que é o estado medido a 2026-10-18 e a razão de este portão existir."
    fail=1
  fi
done

# ---- 4. a ordem escrita é a ordem implementada ------------------------
# O defeito original foi uma DIVERGÊNCIA entre o comentário e o código. Não se
# pode verificar prosa, mas pode-se verificar que o recuo soberano existe no
# código da decisão: um `return 'wasm'` alcançável sem passar pela Web Speech.
if ! grep -qE "^\s+if \(wasm\) return 'wasm'" web/src/legendasPref.ts; then
  echo "✗ asr: o recuo soberano saiu do motorDaTranscricao (web/src/legendasPref.ts)."
  echo "     Esperava um 'if (wasm) return \"wasm\"' DEPOIS do braço da Web Speech:"
  echo "     é o que garante que uma falha do ASR do servidor não cai na Google."
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "✓ asr: o motor da Google só se alcança pela decisão partilhada, e a política do servidor está nas 6 peças"
