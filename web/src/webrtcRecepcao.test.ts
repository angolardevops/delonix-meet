/**
 * Quem não ENVIA um tipo de media tem de ter por onde o RECEBER.
 *
 * O defeito (reportado de uma chamada a 2026-10-13: «entra com micro e câmara
 * off e fica sem som e vídeo»): a via de recepção só se criava quando não havia
 * faixa local NENHUMA — `getTracks().length === 0`. Quem entrasse só com áudio
 * tinha uma faixa, a condição dava falso, e a oferta saía sem m-line de vídeo.
 * Resultado: nunca mais via ninguém, por muito que os outros publicassem.
 *
 * O que o teste guarda é o CASO PARCIAL, que é o que faltava — e os dois
 * sentidos dele, porque entrar só com câmara tem o mesmo problema do outro
 * lado.
 */
import { describe, expect, it } from 'vitest'
import { tiposSemEnvio } from './webrtc'

describe('tiposSemEnvio', () => {
  it('sem faixa nenhuma (espectador): precisa dos dois', () => {
    expect(tiposSemEnvio([])).toEqual(['audio', 'video'])
  })

  it('SÓ ÁUDIO — o caso reportado: falta a via do vídeo', () => {
    expect(tiposSemEnvio(['audio'])).toEqual(['video'])
  })

  it('só vídeo: falta a via do áudio', () => {
    expect(tiposSemEnvio(['video'])).toEqual(['audio'])
  })

  it('com os dois: não se acrescenta nada', () => {
    expect(tiposSemEnvio(['audio', 'video'])).toEqual([])
  })

  it('a ordem das faixas não importa', () => {
    expect(tiposSemEnvio(['video', 'audio'])).toEqual([])
  })

  it('faixas repetidas (simulcast dá vários senders de vídeo) não confundem', () => {
    expect(tiposSemEnvio(['video', 'video', 'video', 'audio'])).toEqual([])
    expect(tiposSemEnvio(['video', 'video', 'video'])).toEqual(['audio'])
  })

  it('um sender sem faixa (`track` a null) não conta como envio', () => {
    // É o caso de um transceiver `recvonly` já existente: o sender existe mas
    // não envia nada. Contá-lo faria o remédio não ser aplicado.
    expect(tiposSemEnvio([undefined, undefined])).toEqual(['audio', 'video'])
    expect(tiposSemEnvio(['audio', undefined])).toEqual(['video'])
  })
})
