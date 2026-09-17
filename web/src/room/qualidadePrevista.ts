/**
 * «Qualidade prevista» da pré-entrada (template DelonixPrejoin): várias
 * sondagens curtas de descarga e subida contra ESTE servidor (`netProbe`), e o
 * que elas dizem da ligação. Puro — o `usePrejoin` corre as sondagens.
 */
export interface AmostraRede {
  downKbps: number
  upKbps: number
}

/** Quantas sondagens (barras). O servidor aceita 30 por conta por minuto. */
export const SONDAGENS = 8

/** O que se pode enviar com esta ligação: o gargalo é o MENOR dos dois sentidos. */
export type Veredicto = '4k' | '1080p' | '720p' | 'baixa' | 'fraca'

export function veredicto(amostras: AmostraRede[]): Veredicto | null {
  if (amostras.length === 0) return null
  // Mediana dos gargalos: uma sondagem má isolada não condena a ligação.
  const g = amostras.map((a) => Math.min(a.downKbps, a.upKbps)).sort((a, b) => a - b)
  const med = g[Math.floor(g.length / 2)]
  if (med >= 25_000) return '4k'
  if (med >= 8_000) return '1080p'
  if (med >= 2_500) return '720p'
  if (med >= 600) return 'baixa'
  return 'fraca'
}

/** Altura (0..1) de cada barra e se é uma quebra (abaixo de 70 % da mediana). */
export function barras(amostras: AmostraRede[]): { altura: number; quebra: boolean }[] {
  const g = amostras.map((a) => Math.min(a.downKbps, a.upKbps))
  if (g.length === 0) return []
  const max = Math.max(...g)
  const ordenado = [...g].sort((a, b) => a - b)
  const med = ordenado[Math.floor(ordenado.length / 2)]
  return g.map((v) => ({ altura: max > 0 ? Math.max(0.08, v / max) : 0.08, quebra: v < med * 0.7 }))
}

/** Médias de descarga e subida (kbit/s). */
export function medias(amostras: AmostraRede[]): { down: number; up: number } {
  const n = amostras.length || 1
  return {
    down: amostras.reduce((s, a) => s + a.downKbps, 0) / n,
    up: amostras.reduce((s, a) => s + a.upKbps, 0) / n,
  }
}

/** «48 Mbps», «3,5 Mbps», «640 kbps». */
export function debito(kbps: number, locale: string): string {
  if (kbps >= 10_000) return `${Math.round(kbps / 1000).toLocaleString(locale)} Mbps`
  if (kbps >= 1_000) return `${(kbps / 1000).toLocaleString(locale, { maximumFractionDigits: 1 })} Mbps`
  return `${Math.round(kbps).toLocaleString(locale)} kbps`
}
