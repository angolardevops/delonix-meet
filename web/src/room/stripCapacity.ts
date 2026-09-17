/**
 * Quantos retratos cabem numa fila (plateia ao lado/em baixo, fila do quadro,
 * telemóvel) — e quantos ficam por detrás de um «+N». Puro: a medição do DOM
 * vive em `useStripCapacity`.
 */
export function capacidadeDaFila(disponivel: number, tamanhoItem: number, gap: number): number {
  if (!(disponivel > 0) || !(tamanhoItem > 0)) return Infinity
  return Math.max(1, Math.floor((disponivel + gap) / (tamanhoItem + gap)))
}

/** Com excesso, o último lugar é do «+N»: mostra-se `capacidade − 1`. */
export function repartirFila(total: number, capacidade: number): { mostrar: number; resto: number } {
  if (total <= capacidade) return { mostrar: total, resto: 0 }
  const mostrar = Math.max(0, capacidade - 1)
  return { mostrar, resto: total - mostrar }
}
