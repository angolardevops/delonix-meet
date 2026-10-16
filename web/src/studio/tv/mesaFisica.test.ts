/**
 * A mesa física: ler MIDI e traduzi-lo em acções do estúdio.
 *
 * Tudo o que decide o que uma superfície faz ao ar está aqui, puro: abrir o
 * acesso e falar com o dispositivo é do hook, e isso não se prova sem browser
 * nem sem a mesa em cima da secretária.
 */
import { describe, expect, it } from 'vitest'
import { accaoDaMesaFisica, CANAIS_DA_SUPERFICIE, descreverMidi, lerMidi, lucesDaMesa, MAPA } from './mesaFisica'

describe('ler MIDI', () => {
  it('controlo contínuo, nota e fader de 14 bits', () => {
    expect(lerMidi([0xb0, 3, 100])).toEqual({ tipo: 'controlo', canal: 0, controlo: 3, valor: 100 })
    expect(lerMidi([0x92, 20, 127])).toEqual({ tipo: 'nota', canal: 2, nota: 20, velocidade: 127, ligada: true })
    // 14 bits: o byte alto vem DEPOIS do baixo.
    expect(lerMidi([0xe0, 0x00, 0x40])).toEqual({ tipo: 'fader', canal: 0, valor: 8192 })
  })

  it('e uma nota com velocidade zero é LARGAR, não carregar', () => {
    // É assim que metade das superfícies larga um botão. Tratá-la como
    // «carregado» deixava o botão preso para sempre — e o canal mudo sem
    // ninguém o ter calado.
    expect(lerMidi([0x90, 5, 0])).toMatchObject({ ligada: false })
    expect(lerMidi([0x80, 5, 64])).toMatchObject({ ligada: false })
  })

  it('e o que não é comando não é lido', () => {
    // Uma superfície manda dezenas de mensagens por segundo que não são
    // comandos (relógio, afinação): tratá-las como tal fazia o estúdio tremer.
    expect(lerMidi([0xf8])).toBeNull()
    expect(lerMidi([0xd0, 64])).toBeNull()
    expect(lerMidi([])).toBeNull()
  })
})

describe('traduzir em acções', () => {
  it('os oito primeiros controlos são faders, e o nono é o mestre', () => {
    expect(accaoDaMesaFisica(lerMidi([0xb0, MAPA.faderPrimeiro, 127])!)).toEqual({ tipo: 'fader', canal: 0, posicao: 1 })
    expect(accaoDaMesaFisica(lerMidi([0xb0, MAPA.faderUltimo, 0])!)).toEqual({ tipo: 'fader', canal: 7, posicao: 0 })
    expect(accaoDaMesaFisica(lerMidi([0xb0, MAPA.mestre, 127])!)).toEqual({ tipo: 'mestre', posicao: 1 })
  })

  it('as filas de botões são mudo, solo, efeitos, e o corte', () => {
    const nota = (n: number) => accaoDaMesaFisica(lerMidi([0x90, n, 127])!)
    expect(nota(MAPA.mudoPrimeiro + 2)).toEqual({ tipo: 'mudo', canal: 2 })
    expect(nota(MAPA.soloPrimeiro + 7)).toEqual({ tipo: 'solo', canal: 7 })
    expect(nota(MAPA.efeitoPrimeiro + 3)).toEqual({ tipo: 'efeito', indice: 3 })
    expect(nota(MAPA.cortar)).toEqual({ tipo: 'cortar' })
    expect(nota(MAPA.misturar)).toEqual({ tipo: 'misturar' })
  })

  it('e os botões agem ao CARREGAR, nunca ao largar', () => {
    // Numa emissão, o corte tem de acontecer quando o dedo desce. Se também
    // agisse ao largar, cada carregar dava dois cortes.
    expect(accaoDaMesaFisica(lerMidi([0x90, MAPA.cortar, 0])!)).toBeNull()
    expect(accaoDaMesaFisica(lerMidi([0x80, MAPA.cortar, 64])!)).toBeNull()
  })

  it('e o que não está mapeado não faz nada', () => {
    // É isto que mantém o estúdio quieto perante uma superfície que manda o
    // que lhe apetece.
    expect(accaoDaMesaFisica(lerMidi([0xb0, 60, 100])!)).toBeNull()
    expect(accaoDaMesaFisica(lerMidi([0x90, 99, 127])!)).toBeNull()
    expect(accaoDaMesaFisica({ tipo: 'fader', canal: CANAIS_DA_SUPERFICIE, valor: 0 })).toBeNull()
  })
})

describe('as luzes de volta', () => {
  it('uma nota por botão, acesa a 127 e apagada a 0', () => {
    const l = lucesDaMesa([{ mudo: true, solo: false }, { mudo: false, solo: true }])
    expect(l).toEqual([
      [0x90, MAPA.mudoPrimeiro, 127],
      [0x90, MAPA.soloPrimeiro, 0],
      [0x90, MAPA.mudoPrimeiro + 1, 0],
      [0x90, MAPA.soloPrimeiro + 1, 127],
    ])
  })

  it('e uma mesa com mais canais do que botões não escreve fora da superfície', () => {
    const muitos = Array.from({ length: 20 }, () => ({ mudo: false, solo: false }))
    expect(lucesDaMesa(muitos)).toHaveLength(CANAIS_DA_SUPERFICIE * 2)
  })
})

describe('descrever', () => {
  it('diz a mensagem crua, para quem tem outra mesa saber o que pedir', () => {
    expect(descreverMidi(lerMidi([0xb0, 7, 64])!)).toBe('CC 7 = 64')
    expect(descreverMidi(lerMidi([0x90, 16, 127])!)).toBe('nota 16')
    expect(descreverMidi(lerMidi([0x90, 16, 0])!)).toBe('larga 16')
  })
})
