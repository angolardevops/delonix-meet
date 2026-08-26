/**
 * Análise da aula gravada — a parte "inteligente" da edição.
 *
 * PORQUE NÃO É UM MODELO: a pergunta «onde é que esta aula tem pausas mortas»
 * responde-se com a energia do sinal, não com aprendizagem. Um detector de
 * silêncio por RMS corre em milissegundos sobre uma aula de meia hora, não
 * traz um único byte para o bundle, e funciona offline no primeiro arranque —
 * ao contrário do Whisper, cujo modelo e runtime ONNX vêm do build da imagem e
 * pesam dezenas de MB.
 *
 * A transcrição TEM lugar nesta ferramenta (cortar pela transcrição é o passo
 * seguinte óbvio, e o `Transcriber` do media.ts já existe), mas é uma camada
 * por cima desta — não um substituto. Quem quiser apertar as pausas de uma
 * aula não devia ter de esperar por um modelo de fala.
 */

import type { Troco } from './editor'

export interface Pausa {
  inicio: number
  fim: number
}

export interface AnaliseDeAudio {
  duracao: number
  pausas: Pausa[]
  /** Segundos que se poupam removendo todas as pausas encontradas. */
  poupanca: number
  /** Nível de referência (RMS mediano das janelas com som), para diagnóstico. */
  referencia: number
  /** Limiar usado, derivado da referência. */
  limiar: number
}

export interface OpcoesDeAnalise {
  /** Uma pausa só conta a partir desta duração. Abaixo disto é respiração. */
  minimo?: number
  /**
   * Margem mantida de cada lado da pausa. Cortar rente ao som corta a cauda
   * das palavras e o resultado soa a máquina — é o erro clássico de quem
   * automatiza isto pela primeira vez.
   */
  margem?: number
  /** Janela de análise, em segundos. */
  janela?: number
}

const MINIMO = 0.7
const MARGEM = 0.12
const JANELA = 0.02

/**
 * Encontra as pausas de uma faixa de áudio.
 *
 * O limiar é RELATIVO ao próprio material, não um valor fixo em dB: uma aula
 * gravada num portátil ruidoso e outra num microfone bom têm chãos de ruído
 * muito diferentes, e um limiar fixo ou não apanha nada ou corta a fala.
 */
export async function analisarPausas(
  audio: Blob,
  opcoes: OpcoesDeAnalise = {},
): Promise<AnaliseDeAudio> {
  const minimo = opcoes.minimo ?? MINIMO
  const margem = opcoes.margem ?? MARGEM
  const janela = opcoes.janela ?? JANELA

  const ctx = new AudioContext()
  let buffer: AudioBuffer
  try {
    buffer = await ctx.decodeAudioData(await audio.arrayBuffer())
  } finally {
    void ctx.close().catch(() => {})
  }

  const sr = buffer.sampleRate
  const porJanela = Math.max(1, Math.round(janela * sr))
  const canais = buffer.numberOfChannels
  const dados: Float32Array[] = []
  for (let c = 0; c < canais; c++) dados.push(buffer.getChannelData(c))

  // RMS por janela, misturando os canais.
  const n = Math.floor(buffer.length / porJanela)
  const rms = new Float32Array(n)
  for (let j = 0; j < n; j++) {
    let soma = 0
    const de = j * porJanela
    for (let i = 0; i < porJanela; i++) {
      let amostra = 0
      for (let c = 0; c < canais; c++) amostra += dados[c][de + i]
      amostra /= canais
      soma += amostra * amostra
    }
    rms[j] = Math.sqrt(soma / porJanela)
  }

  // Referência = mediana das janelas COM som. Usar a média deixaria o próprio
  // silêncio puxar a referência para baixo e o limiar deixaria de apanhar nada.
  const comSom = Array.from(rms).filter((v) => v > 1e-5).sort((a, b) => a - b)
  const referencia = comSom.length ? comSom[Math.floor(comSom.length / 2)] : 0
  // 8% da referência: bem abaixo da fala, bem acima do chão de ruído típico.
  // O mínimo absoluto evita que uma gravação totalmente muda dê "tudo pausa".
  const limiar = Math.max(referencia * 0.08, 1e-4)

  const pausas: Pausa[] = []
  let inicio: number | null = null
  for (let j = 0; j <= n; j++) {
    const calado = j < n ? rms[j] < limiar : true
    if (calado && inicio === null) inicio = j
    if (!calado && inicio !== null) {
      empurrar(pausas, inicio * janela, j * janela, minimo, margem)
      inicio = null
    }
  }
  if (inicio !== null) empurrar(pausas, inicio * janela, n * janela, minimo, margem)

  const duracao = buffer.duration
  const poupanca = pausas.reduce((a, p) => a + (p.fim - p.inicio), 0)
  return { duracao, pausas, poupanca, referencia, limiar }
}

function empurrar(destino: Pausa[], de: number, ate: number, minimo: number, margem: number): void {
  const inicio = de + margem
  const fim = ate - margem
  if (fim - inicio >= minimo) destino.push({ inicio, fim })
}

/**
 * Converte as pausas nos troços a MANTER. É este o formato que o exportador
 * consome — o corte pensa no que fica, não no que sai.
 */
export function trocosSemPausas(analise: AnaliseDeAudio): Troco[] {
  const trocos: Troco[] = []
  let cursor = 0
  for (const p of analise.pausas) {
    if (p.inicio > cursor) trocos.push({ inicio: cursor, fim: p.inicio })
    cursor = Math.max(cursor, p.fim)
  }
  if (cursor < analise.duracao) trocos.push({ inicio: cursor, fim: analise.duracao })
  // Um troço mais curto do que dois frames não sobrevive ao reencode e só
  // acrescenta um salto na imagem.
  return trocos.filter((t) => t.fim - t.inicio > 0.1)
}

/** Texto curto para a interface: «3 pausas · 12 s poupados». */
export function resumo(analise: AnaliseDeAudio): { pausas: number; poupanca: number; pct: number } {
  return {
    pausas: analise.pausas.length,
    poupanca: Math.round(analise.poupanca * 10) / 10,
    pct: analise.duracao > 0 ? Math.round((analise.poupanca / analise.duracao) * 100) : 0,
  }
}
