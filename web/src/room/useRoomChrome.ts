import { useCallback, useEffect, useRef, useState } from 'react'

export type Panel = 'none' | 'chat' | 'qa' | 'polls' | 'people' | 'settings' | 'notes' | 'multicam'

/**
 * O que é só da VISTA: painel lateral aberto, cartão «reunião pronta».
 *
 * Esc fecha o painel e o foco VOLTA a quem o abriu (R104). Não se prende o
 * foco dentro do painel: um `<aside>` não é um modal, e prender lá dentro
 * impediria de chegar aos controlos da chamada.
 */
export function useRoomChrome(code: string, peersCount: number, inRoom: boolean) {
  const [panel, setPanelState] = useState<Panel>('none')
  const focoAntesDoPainel = useRef<HTMLElement | null>(null)

  const setPanel = useCallback((next: Panel) => {
    setPanelState((cur) => {
      if (cur === 'none' && next !== 'none') focoAntesDoPainel.current = document.activeElement as HTMLElement | null
      return next
    })
  }, [])
  const togglePanel = useCallback((p: Panel) => setPanel(panel === p ? 'none' : p), [panel, setPanel])

  const closePanel = useCallback(() => {
    setPanelState('none')
    focoAntesDoPainel.current?.focus?.()
  }, [])

  useEffect(() => {
    if (panel === 'none') return
    const onEsc = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return
      // Um diálogo aberto trata do seu próprio Esc.
      if (document.querySelector('.dx-dialog')) return
      closePanel()
    }
    window.addEventListener('keydown', onEsc)
    return () => window.removeEventListener('keydown', onEsc)
  }, [panel, closePanel])

  // Cartão «reunião pronta»: fecha-se SOZINHO (R87) — alguém entrou, abriu-se
  // um painel, ou passaram 20 s. O `dx_ready_` impede que volte nesta sessão.
  const [readyOpen, setReadyOpen] = useState(() => !sessionStorage.getItem(`dx_ready_${code}`))
  const dismissReady = useCallback(() => {
    sessionStorage.setItem(`dx_ready_${code}`, '1')
    setReadyOpen(false)
  }, [code])
  // Os 20 s contam a partir de o cartão APARECER — não desde a pré-entrada.
  useEffect(() => {
    if (!readyOpen || !inRoom) return
    const id = setTimeout(dismissReady, 20_000)
    return () => clearTimeout(id)
  }, [readyOpen, inRoom, dismissReady])
  useEffect(() => {
    if (readyOpen && (peersCount > 0 || panel !== 'none')) dismissReady()
  }, [readyOpen, peersCount, panel, dismissReady])

  return { panel, setPanel, togglePanel, closePanel, readyOpen, dismissReady }
}
