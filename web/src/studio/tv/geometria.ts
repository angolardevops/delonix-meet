/**
 * Geometria de uma fonte — como a imagem é POSTA no rectângulo, não como as
 * suas cores são tratadas (isso é `correccao.ts`).
 *
 * Existe porque uma câmara nem sempre chega direita: um telemóvel em pé dá
 * retrato, uma câmara de tecto pode vir ao contrário, e uma câmara virada ao
 * operador costuma querer-se espelhada. Antes disto, endireitar uma fonte
 * obrigava a mexer na FONTE (relançar o produtor com outra rotação), e o
 * browser largava e reabria o dispositivo — no Estúdio via-se a imagem cair e
 * ter de se ligar outra vez.
 *
 * Aqui a volta é dada no DESENHO: o `MediaStream` não é tocado, por isso a
 * fonte não pisca e a mudança aparece ao mesmo tempo na pré-visualização, na
 * mesa de corte e no que vai para o ar.
 */

/** Quartos de volta, no sentido dos ponteiros do relógio. */
export type Rotacao = 0 | 90 | 180 | 270

export interface GeometriaDaFonte {
  rotacao: Rotacao
  /** Espelho horizontal, aplicado ANTES da rotação (como um espelho de selfie). */
  espelho: boolean
}

export const GEOMETRIA_NEUTRA: GeometriaDaFonte = { rotacao: 0, espelho: false }

export const ROTACOES: readonly Rotacao[] = [0, 90, 180, 270]

export function sanearGeometria(v: unknown): GeometriaDaFonte {
  const o = (v && typeof v === 'object' ? v : {}) as Record<string, unknown>
  const r = Number(o.rotacao)
  return {
    rotacao: (ROTACOES as readonly number[]).includes(r) ? (r as Rotacao) : 0,
    espelho: o.espelho === true,
  }
}

export function ehDireita(g: GeometriaDaFonte): boolean {
  return g.rotacao === 0 && !g.espelho
}

/** `true` quando a volta troca largura por altura (um retrato vira paisagem). */
export function trocaOsLados(g: GeometriaDaFonte): boolean {
  return g.rotacao === 90 || g.rotacao === 270
}

/** A volta seguinte, para um botão que roda sempre no mesmo sentido. */
export function rodar(g: GeometriaDaFonte, sentido: 1 | -1 = 1): GeometriaDaFonte {
  const i = ROTACOES.indexOf(g.rotacao)
  return { ...g, rotacao: ROTACOES[(i + sentido + ROTACOES.length) % ROTACOES.length] }
}
