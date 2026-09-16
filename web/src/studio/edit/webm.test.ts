/**
 * O desmultiplexador lê o que o `webm-muxer` escreve — com tamanhos conhecidos
 * e em modo «streaming» (tamanhos desconhecidos, como o MediaRecorder).
 * Os bytes dos frames são inventados: o que se testa é o contentor.
 */
import { describe, expect, it } from 'vitest'
import { ArrayBufferTarget, Muxer } from 'webm-muxer'
import { codecParaDecoder, lerWebm } from './webm'

function fabricar(streaming: boolean): Uint8Array {
  const alvo = new ArrayBufferTarget()
  const muxer = new Muxer({
    target: alvo,
    video: { codec: 'V_VP9', width: 640, height: 360 },
    audio: { codec: 'A_OPUS', sampleRate: 48000, numberOfChannels: 2 },
    streaming,
    firstTimestampBehavior: 'offset',
  })
  for (let i = 0; i < 90; i++) {
    const dados = new Uint8Array([i, i + 1, i + 2, 0xff])
    muxer.addVideoChunkRaw(dados, i % 30 === 0 ? 'key' : 'delta', Math.round((i * 1e6) / 30))
    muxer.addAudioChunkRaw(new Uint8Array([7, i]), 'key', i * 20_000)
  }
  muxer.finalize()
  return new Uint8Array(alvo.buffer)
}

describe('lerWebm', () => {
  for (const streaming of [false, true]) {
    it(streaming ? 'tamanhos desconhecidos (como o MediaRecorder)' : 'tamanhos conhecidos', () => {
      const w = lerWebm(fabricar(streaming))
      const video = w.pistas.find((p) => p.tipo === 'video')!
      const audio = w.pistas.find((p) => p.tipo === 'audio')!
      expect(video).toMatchObject({ codec: 'V_VP9', largura: 640, altura: 360 })
      expect(audio).toMatchObject({ codec: 'A_OPUS', taxa: 48000, canais: 2 })

      const vs = w.blocos.filter((b) => b.pista === video.numero)
      expect(vs).toHaveLength(90)
      // Precisão de 1 ms (TimecodeScale omisso).
      vs.forEach((b, i) => {
        expect(Math.abs(b.tempo - (i * 1e6) / 30)).toBeLessThanOrEqual(1000)
        expect(b.chave).toBe(i % 30 === 0)
        expect(Array.from(b.dados)).toEqual([i, i + 1, i + 2, 0xff])
      })
      expect(w.blocos.filter((b) => b.pista === audio.numero)).toHaveLength(90)
    })
  }

  it('ficheiro truncado não rebenta — lê o que houver', () => {
    const b = fabricar(true)
    const w = lerWebm(b.subarray(0, b.length - 200))
    expect(w.pistas.length).toBe(2)
    expect(w.blocos.length).toBeGreaterThan(10)
  })
})

describe('codecParaDecoder', () => {
  const base = { numero: 1, tipo: 'video' as const, privado: null, largura: 1, altura: 1, taxa: 0, canais: 0 }
  it('VP8, VP9 e AVC com descrição', () => {
    expect(codecParaDecoder({ ...base, codec: 'V_VP8' })?.codec).toBe('vp8')
    expect(codecParaDecoder({ ...base, codec: 'V_VP9' })?.codec).toBe('vp09.00.10.08')
    const avc = codecParaDecoder({ ...base, codec: 'V_MPEG4/ISO/AVC', privado: new Uint8Array([1, 0x64, 0x00, 0x28, 0xff]) })
    expect(avc?.codec).toBe('avc1.640028')
    expect(avc?.description).toBeDefined()
    expect(codecParaDecoder({ ...base, codec: 'V_THEORA' })).toBeNull()
  })
})
