import { describe, expect, it } from 'vitest'
import { metaCurta, metaLonga, videoMeta } from './mediaMeta'
import { reuniaoMaisProxima } from './salaInfo'

const track = (s: MediaTrackSettings, kind = 'video', readyState = 'live') =>
  ({ kind, readyState, getSettings: () => s }) as unknown as MediaStreamTrack

describe('pré-entrada · o que se mostra vem da fonte, não do template', () => {
  it('a reunião agendada é a DESTA sala e a mais perto de agora', () => {
    const agora = Date.parse('2026-09-16T10:05:00Z')
    const ms = [
      { room_code: 'abc-def', starts_at: '2026-09-15T10:00:00Z' },
      { room_code: 'outra', starts_at: '2026-09-16T10:00:00Z' },
      { room_code: 'abc-def', starts_at: '2026-09-16T10:00:00Z' },
      { room_code: 'abc-def', starts_at: '2026-09-23T10:00:00Z' },
    ]
    expect(reuniaoMaisProxima(ms, 'abc-def', agora)).toBe(ms[2])
    expect(reuniaoMaisProxima(ms, 'sem-reuniao', agora)).toBeNull()
  })

  it('resolução e fps lidos da track (o lado menor, também em retrato)', () => {
    expect(videoMeta(track({ width: 1920, height: 1080, frameRate: 30 }))).toEqual({ lines: 1080, fps: 30 })
    expect(videoMeta(track({ width: 1080, height: 1920, frameRate: 29.97 }))).toEqual({ lines: 1080, fps: 30 })
    expect(metaLonga({ lines: 2160, fps: 25 }, 'fps')).toBe('2160p · 25 fps')
    expect(metaCurta({ lines: 1080, fps: 30 })).toBe('1080p30')
  })

  it('sem números, não se inventa nenhum', () => {
    expect(videoMeta(null)).toBeNull()
    expect(videoMeta(track({}))).toBeNull()
    expect(videoMeta(track({ width: 640, height: 480 }, 'audio'))).toBeNull()
    expect(videoMeta(track({ width: 640, height: 480 }, 'video', 'ended'))).toBeNull()
    expect(metaCurta({ lines: 720, fps: 0 })).toBe('720p')
  })
})
