/**
 * Corte automático por voz — a decisão, sem Web Audio.
 *
 * Recebe, a cada tique, o nível (dBFS) de cada canal ligado a uma fonte e
 * diz para que fonte cortar, ou nada. Três regras, todas medidas contra o
 * «pingue-pongue» que um corte ingénuo faz numa conversa:
 *
 *   1. VOZ: o canal tem de estar acima do limiar (`limiarDb`) e acima dos
 *      outros por uma margem (`margemDb`) — duas pessoas a falar ao mesmo
 *      tempo não fazem cortar;
 *   2. PERSISTÊNCIA: tem de ganhar durante `seguraMs` seguidos — uma tosse
 *      ou um «hum» não mudam o plano;
 *   3. RITMO: nunca menos de `intervaloMinimoMs` entre dois cortes.
 */

export interface OpcoesDoCorteDeVoz {
  limiarDb: number
  margemDb: number
  seguraMs: number
  intervaloMinimoMs: number
}

export const CORTE_DE_VOZ_INICIAL: OpcoesDoCorteDeVoz = {
  limiarDb: -42,
  margemDb: 6,
  seguraMs: 900,
  intervaloMinimoMs: 3000,
}

export interface EstadoDoCorteDeVoz {
  candidato: string | null
  desde: number
  ultimoCorte: number
}

export const CORTE_DE_VOZ_ZERO: EstadoDoCorteDeVoz = { candidato: null, desde: 0, ultimoCorte: -Infinity }

/** Nível por FONTE (o maior dos canais dessa fonte). */
export type NiveisPorFonte = ReadonlyMap<string, number>

export interface DecisaoDeVoz {
  estado: EstadoDoCorteDeVoz
  /** A fonte para onde cortar agora, ou `null`. */
  cortarPara: string | null
}

export function decidirCorteDeVoz(
  estado: EstadoDoCorteDeVoz,
  niveis: NiveisPorFonte,
  noAr: string | null,
  agora: number,
  op: OpcoesDoCorteDeVoz = CORTE_DE_VOZ_INICIAL,
): DecisaoDeVoz {
  const ordenados = [...niveis.entries()].filter(([, db]) => Number.isFinite(db)).sort((a, b) => b[1] - a[1])
  const [primeiro, segundo] = ordenados
  const vencedor =
    primeiro && primeiro[1] >= op.limiarDb && (!segundo || primeiro[1] - segundo[1] >= op.margemDb) ? primeiro[0] : null

  if (!vencedor) return { estado: { ...estado, candidato: null, desde: 0 }, cortarPara: null }
  if (vencedor === noAr) return { estado: { ...estado, candidato: vencedor, desde: agora }, cortarPara: null }

  const desde = estado.candidato === vencedor ? estado.desde : agora
  const seguro = agora - desde >= op.seguraMs
  const ritmo = agora - estado.ultimoCorte >= op.intervaloMinimoMs
  if (seguro && ritmo) {
    return { estado: { candidato: vencedor, desde: agora, ultimoCorte: agora }, cortarPara: vencedor }
  }
  return { estado: { ...estado, candidato: vencedor, desde }, cortarPara: null }
}
