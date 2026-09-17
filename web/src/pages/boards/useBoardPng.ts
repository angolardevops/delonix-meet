/**
 * PNG de um quadro como object URL. O endpoint exige Bearer, por isso um
 * `<img src="/api/whiteboards/…/image">` cru dá «unauthorized»: vai-se buscar
 * autenticado e mostra-se o blob. O URL é revogado ao desmontar.
 */
import { useEffect, useState } from 'react'
import { authedBlobUrl, whiteboardPngUrl } from '../../api'

export type BoardPng = { s: 'loading' } | { s: 'ready'; url: string } | { s: 'error' }

export function useBoardPng(id: string, enabled = true): BoardPng {
  const [png, setPng] = useState<BoardPng>({ s: 'loading' })
  useEffect(() => {
    if (!enabled) return
    let live = true
    let made = ''
    setPng({ s: 'loading' })
    authedBlobUrl(whiteboardPngUrl(id))
      .then((u) => {
        if (live) {
          made = u
          setPng({ s: 'ready', url: u })
        } else URL.revokeObjectURL(u)
      })
      .catch(() => {
        if (live) setPng({ s: 'error' })
      })
    return () => {
      live = false
      if (made) URL.revokeObjectURL(made)
    }
  }, [id, enabled])
  return png
}
