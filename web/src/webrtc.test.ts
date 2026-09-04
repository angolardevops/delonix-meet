import { describe, it, expect } from 'vitest'
import { enhanceOpus, SCREEN_CONSTRAINTS } from './webrtc'

// Uma SDP com DUAS m-lines de áudio: microfone + áudio do ecrã partilhado.
// É exatamente o caso que o `replace` não-global tratava a meio.
const SDP_TWO_AUDIO = [
  'v=0',
  'm=audio 9 UDP/TLS/RTP/SAVPF 111',
  'a=rtpmap:111 opus/48000/2',
  'a=fmtp:111 minptime=10;useinbandfec=1',
  'm=video 9 UDP/TLS/RTP/SAVPF 96',
  'a=rtpmap:96 VP8/90000',
  'm=audio 9 UDP/TLS/RTP/SAVPF 111',
  'a=rtpmap:111 opus/48000/2',
  'a=fmtp:111 minptime=10;useinbandfec=1',
].join('\r\n')

describe('enhanceOpus', () => {
  it('aplica o fmtp a TODAS as m-lines de áudio, não só à primeira', () => {
    const out = enhanceOpus(SDP_TWO_AUDIO)
    const tuned = out.split('\r\n').filter((l) => l.startsWith('a=fmtp:111') && l.includes('maxaveragebitrate=128000'))
    // Antes só a primeira era tratada e o áudio do ecrã ficava nos defaults.
    expect(tuned).toHaveLength(2)
  })

  it('liga DTX — em silêncio o encoder deixa de gastar banda', () => {
    expect(enhanceOpus(SDP_TWO_AUDIO)).toContain('usedtx=1')
  })

  it('preserva os parâmetros que já lá estavam', () => {
    expect(enhanceOpus(SDP_TWO_AUDIO)).toContain('minptime=10')
  })

  it('cria a linha fmtp quando o rtpmap existe sem ela', () => {
    const sdp = ['m=audio 9 UDP/TLS/RTP/SAVPF 111', 'a=rtpmap:111 opus/48000/2'].join('\r\n')
    const out = enhanceOpus(sdp)
    expect(out).toContain('a=fmtp:111 maxaveragebitrate=128000')
  })

  it('não mexe em SDP sem Opus', () => {
    const sdp = 'm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=rtpmap:96 VP8/90000'
    expect(enhanceOpus(sdp)).toBe(sdp)
  })

  it('é idempotente (aplicado à oferta E à resposta)', () => {
    const once = enhanceOpus(SDP_TWO_AUDIO)
    expect(enhanceOpus(once)).toBe(once)
  })
})

describe('SCREEN_CONSTRAINTS', () => {
  // O ecrã NÃO tem simulcast: o que se captura aqui é o que todos os
  // subscritores recebem. Sem teto, um monitor 4K era enviado a 4K.
  it('limita a captura do ecrã a 1080p e framerate baixo', () => {
    const v = SCREEN_CONSTRAINTS.video as MediaTrackConstraints
    expect((v.width as ConstrainULongRange).max).toBeLessThanOrEqual(1920)
    expect((v.height as ConstrainULongRange).max).toBeLessThanOrEqual(1080)
    expect((v.frameRate as ConstrainDoubleRange).max).toBeLessThanOrEqual(15)
  })

  it('não processa o áudio do sistema (não é voz)', () => {
    expect(SCREEN_CONSTRAINTS.audio).toMatchObject({ echoCancellation: false, autoGainControl: false })
  })
})
