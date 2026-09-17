/**
 * Sub-recursos de uma gravação convertidos para o que o ecrã usa. Separado da
 * `recordingView.ts` para o mapeamento continuar puro (testável sem browser).
 */
import { isAbort, recordingChapters, recordingTranscript } from '../../api'
import type { ChapterView, RecordingView, SegmentView } from './recordingView'

/** Capítulos do servidor, por instante. `null` = não foi possível lê-los (sem acesso, rede). */
export async function loadChapters(rec: RecordingView, _signal?: AbortSignal): Promise<ChapterView[] | null> {
  if (rec.failed) return []
  try {
    const list = await recordingChapters(rec.id)
    return list.map((c) => ({ id: c.id, tMs: c.t_ms, title: c.title, auto: c.source === 'auto' }))
  } catch (e) {
    if (isAbort(e)) throw e
    return null
  }
}

/**
 * Segmentos da transcrição com instante relativo ao vídeo. `null` quando a
 * gravação não tem transcrição pronta com tempos: aí fica a das notas da sala.
 */
export async function loadSegments(rec: RecordingView, signal?: AbortSignal): Promise<SegmentView[] | null> {
  if (rec.failed || !rec.transcriptReady) return null
  try {
    const tr = await recordingTranscript(rec.id, signal)
    if (tr.status !== 'ready' || tr.segments.length === 0) return null
    return tr.segments.map((x) => ({ startMs: x.start_ms, endMs: x.end_ms, text: x.text }))
  } catch (e) {
    if (isAbort(e)) throw e
    return null
  }
}
