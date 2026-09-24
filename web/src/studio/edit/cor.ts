/**
 * Correcção de cor: UMA tradução dos quatro cursores para filtros, usada pela
 * pré-visualização (CSS no `<video>`) e pela exportação (`ctx.filter` do
 * canvas, que aceita a mesma sintaxe). Se as duas tivessem contas próprias, o
 * que se vê a editar não era o que saía no ficheiro.
 *
 * A temperatura não tem filtro CSS: é uma camada de cor em `soft-light` por
 * cima — âmbar para quente, azul para frio. São constantes de PROCESSAMENTO de
 * imagem, não cores de interface, por isso não vêm dos tokens.
 */
import type { Canto, Cor } from './projecto'

export function filtroCss(c: Cor): string {
  const partes: string[] = []
  if (c.exposicao) partes.push(`brightness(${(1 + c.exposicao * 0.6).toFixed(3)})`)
  if (c.contraste) partes.push(`contrast(${(1 + c.contraste / 100).toFixed(3)})`)
  if (c.saturacao) partes.push(`saturate(${(1 + c.saturacao / 100).toFixed(3)})`)
  return partes.length ? partes.join(' ') : 'none'
}

/** Camada de temperatura: `null` quando neutra. */
export function camadaDeTemperatura(c: Cor): { rgb: string; alfa: number } | null {
  if (!c.temperatura) return null
  const alfa = Math.min(0.5, Math.abs(c.temperatura) / 200)
  return { rgb: c.temperatura > 0 ? '255, 147, 41' : '64, 156, 255', alfa }
}

/** Rótulo com sinal, como o template: `+12`, `−4`, `0`. */
export function valorComSinal(v: number, casas = 0): string {
  const r = Number(v.toFixed(casas))
  if (r === 0) return '0'
  return `${r > 0 ? '+' : '−'}${Math.abs(r).toFixed(casas)}`
}

/** Posição (fracções) de um elemento de largura/altura `w`,`h` num canto, com margem. */
export function posicaoNoCanto(canto: Canto, w: number, h: number, margem = 0.03): { x: number; y: number } {
  const direita = canto.endsWith('direito')
  const baixo = canto.startsWith('inferior')
  return { x: direita ? 1 - margem - w : margem, y: baixo ? 1 - margem - h : margem }
}
