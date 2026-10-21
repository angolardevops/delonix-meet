/**
 * A MESA FÍSICA: uma superfície de controlo MIDI a comandar o estúdio.
 *
 * O PROBLEMA QUE RESOLVE. Uma mesa de som ligada por USB já entrava no estúdio
 * — como ÁUDIO, na lista de microfones. O que não havia era o CONTROLO: os
 * faders dela não mexiam nos faders do ecrã, os botões não cortavam, e as luzes
 * dela não sabiam o que estava no ar. `grep -i "requestMIDI|navigator.hid|OSC"`
 * dava zero em todo o repo. É a diferença entre «o PC tem uma mesa ligada» e «a
 * mesa comanda o PC».
 *
 * PORQUE MIDI E NÃO HID. Quase toda a superfície de controlo de áudio fala MIDI
 * (Mackie Control, APC, X-Touch, nanoKONTROL, e as mesas digitais com modo de
 * controlo), e o browser dá-lhe acesso sem driver. O HID puro exigia um
 * descritor por modelo. O OSC exigia rede e um servidor — não é para o browser.
 *
 * O QUE ESTE MÓDULO É: as REGRAS, puras e testáveis — ler uma mensagem MIDI,
 * traduzi-la numa acção do estúdio, e dizer que luz acender de volta. Abrir o
 * acesso e falar com o dispositivo é do `useMesaFisica`.
 *
 * O MAPA É FIXO NESTA VERSÃO, e isso está escrito no ecrã. «Aprender» (carregar
 * num fader e dizer-lhe o que faz) é o passo seguinte: sem isso, uma superfície
 * com outra numeração obriga a mudar código, e é por isso que o painel mostra a
 * mensagem crua que recebeu — para quem tem outra mesa saber o que pedir.
 */

/** Uma mensagem MIDI já lida. `canal` é 0–15. */
export type MensagemMidi =
  | { tipo: 'controlo'; canal: number; controlo: number; valor: number }
  | { tipo: 'nota'; canal: number; nota: number; velocidade: number; ligada: boolean }
  | { tipo: 'fader'; canal: number; valor: number }

/**
 * Lê os bytes de uma mensagem MIDI.
 *
 * `null` para o que não interessa (relógio, afinação, sysex): uma superfície
 * manda dezenas de mensagens por segundo que não são comandos, e tratá-las
 * como tal faria o estúdio tremer sozinho.
 */
export function lerMidi(dados: Uint8Array | number[]): MensagemMidi | null {
  const d = Array.from(dados)
  if (d.length < 2) return null
  const estado = d[0]
  const canal = estado & 0x0f
  const tipo = estado & 0xf0
  if (tipo === 0xb0) return { tipo: 'controlo', canal, controlo: d[1], valor: d[2] ?? 0 }
  // Note-off, e também note-on com velocidade 0: é assim que metade das
  // superfícies larga um botão, e tratá-lo como «carregado» deixava o botão
  // preso para sempre.
  if (tipo === 0x80) return { tipo: 'nota', canal, nota: d[1], velocidade: 0, ligada: false }
  if (tipo === 0x90) {
    const v = d[2] ?? 0
    return { tipo: 'nota', canal, nota: d[1], velocidade: v, ligada: v > 0 }
  }
  // Pitch bend: é o fader motorizado das superfícies tipo Mackie, com 14 bits.
  if (tipo === 0xe0) return { tipo: 'fader', canal, valor: ((d[2] ?? 0) << 7) | d[1] }
  return null
}

/** O que a mesa física pode mandar fazer ao estúdio. */
export type AccaoFisica =
  | { tipo: 'fader'; canal: number; posicao: number }
  | { tipo: 'mestre'; posicao: number }
  | { tipo: 'mudo'; canal: number }
  | { tipo: 'solo'; canal: number }
  | { tipo: 'efeito'; indice: number }
  | { tipo: 'cortar' }
  | { tipo: 'misturar' }

/**
 * O MAPA de origem.
 *
 * Os números são os do consenso das superfícies baratas (nanoKONTROL e as que
 * o imitam): os oito primeiros controlos contínuos são os faders, e a primeira
 * fila de botões é o mudo. O que não cabe nessa convenção — o solo, os efeitos,
 * o corte — fica a seguir, em ordem.
 */
export const MAPA = {
  /** CC 0–7: faders dos canais 1–8. */
  faderPrimeiro: 0,
  faderUltimo: 7,
  /** CC 8: fader de mestre. */
  mestre: 8,
  /** Notas 0–7: mudo dos canais 1–8. */
  mudoPrimeiro: 0,
  /** Notas 8–15: solo dos canais 1–8. */
  soloPrimeiro: 8,
  /** Notas 16–19: as quatro teclas de efeito. */
  efeitoPrimeiro: 16,
  /** Nota 20: cortar. Nota 21: misturar. */
  cortar: 20,
  misturar: 21,
} as const

export const CANAIS_DA_SUPERFICIE = 8
const MAX_MIDI = 127
const MAX_FADER_14 = 16383

/** 0–127 (ou 0–16383, no fader de 14 bits) para a posição 0–1 de um fader. */
const posicao = (v: number, max: number) => Math.min(1, Math.max(0, v / max))

/**
 * Traduz uma mensagem numa acção. `null` quando aquela mensagem não está
 * mapeada — e é isso que mantém o estúdio quieto perante uma superfície que
 * manda o que lhe apetece.
 *
 * Os botões agem ao CARREGAR, não ao largar: numa emissão, o corte tem de
 * acontecer quando o dedo desce.
 */
export function accaoDaMesaFisica(m: MensagemMidi): AccaoFisica | null {
  if (m.tipo === 'fader') {
    // Pitch bend por canal MIDI: o canal 0 é o fader 1.
    if (m.canal >= CANAIS_DA_SUPERFICIE) return null
    return { tipo: 'fader', canal: m.canal, posicao: posicao(m.valor, MAX_FADER_14) }
  }
  if (m.tipo === 'controlo') {
    if (m.controlo >= MAPA.faderPrimeiro && m.controlo <= MAPA.faderUltimo) {
      return { tipo: 'fader', canal: m.controlo - MAPA.faderPrimeiro, posicao: posicao(m.valor, MAX_MIDI) }
    }
    if (m.controlo === MAPA.mestre) return { tipo: 'mestre', posicao: posicao(m.valor, MAX_MIDI) }
    return null
  }
  if (!m.ligada) return null
  const n = m.nota
  if (n >= MAPA.mudoPrimeiro && n < MAPA.mudoPrimeiro + CANAIS_DA_SUPERFICIE) {
    return { tipo: 'mudo', canal: n - MAPA.mudoPrimeiro }
  }
  if (n >= MAPA.soloPrimeiro && n < MAPA.soloPrimeiro + CANAIS_DA_SUPERFICIE) {
    return { tipo: 'solo', canal: n - MAPA.soloPrimeiro }
  }
  if (n >= MAPA.efeitoPrimeiro && n < MAPA.efeitoPrimeiro + 4) return { tipo: 'efeito', indice: n - MAPA.efeitoPrimeiro }
  if (n === MAPA.cortar) return { tipo: 'cortar' }
  if (n === MAPA.misturar) return { tipo: 'misturar' }
  return null
}

/**
 * As LUZES de volta: uma nota por botão, 127 acesa e 0 apagada.
 *
 * É a metade que faz a superfície servir para alguma coisa numa emissão: sem
 * eco, o operador olha para o ecrã para saber se o canal 3 está mudo — e nesse
 * caso mais valia carregar no ecrã. Uma superfície sem luzes ignora isto, e não
 * se parte nada.
 */
export function lucesDaMesa(estados: readonly { mudo: boolean; solo: boolean }[]): number[][] {
  const saida: number[][] = []
  estados.slice(0, CANAIS_DA_SUPERFICIE).forEach((e, i) => {
    saida.push([0x90, MAPA.mudoPrimeiro + i, e.mudo ? 127 : 0])
    saida.push([0x90, MAPA.soloPrimeiro + i, e.solo ? 127 : 0])
  })
  return saida
}

/** O browser deixa falar com uma mesa física? */
export function midiSuportado(): boolean {
  return typeof navigator !== 'undefined' && typeof navigator.requestMIDIAccess === 'function'
}

/** Como se descreve uma mensagem no painel, para quem tem outra mesa. */
export function descreverMidi(m: MensagemMidi): string {
  if (m.tipo === 'controlo') return `CC ${m.controlo} = ${m.valor}`
  if (m.tipo === 'fader') return `fader ${m.canal + 1} = ${m.valor}`
  return `${m.ligada ? 'nota' : 'larga'} ${m.nota}`
}
