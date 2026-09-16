import { useCallback, useEffect, useRef, useState } from 'react'
import { capacidadeDaFila } from './stripCapacity'

/**
 * Mede uma fila de retratos (`.rm-strip`, `.rm-wbpeople`…) e diz quantos
 * itens cabem. A direcção lê-se do CSS (a mesma fila é coluna no desktop e
 * linha no telemóvel); o tamanho de um item lê-se do primeiro que existir.
 * Enquanto não há medida, cabe tudo — nunca se esconde ninguém às cegas.
 */
export function useStripCapacity(itemSelector: string) {
  const [capacity, setCapacity] = useState(Infinity)
  const elRef = useRef<HTMLElement | null>(null)
  const roRef = useRef<ResizeObserver | null>(null)

  const compute = useCallback(() => {
    const el = elRef.current
    if (!el) return
    const cs = getComputedStyle(el)
    const vertical = cs.flexDirection.startsWith('column')
    const gap = parseFloat(vertical ? cs.rowGap : cs.columnGap) || 0
    const item = el.querySelector<HTMLElement>(itemSelector)
    let disponivel = vertical ? el.clientHeight : el.clientWidth
    // O cabeçalho da fila («13 PARTICIPANTES») ocupa lugar na coluna.
    const head = el.querySelector<HTMLElement>('[data-strip-head]')
    if (vertical && head && head.offsetParent) disponivel -= head.offsetHeight + gap
    const tamanho = item ? (vertical ? item.offsetHeight : item.offsetWidth) : 0
    const cap = capacidadeDaFila(disponivel, tamanho, gap)
    setCapacity((c) => (c === cap ? c : cap))
  }, [itemSelector])

  const ref = useCallback(
    (node: HTMLElement | null) => {
      roRef.current?.disconnect()
      roRef.current = null
      elRef.current = node
      if (!node) return
      const ro = new ResizeObserver(() => compute())
      ro.observe(node)
      roRef.current = ro
      compute()
    },
    [compute],
  )

  // Um item novo (primeiro retrato) muda o tamanho de referência.
  useEffect(() => {
    compute()
  })

  useEffect(() => () => roRef.current?.disconnect(), [])

  return { ref, capacity }
}
