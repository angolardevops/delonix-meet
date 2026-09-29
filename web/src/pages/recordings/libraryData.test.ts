import { describe, expect, it } from 'vitest'
import {
  chapterAt,
  filterCounts,
  formatClock,
  parseClock,
  kindGroup,
  matchesFilter,
  parseTags,
  percentOf,
  resolutionLabel,
  segmentAt,
  splitSpeaker,
  visibleFilters,
  visibleState,
} from './libraryData'
import type { RecordingLibraryItem } from '../../api'
import { fromRecordingItem, RecordingView } from './recordingView'

export const libItem = (over: Partial<RecordingLibraryItem> = {}): RecordingLibraryItem => ({
  id: 'x',
  room_id: 'r',
  uploader_id: 'u',
  filename: 'a.webm',
  size_bytes: 10,
  created_at: '2026-09-01T10:00:00Z',
  room_code: 'abc',
  uploader_name: 'Ana',
  owned: true,
  share_count: 0,
  can_download: true,
  status: 'ready',
  failure_reason: null,
  state: 'ready',
  progress_pct: null,
  kind: 'meeting',
  duration_ms: null,
  width: null,
  height: null,
  fps: null,
  video_codec: null,
  audio_codec: null,
  has_thumbnail: false,
  transcript_status: 'none',
  transcript_language: null,
  transcribed_at: null,
  chapter_count: 0,
  comment_count: 0,
  view_count: 0,
  participant_count: 0,
  caption_languages: [],
  description: '',
  tags: [],
  visibility: 'private',
  published_at: null,
  can_manage: true,
  uploader_org_id: null,
  uploader_org_name: null,
  ...over,
})

const item = (over: Partial<RecordingView> = {}): RecordingView => ({ ...fromRecordingItem(libItem()), ...over })

describe('visibleState', () => {
  it('falhada manda sobre tudo', () => {
    expect(visibleState(item({ pipeline: 'failed', failed: true })).kind).toBe('failed')
  })
  it('a transcrever traz a percentagem do servidor', () => {
    expect(visibleState(item({ pipeline: 'transcribing', progressPct: 74 }))).toEqual({ kind: 'transcribing', pct: 74 })
  })
  it('pronta com transcrição a correr lê-se «a transcrever»', () => {
    expect(visibleState(item({ transcriptRunning: true })).kind).toBe('transcribing')
  })
  it('publicada não mostra retenção', () => {
    expect(visibleState(item({ pipeline: 'published' }), 90).kind).toBe('published')
  })
  it('retenção conta os dias que faltam, arredondados para cima, nunca negativos', () => {
    const now = new Date('2026-09-11T09:00:00Z').getTime()
    expect(visibleState(item(), 90, now)).toEqual({ kind: 'retained', days: 81 })
    expect(visibleState(item(), 1, now)).toEqual({ kind: 'retained', days: 0 })
    expect(visibleState(item(), 0, now).kind).toBe('ready')
  })
})

describe('resolução e filtros', () => {
  it('rótulos medidos, nunca inventados', () => {
    expect(resolutionLabel({ width: 3840, height: 2160 })).toBe('4K')
    expect(resolutionLabel({ width: 1920, height: 1080 })).toBe('1080p')
    expect(resolutionLabel({ width: 1080, height: 1920 })).toBe('1080p')
    expect(resolutionLabel(item())).toBeNull()
  })
  it('a híbrida conta como videoaula', () => {
    expect(kindGroup(null)).toBeNull()
    expect(kindGroup('hybrid')).toBe('training')
    expect(kindGroup('broadcast')).toBe('broadcast')
    expect(kindGroup('meeting')).toBe('meeting')
  })
  it('contagens por chip; uma falhada não entra nas categorias', () => {
    const list = [
      item({ category: 'training' }),
      item({ category: 'hybrid', width: 3840, height: 2160, owned: false }),
      item({ category: 'broadcast', pipeline: 'transcribing' }),
      item({ category: 'meeting', pipeline: 'failed', failed: true }),
    ]
    expect(filterCounts(list)).toEqual({ all: 4, mine: 3, shared: 1, training: 2, broadcast: 1, meeting: 0, '4k': 1, transcribing: 1, failed: 1 })
    expect(matchesFilter(list[3], 'meeting')).toBe(false)
    expect(visibleFilters(list)).toEqual(['all', 'training', 'broadcast', 'meeting', '4k', 'transcribing', 'failed'])
  })
  it('sem dado do servidor não há chip que filtraria para zero', () => {
    const hoje = [item({ category: null }), item({ category: null, owned: false })]
    expect(visibleFilters(hoje)).toEqual(['all', 'mine', 'shared'])
  })
  it('a camada de mapeamento passa o que o servidor mediu e deixa null o que ele não diz', () => {
    const medido = fromRecordingItem(
      libItem({ duration_ms: 125000, width: 1920, height: 1080, kind: 'training', view_count: 3, chapter_count: 2, description: 'd', tags: ['a'] }),
    )
    expect([medido.durationMs, medido.width, medido.category, medido.viewCount, medido.chapterCount, medido.description, medido.tags]).toEqual([
      125000, 1920, 'training', 3, 2, 'd', ['a'],
    ])
    const semMedida = fromRecordingItem(libItem())
    expect([semMedida.durationMs, semMedida.width, semMedida.height]).toEqual([null, null, null])
    expect(fromRecordingItem(libItem({ status: 'failed', state: 'failed', size_bytes: 0 })).sizeBytes).toBeNull()
  })
  it('estado: a transcrever, publicada, falhada — sem «a processar»', () => {
    expect(fromRecordingItem(libItem({ status: 'transcribing', state: 'transcribing' })).pipeline).toBe('transcribing')
    expect(fromRecordingItem(libItem({ state: 'published', visibility: 'org' })).pipeline).toBe('published')
    expect(fromRecordingItem(libItem({ status: 'failed', state: 'failed' })).pipeline).toBe('failed')
    expect(fromRecordingItem(libItem({ status: 'failed', state: 'failed', visibility: 'org' })).published).toBe(false)
  })
})

describe('tempo, capítulos e transcrição', () => {
  it('relógio com minutos de dois algarismos e horas quando as há', () => {
    expect(formatClock(0)).toBe('00:00')
    expect(formatClock(521_000)).toBe('08:41')
    expect(formatClock(3_775_000)).toBe('1:02:55')
    expect(formatClock(null)).toBe('—')
  })
  it('lê um instante escrito à mão, e o formatClock volta a dar o mesmo', () => {
    expect(parseClock('08:41')).toBe(521_000)
    expect(parseClock('1:02:55')).toBe(3_775_000)
    expect(parseClock('95')).toBe(95_000)
    expect(parseClock(formatClock(521_000))).toBe(521_000)
    expect(parseClock('1:75')).toBeNull()
    expect(parseClock('a:10')).toBeNull()
    expect(parseClock('')).toBeNull()
  })
  it('capítulo e segmento em curso', () => {
    const ch = [{ tMs: 0 }, { tMs: 41000 }, { tMs: 92000 }]
    expect(chapterAt(ch, 50000)).toBe(1)
    expect(chapterAt([{ tMs: 5000 }], 0)).toBe(-1)
    const seg = [{ startMs: 0 }, { startMs: 41000 }]
    expect(segmentAt(seg, 42000)).toBe(1)
    expect(segmentAt(seg, 10000)).toBe(0)
  })
  it('orador à frente do texto', () => {
    expect(splitSpeaker('Ana Mbala: O failover foi corrigido.')).toEqual({ speaker: 'Ana Mbala', text: 'O failover foi corrigido.' })
    expect(splitSpeaker('Às 10:30 começamos')).toEqual({ speaker: null, text: 'Às 10:30 começamos' })
  })
  it('posição na barra e etiquetas', () => {
    expect(percentOf(30000, 60000)).toBe(50)
    expect(percentOf(1, null)).toBeNull()
    expect(parseTags('#formação, voz  #Voz sip')).toEqual(['formação', 'voz', 'sip'])
  })
})
