import { describe, expect, it } from 'vitest'
import { deveTrocarFonte, escolherFontePip, type EstadoPip } from './pipPolicy'

const sala = (p: Partial<EstadoPip> = {}): EstadoPip => ({
  apresentacao: null, afixado: null, ultimoAFalar: null, candidatos: [], ...p,
})
const c = (peerId: string, temVideo = true, aFalar = false) => ({ peerId, temVideo, aFalar })

describe('escolherFontePip · quem aparece na janela flutuante', () => {
  it('sala só de áudio não abre janela nenhuma', () => {
    expect(escolherFontePip(sala({ candidatos: [c('a', false), c('b', false)] }))).toBe(null)
  })

  it('sala vazia não abre janela nenhuma', () => {
    expect(escolherFontePip(sala())).toBe(null)
  })

  it('a apresentação passa à frente de tudo — até de quem está afixado e a falar', () => {
    expect(escolherFontePip(sala({
      apresentacao: 'ecra', afixado: 'a', candidatos: [c('a', true, true), c('b')],
    }))).toBe('ecra')
  })

  it('a apresentação ganha mesmo sem candidatos: o ecrã não é câmara de ninguém', () => {
    expect(escolherFontePip(sala({ apresentacao: 'ecra' }))).toBe('ecra')
  })

  it('afixar alguém passa à frente de quem está a falar', () => {
    expect(escolherFontePip(sala({
      afixado: 'a', candidatos: [c('a'), c('b', true, true)],
    }))).toBe('a')
  })

  it('afixar alguém SEM câmara não prende a janela num quadrado preto', () => {
    expect(escolherFontePip(sala({
      afixado: 'a', candidatos: [c('a', false), c('b', true, true)],
    }))).toBe('b')
  })

  it('sem afixado, mostra quem fala', () => {
    expect(escolherFontePip(sala({ candidatos: [c('a'), c('b', true, true)] }))).toBe('b')
  })

  it('no silêncio, fica no último que falou — não salta para o primeiro da lista', () => {
    expect(escolherFontePip(sala({
      ultimoAFalar: 'b', candidatos: [c('a'), c('b'), c('z')],
    }))).toBe('b')
  })

  it('se o último a falar desligou a câmara, cai para quem a tem', () => {
    expect(escolherFontePip(sala({
      ultimoAFalar: 'b', candidatos: [c('a'), c('b', false)],
    }))).toBe('a')
  })

  it('a própria pessoa não entra na lista — quem chama é que a exclui', () => {
    // O contrato está aqui em forma de teste para não se perder: `candidatos`
    // NUNCA inclui o próprio. Ver ao pé de `pipCandidatos` em Room.tsx.
    const escolhido = escolherFontePip(sala({ candidatos: [c('outro')] }))
    expect(escolhido).toBe('outro')
  })
})

describe('deveTrocarFonte · a janela não pisca a cada frase', () => {
  const tres = sala({ candidatos: [c('a', true, true), c('b'), c('c')] })

  it('sem fonte, abre', () => {
    expect(deveTrocarFonte(null, tres)).toBe(true)
  })

  it('numa conversa a três, quem está na janela FICA lá — mesmo quando outro fala', () => {
    expect(deveTrocarFonte('b', tres)).toBe(false)
  })

  it('troca quando quem lá está desliga a câmara', () => {
    expect(deveTrocarFonte('b', sala({ candidatos: [c('a'), c('b', false)] }))).toBe(true)
  })

  it('troca quando quem lá está sai da sala', () => {
    expect(deveTrocarFonte('b', sala({ candidatos: [c('a')] }))).toBe(true)
  })

  it('uma apresentação a começar interrompe — é conteúdo, e é o que se ia perder', () => {
    expect(deveTrocarFonte('b', sala({ apresentacao: 'ecra', candidatos: [c('b')] }))).toBe(true)
  })

  it('já a mostrar a apresentação, não se troca por nada', () => {
    expect(deveTrocarFonte('ecra', sala({
      apresentacao: 'ecra', afixado: 'a', candidatos: [c('a', true, true)],
    }))).toBe(false)
  })

  it('afixar alguém é ordem directa da pessoa — interrompe', () => {
    expect(deveTrocarFonte('b', sala({ afixado: 'a', candidatos: [c('a'), c('b')] }))).toBe(true)
  })

  it('afixar quem JÁ está na janela não a mexe', () => {
    expect(deveTrocarFonte('b', sala({ afixado: 'b', candidatos: [c('a'), c('b')] }))).toBe(false)
  })
})
