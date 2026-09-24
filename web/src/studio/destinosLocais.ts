/**
 * O estado de cada cartão de destino, só com o que ESTE browser sabe: se o
 * destino tem chave e em que fase está a emissão (uma ligação, que o servidor
 * reparte). Saúde, débito e perdas por destino ficam de fora até o estado por
 * destino do servidor ser contrato desta UI. Puro — testado em
 * `destinosLocais.test.ts`.
 */
import type { Destino, EstadoDoDirecto } from './directo'

export type EstadoDoCartao = 'sem-chave' | 'pronto' | 'a-ligar' | 'no-ar' | 'erro'

export function estadoDoCartao(fase: EstadoDoDirecto['fase'], temChave: boolean): EstadoDoCartao {
  // Sem chave o destino não vai na ligação: não está «no ar», mesmo com a
  // emissão a decorrer para os outros.
  if (!temChave) return 'sem-chave'
  if (fase === 'parado') return 'pronto'
  return fase
}

/** Contagem do cabeçalho («2 no ar · 1 sem chave»). */
export function contagemDosDestinos(
  destinos: readonly Pick<Destino, 'chave'>[],
  fase: EstadoDoDirecto['fase'],
): Record<EstadoDoCartao, number> {
  const r: Record<EstadoDoCartao, number> = { 'sem-chave': 0, pronto: 0, 'a-ligar': 0, 'no-ar': 0, erro: 0 }
  for (const d of destinos) r[estadoDoCartao(fase, !!d.chave.trim())]++
  return r
}

/** A ordem em que as partes da contagem aparecem. */
export const ORDEM_DA_CONTAGEM: readonly EstadoDoCartao[] = ['no-ar', 'a-ligar', 'erro', 'pronto', 'sem-chave']
