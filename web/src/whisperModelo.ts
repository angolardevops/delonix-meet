/**
 * O modelo Whisper WASM está servido por ESTA instalação?
 *
 * PORQUE EXISTE: a resposta decide se há recuo soberano quando o ASR do
 * servidor cai. Era medida num só sítio — a consola de IA (`pages/Intelligence.tsx`),
 * para mostrar «instalado» ou «não instalado» — e o motor (`media.ts`) nunca a
 * olhava: arrancava o worker, o `pipeline()` não encontrava o modelo, o erro
 * morria num `console.warn` dentro do worker, e o utilizador ficava com «(a
 * carregar modelo de voz local…)» para sempre, sem legenda e sem explicação.
 *
 * SEM IMPORTAÇÕES do resto da app, como o `legendasPref.ts`: isto entra no
 * chunk da consola E no do motor, e não pode arrastar nenhum dos dois.
 *
 * O modelo é servido pela própria instalação (`deploy/fetch-whisper.sh` põe-no
 * em `web/public/models`), nunca de um CDN — `whisperWorker.ts` arranca com
 * `env.allowRemoteModels = false`, e é isso que torna este motor soberano.
 */

export const MODEL_BASE = '/models/Xenova/whisper-tiny'
/**
 * Os dois ficheiros sem os quais o `pipeline()` não transcreve. Não é a lista
 * completa do modelo: é o par cuja ausência prova que o modelo não está lá.
 */
export const MODEL_FILES = [
  'onnx/encoder_model_quantized.onnx',
  'onnx/decoder_model_merged_quantized.onnx',
]

/**
 * Tamanho de um ficheiro estático, ou `null` se não existe.
 *
 * `text/html` conta como ausência: numa SPA, um caminho que não existe devolve
 * **200 com a página da app** (o fallback do servidor). Sem esta verificação,
 * «o modelo está instalado» era verdade em qualquer instalação do mundo.
 */
export async function tamanhoEstatico(url: string, signal?: AbortSignal): Promise<number | null> {
  const r = await fetch(url, { method: 'HEAD', signal, cache: 'no-store' })
  const type = r.headers.get('content-type') ?? ''
  if (!r.ok || type.includes('text/html')) return null
  return Number(r.headers.get('content-length') ?? 0)
}

/**
 * A medição, UMA vez por carregamento da página.
 *
 * Em cache numa promessa e não num booleano: duas chamadas quase simultâneas
 * (a consola a desenhar e a sala a arrancar o motor) partilham a mesma ida à
 * rede em vez de fazerem duas.
 *
 * Uma falha de rede conta como ausência NESTA tentativa, de propósito: se não
 * se consegue medir, não se pode prometer um recuo que talvez não exista. A
 * consequência de errar aqui é uma mensagem a dizer que não há motor local —
 * nunca áudio a sair para fora, porque a Web Speech continua a exigir a
 * aceitação explícita.
 *
 * MAS SÓ O SIM É QUE SE GUARDA. Cachar o «não» para o resto da página fazia de
 * uma falha de rede de um segundo — um 5xx do servidor de estáticos, uma
 * quebra de Wi-Fi, a aplicação instalada a abrir offline — uma instalação
 * permanentemente «sem modelo local»: a pessoa ficava a ver «a transcrição
 * local não está instalada neste servidor» até recarregar a página, numa
 * instalação que serve o modelo. Falhar deixa a medição por fazer, e a
 * tentativa seguinte volta a medir.
 */
let medicao: Promise<boolean> | null = null
let medido: boolean | null = null

export function modeloInstalado(): Promise<boolean> {
  medicao ??= Promise.all(MODEL_FILES.map((f) => tamanhoEstatico(`${MODEL_BASE}/${f}`).catch(() => null)))
    .then((sizes) => sizes.every((s) => s !== null))
    .catch(() => false)
    .then((ok) => {
      if (ok) medido = true
      // Não ficou provado que está lá: esquece a tentativa para que a
      // próxima volte a medir, em vez de herdar este «não».
      else medicao = null
      return ok
    })
  return medicao
}

/**
 * O que JÁ se sabe, sem esperar: `null` enquanto a medição não voltou.
 *
 * Serve a decisão síncrona do `Transcriber.start`. `null` vale «presume-se
 * instalado» no `motorDaTranscricao` — e se a presunção estiver errada, o
 * `WhisperEngine.start` devolve `false` e quem chama diz que não há motor. A
 * presunção custa uma mensagem, nunca uma fuga de áudio.
 */
export function modeloInstaladoConhecido(): boolean | null {
  return medido
}

/** Só para os testes: esquece a medição desta página. */
export function esqueceMedicao() {
  medicao = null
  medido = null
}
