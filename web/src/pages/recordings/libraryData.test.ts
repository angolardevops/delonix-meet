import { describe, expect, it } from 'vitest'
import type { RecordingLibraryItem } from '../../api'
import {
  chapterAt,
  filterCounts,
  formatClock,
  kindGroup,
  matchesFilter,
  parseTags,
  percentOf,
  resolutionLabel,
  segmentAt,
  splitSpeaker,
  visibleState,
} from './libraryData'

const item = (over: Partial<RecordingLibraryItem> = {}): RecordingLibraryItem => ({
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
  duration_ms: 60000,
  width: 1280,
  height: 720,
  fps: 25,
  video_codec: 'vp8',
  audio_codec: 'opus',
  has_thumbnail: true,
  transcript_status: 'ready',
  transcript_language: 'pt',
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

describe('visibleState', () => {
  it('falhada manda sobre tudo', () => {
    expect(visibleState(item({ status: 'failed', state: 'published' })).kind).toBe('failed')
  })
  it('a processar traz a percentagem do servidor', () => {
    expect(visibleState(item({ state: 'processing', status: 'processing', progress_pct: 74 }))).toEqual({ kind: 'processing', pct: 74 })
  })
  it('pronta com transcrição a correr lê-se «a transcrever»', () => {
    expect(visibleState(item({ transcript_status: 'transcribing' })).kind).toBe('transcribing')
  })
  it('publicada não mostra retenção', () => {
    expect(visibleState(item({ state: 'published' }), 90).kind).toBe('published')
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
    expect(resolutionLabel(item({ width: 3840, height: 2160 }))).toBe('4K')
    expect(resolutionLabel(item({ width: 1920, height: 1080 }))).toBe('1080p')
    expect(resolutionLabel(item({ width: 1080, height: 1920 }))).toBe('1080p')
    expect(resolutionLabel(item({ width: null, height: null }))).toBeNull()
  })
  it('a híbrida conta como videoaula', () => {
    expect(kindGroup('hybrid')).toBe('training')
    expect(kindGroup('broadcast')).toBe('broadcast')
    expect(kindGroup('meeting')).toBe('meeting')
  })
  it('contagens por chip; uma falhada não entra nas categorias', () => {
    const list = [
      item({ kind: 'training' }),
      item({ kind: 'hybrid', width: 3840, height: 2160 }),
      item({ kind: 'broadcast', state: 'processing', status: 'processing' }),
      item({ kind: 'meeting', status: 'failed', state: 'failed' }),
    ]
    expect(filterCounts(list)).toEqual({ all: 4, training: 2, broadcast: 1, meeting: 0, '4k': 1, processing: 1, failed: 1 })
    expect(matchesFilter(list[3], 'meeting')).toBe(false)
  })
})

describe('tempo, capítulos e transcrição', () => {
  it('relógio com minutos de dois algarismos e horas quando as há', () => {
    expect(formatClock(0)).toBe('00:00')
    expect(formatClock(521_000)).toBe('08:41')
    expect(formatClock(3_775_000)).toBe('1:02:55')
    expect(formatClock(null)).toBe('—')
  })
  it('capítulo e segmento em curso', () => {
    const ch = [{ t_ms: 0 }, { t_ms: 41000 }, { t_ms: 92000 }]
    expect(chapterAt(ch, 50000)).toBe(1)
    expect(chapterAt([{ t_ms: 5000 }], 0)).toBe(-1)
    const seg = [{ start_ms: 0, end_ms: 5000 }, { start_ms: 41000, end_ms: 46000 }]
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
