/**
 * O que uma fonte de vídeo ESTÁ a entregar — lido da própria track
 * (`getSettings()`), nunca do que se pediu. Pedir 4K a uma webcam 1080p devolve
 * 1080p, e é isso que se mostra.
 */
export interface VideoMeta {
  /** Linhas (lado menor): 1080 para 1920×1080, também em retrato. */
  lines: number
  fps: number
}

export function videoMeta(track: MediaStreamTrack | null | undefined): VideoMeta | null {
  if (!track || track.kind !== 'video' || track.readyState === 'ended') return null
  const s = track.getSettings()
  const w = s.width ?? 0
  const h = s.height ?? 0
  const lines = w && h ? Math.min(w, h) : h || w
  if (!lines) return null
  return { lines, fps: Math.round(s.frameRate ?? 0) }
}

/** «1080p · 30 fps» — a etiqueta da pré-visualização. */
export function metaLonga(m: VideoMeta, fpsWord: string): string {
  return m.fps ? `${m.lines}p · ${m.fps} ${fpsWord}` : `${m.lines}p`
}

/** «1080p30» — a meta curta da lista de câmaras. */
export function metaCurta(m: VideoMeta): string {
  return m.fps ? `${m.lines}p${m.fps}` : `${m.lines}p`
}
