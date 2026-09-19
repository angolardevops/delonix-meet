/**
 * Quem aparece na janela flutuante (Picture-in-Picture) da sala.
 *
 * PORQUÊ um módulo à parte: a janela de PiP mostra UM vídeo. Quando alguém sai,
 * desliga a câmara ou pára de apresentar, alguma coisa tem de decidir para onde
 * a janela salta — e essa decisão corre dezenas de vezes por reunião, sem DOM à
 * frente. Aqui é pura: entra o estado da sala, sai um `peerId`. Testa-se e
 * muta-se sem browser (a mesma razão do `layerPolicy.ts`).
 *
 * A ORDEM vem do que a pessoa foi lá fazer, não do que é fácil de calcular:
 * quem sai da separador quer continuar a ver **aquilo que estava a seguir**.
 *
 *   1. A apresentação. Se alguém partilha ecrã, é isso que se está a seguir —
 *      é o único caso em que a janela mostra conteúdo e não uma cara.
 *   2. O participante afixado. Foi uma escolha explícita da pessoa; nada a
 *      passa à frente excepto uma apresentação.
 *   3. Quem está a falar AGORA, se tiver câmara ligada.
 *   4. O último a falar com câmara ligada. Sem isto a janela ficaria em branco
 *      em cada silêncio — e o silêncio é a maior parte de uma reunião.
 *   5. Qualquer participante com câmara.
 *
 * O próprio utilizador NUNCA é candidato: pôr a nossa cara numa janela
 * flutuante enquanto estamos noutro separador não serve para nada, e a câmara
 * própria já se vê no separador da sala.
 */

export interface PipCandidato {
  peerId: string
  /** Tem vídeo a chegar AGORA — não «tem câmara no dispositivo». */
  temVideo: boolean
  /** Está a falar neste instante. */
  aFalar: boolean
}

export interface EstadoPip {
  /** `peerId` de quem apresenta, ou `null`. Ganha sempre. */
  apresentacao: string | null
  /** `peerId` afixado pela pessoa, ou `null`. */
  afixado: string | null
  /** `peerId` do último a falar, mesmo que já esteja calado. */
  ultimoAFalar: string | null
  candidatos: PipCandidato[]
}

/**
 * Devolve o `peerId` a mostrar, ou `null` quando não há nada que valha a pena —
 * uma sala só de áudio, por exemplo. `null` quer dizer «não abras a janela»,
 * nunca «abre-a vazia»: uma janela preta a flutuar por cima do trabalho de
 * alguém é pior do que janela nenhuma.
 */
export function escolherFontePip(estado: EstadoPip): string | null {
  const { apresentacao, afixado, ultimoAFalar, candidatos } = estado
  // A apresentação passa à frente mesmo sem estar na lista de candidatos: o
  // stream de ecrã não é o de câmara de ninguém.
  if (apresentacao) return apresentacao

  const comVideo = candidatos.filter((c) => c.temVideo)
  if (comVideo.length === 0) return null

  const oAfixado = comVideo.find((c) => c.peerId === afixado)
  if (oAfixado) return oAfixado.peerId

  const aFalar = comVideo.find((c) => c.aFalar)
  if (aFalar) return aFalar.peerId

  const ultimo = comVideo.find((c) => c.peerId === ultimoAFalar)
  if (ultimo) return ultimo.peerId

  return comVideo[0].peerId
}

/**
 * Trocar a janela de PiP de pessoa custa: o browser faz um corte visível e, em
 * algumas versões, a janela pisca. Por isso só se troca quando a fonte actual
 * DEIXOU de servir — não sempre que a escolha ideal muda.
 *
 * O caso que isto resolve: numa conversa a três, `escolherFontePip` alterna de
 * cara a cada frase. Sem esta guarda, a janela ficava a piscar de segundo a
 * segundo. Com ela, fica em quem estava até essa pessoa desligar a câmara,
 * sair, ou até alguém começar a apresentar.
 */
export function deveTrocarFonte(actual: string | null, estado: EstadoPip): boolean {
  if (actual === null) return true
  // Uma apresentação a começar é o único caso que interrompe uma fonte válida:
  // é conteúdo, e ninguém que sai do separador quer perder o ecrã partilhado.
  if (estado.apresentacao && estado.apresentacao !== actual) return true
  if (estado.apresentacao === actual) return false
  // Afixar alguém é uma ordem directa da pessoa — também interrompe.
  if (estado.afixado && estado.afixado !== actual) return true
  const ainda = estado.candidatos.find((c) => c.peerId === actual)
  return !ainda || !ainda.temVideo
}
