/**
 * O codificador adaptativo do directo: as regras que se podem medir sem
 * browser — o nível H.264 e a ordem dos perfis.
 *
 * O resto (codificar, muxar, adaptar) só se prova num Chromium com WebCodecs,
 * e está por fazer: ver o cabeçalho da PR que trouxe este ficheiro.
 */
import { describe, expect, it } from 'vitest'
import { candidatosH264, nivelH264 } from './codificadorDoDirecto'

describe('o nível H.264 sai do tamanho e dos fps', () => {
  it('sobe com a área e com os quadros', () => {
    expect(nivelH264(1280, 720, 30)).toBe('1f') // 3.1
    expect(nivelH264(1920, 1080, 30)).toBe('28') // 4.0
    expect(nivelH264(1920, 1080, 50)).toBe('2a') // 4.2
    expect(nivelH264(3840, 2160, 30)).toBe('33') // 5.1
    expect(nivelH264(3840, 2160, 50)).toBe('34') // 5.2
  })

  it('e um nível baixo de mais não passa por lá sem se notar', () => {
    // O mesmo tamanho ao DOBRO dos quadros não pode ficar no mesmo nível: era
    // assim que uma emissão a 50 fps declarava um nível que não cumpria, e a
    // plataforma do outro lado a recusava sem dizer porquê.
    expect(nivelH264(1920, 1080, 30)).not.toBe(nivelH264(1920, 1080, 60))
  })
})

describe('os candidatos de codec', () => {
  it('vão do melhor ao mais compatível, todos no mesmo nível', () => {
    const c = candidatosH264(1920, 1080, 30)
    expect(c).toEqual(['avc1.640028', 'avc1.4d4028', 'avc1.42e028'])
    // High primeiro: melhor imagem pelo mesmo débito. Baseline por último:
    // é o que um descodificador velho aceita.
    expect(c[0].startsWith('avc1.64')).toBe(true)
    expect(c[c.length - 1].startsWith('avc1.42')).toBe(true)
  })
})
