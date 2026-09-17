/**
 * Miniatura de uma gravação: a do servidor (`…/thumbnail`, jpeg) quando
 * `hasThumbnail`; sem ela, o fundo por nome — nunca um fotograma inventado.
 * O `<img>` não envia o Bearer, por isso a imagem chega como URL de objecto.
 */
import { useEffect, useState } from 'react'
import { recordingThumbnailUrl } from '../../api'
import { thumbBackground } from './format'
import type { RecordingView } from './recordingView'

export function useThumbnail(rec: Pick<RecordingView, 'id' | 'hasThumbnail' | 'failed'>): string | null {
  const [url, setUrl] = useState<string | null>(null)
  useEffect(() => {
    setUrl(null)
    if (!rec.hasThumbnail || rec.failed) return
    let live = true
    let made = ''
    recordingThumbnailUrl(rec.id)
      .then((u) => {
        if (live) {
          made = u
          setUrl(u)
        } else URL.revokeObjectURL(u)
      })
      // 404: o servidor deixou de a ter — fica o fundo por nome.
      .catch(() => undefined)
    return () => {
      live = false
      if (made) URL.revokeObjectURL(made)
    }
  }, [rec.id, rec.hasThumbnail, rec.failed])
  return url
}

/** Estilo de fundo de uma miniatura: a imagem do servidor ou o tom por nome. */
export function thumbStyle(url: string | null, name: string): { background: string } {
  return url ? { background: `center / cover no-repeat url("${url}")` } : { background: thumbBackground(name) }
}
