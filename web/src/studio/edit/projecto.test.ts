/**
 * A lista de edições é o coração do editor não destrutivo: se a matemática da
 * linha de tempo errar, imagem, som e legendas desalinham-se e só se nota no
 * ficheiro exportado. Testa-se aqui, em Node, com projectos construídos à mão.
 */
import { describe, expect, it } from 'vitest'
import * as H from './historico'
import {
  clipEm,
  clipsDaFaixa,
  cortarCues,
  duracaoDoClip,
  duracaoDoProjecto,
  editar,
  fimDoClip,
  Fonte,
  intervalosDaFonteNaLinha,
  mapearTempo,
  montarInicial,
  normalizarIntervalos,
  novoProjecto,
  Projecto,
  subtrair,
  tempoNaFonte,
} from './projecto'

function fonteAv(id = 'f1', duracao = 60): Fonte {
  return { id, nome: 'take.webm', tipo: 'av', origem: 'completo', duracao, largura: 1920, altura: 1080, bytes: 1000, criadaEm: 0 }
}

function comClip(duracao = 60): Projecto {
  let p = novoProjecto('Aula', 0)
  p = editar(p, { tipo: 'fonte', fonte: fonteAv('f1', duracao) }, 1)
  return editar(p, { tipo: 'inserir', fonteId: 'f1', faixa: 'V1', inicio: 0 }, 2)
}

describe('intervalos', () => {
  it('normaliza: ordena, junta sobreposições e deita fora os vazios', () => {
    expect(normalizarIntervalos([{ inicio: 5, fim: 7 }, { inicio: 1, fim: 2 }, { inicio: 6, fim: 9 }, { inicio: 3, fim: 3 }])).toEqual([
      { inicio: 1, fim: 2 },
      { inicio: 5, fim: 9 },
    ])
  })

  it('mapeia o tempo fechando os buracos, e devolve null dentro de um removido', () => {
    const is = [{ inicio: 10, fim: 15 }, { inicio: 20, fim: 22 }]
    expect(mapearTempo(5, is)).toBe(5)
    expect(mapearTempo(12, is)).toBeNull()
    expect(mapearTempo(18, is)).toBe(13)
    expect(mapearTempo(30, is)).toBe(23)
  })

  it('subtrai intervalos de um troço', () => {
    expect(subtrair(0, 30, [{ inicio: 10, fim: 15 }, { inicio: 20, fim: 40 }])).toEqual([
      { inicio: 0, fim: 10 },
      { inicio: 15, fim: 20 },
    ])
  })
})

describe('inserir', () => {
  it('uma fonte com vídeo e áudio entra em V1 com o áudio LIGADO em A1', () => {
    const p = comClip()
    const v = clipsDaFaixa(p, 'V1')
    const a = clipsDaFaixa(p, 'A1')
    expect(v).toHaveLength(1)
    expect(a).toHaveLength(1)
    expect(v[0].grupo).toBeTruthy()
    expect(v[0].grupo).toBe(a[0].grupo)
    expect(duracaoDoProjecto(p)).toBe(60)
  })

  it('sem início, acrescenta no fim da faixa', () => {
    let p = comClip(10)
    p = editar(p, { tipo: 'fonte', fonte: fonteAv('f2', 5) })
    p = editar(p, { tipo: 'inserir', fonteId: 'f2', faixa: 'V1' })
    expect(clipsDaFaixa(p, 'V1').map((c) => c.inicio)).toEqual([0, 10])
    expect(duracaoDoProjecto(p)).toBe(15)
  })

  it('recusa sobrepor um clipe existente (devolve o MESMO projecto)', () => {
    const p = comClip(10)
    const q = editar(p, { tipo: 'inserir', fonteId: 'f1', faixa: 'V1', inicio: 5 })
    expect(q).toBe(p)
  })

  it('não põe áudio em faixa de vídeo', () => {
    let p = novoProjecto('x')
    p = editar(p, { tipo: 'fonte', fonte: { ...fonteAv('a'), tipo: 'audio' } })
    expect(editar(p, { tipo: 'inserir', fonteId: 'a', faixa: 'V1' })).toBe(p)
  })
})

describe('lâmina (dividir)', () => {
  it('divide vídeo e áudio ligados no mesmo instante, mantendo a ligação por metade', () => {
    const p = editar(comClip(), { tipo: 'dividir', t: 20 })
    const v = clipsDaFaixa(p, 'V1')
    const a = clipsDaFaixa(p, 'A1')
    expect(v.map((c) => [c.inicio, c.entrada, c.saida])).toEqual([
      [0, 0, 20],
      [20, 20, 60],
    ])
    expect(a.map((c) => [c.inicio, c.entrada, c.saida])).toEqual([
      [0, 0, 20],
      [20, 20, 60],
    ])
    expect(v[0].grupo).toBe(a[0].grupo)
    expect(v[1].grupo).toBe(a[1].grupo)
    expect(v[0].grupo).not.toBe(v[1].grupo)
  })

  it('respeita a velocidade: a 2×, 10 s na linha de tempo são 20 s na fonte', () => {
    let p = comClip()
    const id = clipsDaFaixa(p, 'V1')[0].id
    p = editar(p, { tipo: 'velocidade', clipId: id, velocidade: 2 })
    p = editar(p, { tipo: 'dividir', t: 10 })
    expect(clipsDaFaixa(p, 'V1').map((c) => [c.inicio, c.entrada, c.saida])).toEqual([
      [0, 0, 20],
      [10, 20, 60],
    ])
  })

  it('não divide numa faixa bloqueada', () => {
    let p = comClip()
    p = editar(p, { tipo: 'faixa', id: 'A1', patch: { bloqueada: true } })
    p = editar(p, { tipo: 'dividir', t: 20 })
    expect(clipsDaFaixa(p, 'V1')).toHaveLength(2)
    expect(clipsDaFaixa(p, 'A1')).toHaveLength(1)
  })
})

describe('aparar, deslizar, mover, remover', () => {
  it('aparar sem ripple deixa um buraco; com ripple puxa o que vem a seguir', () => {
    let p = comClip(10)
    p = editar(p, { tipo: 'fonte', fonte: fonteAv('f2', 5) })
    p = editar(p, { tipo: 'inserir', fonteId: 'f2', faixa: 'V1' })
    const [c1] = clipsDaFaixa(p, 'V1')
    const sem = editar(p, { tipo: 'aparar', clipId: c1.id, saida: 6, ripple: false })
    expect(clipsDaFaixa(sem, 'V1').map((c) => c.inicio)).toEqual([0, 10])
    const com = editar(p, { tipo: 'aparar', clipId: c1.id, saida: 6, ripple: true })
    expect(clipsDaFaixa(com, 'V1').map((c) => c.inicio)).toEqual([0, 6])
    expect(clipsDaFaixa(com, 'A1').map((c) => [c.inicio, c.saida])).toEqual([
      [0, 6],
      [6, 5],
    ])
  })

  it('aparar a entrada sem ripple move o início na linha de tempo', () => {
    const p = comClip(10)
    const c = clipsDaFaixa(p, 'V1')[0]
    const q = editar(p, { tipo: 'aparar', clipId: c.id, entrada: 3, ripple: false })
    expect(clipsDaFaixa(q, 'V1')[0]).toMatchObject({ inicio: 3, entrada: 3, saida: 10 })
  })

  it('aparar nunca passa dos limites da fonte', () => {
    const p = comClip(10)
    const c = clipsDaFaixa(p, 'V1')[0]
    const q = editar(p, { tipo: 'aparar', clipId: c.id, entrada: -5, saida: 99, ripple: true })
    expect(q).toBe(p)
  })

  it('deslizar muda o conteúdo sem mudar a posição nem a duração', () => {
    let p = comClip(60)
    const c = clipsDaFaixa(p, 'V1')[0]
    p = editar(p, { tipo: 'aparar', clipId: c.id, entrada: 10, saida: 20, ripple: true })
    p = editar(p, { tipo: 'deslizar', clipId: c.id, delta: 5 })
    expect(clipsDaFaixa(p, 'V1')[0]).toMatchObject({ inicio: 0, entrada: 15, saida: 25 })
    expect(clipsDaFaixa(p, 'A1')[0]).toMatchObject({ inicio: 0, entrada: 15, saida: 25 })
    // Não passa do fim da fonte.
    p = editar(p, { tipo: 'deslizar', clipId: c.id, delta: 100 })
    expect(clipsDaFaixa(p, 'V1')[0]).toMatchObject({ entrada: 50, saida: 60 })
  })

  it('mover leva o áudio ligado; separar o áudio corta a ligação', () => {
    let p = comClip(10)
    const v = clipsDaFaixa(p, 'V1')[0]
    p = editar(p, { tipo: 'mover', clipId: v.id, inicio: 4 })
    expect(clipsDaFaixa(p, 'A1')[0].inicio).toBe(4)
    p = editar(p, { tipo: 'separar-audio', clipId: v.id })
    p = editar(p, { tipo: 'mover', clipId: v.id, inicio: 0 })
    expect(clipsDaFaixa(p, 'V1')[0].inicio).toBe(0)
    expect(clipsDaFaixa(p, 'A1')[0].inicio).toBe(4)
  })

  it('remover com ripple fecha o buraco só nas faixas afectadas', () => {
    let p = comClip(10)
    p = editar(p, { tipo: 'fonte', fonte: fonteAv('f2', 5) })
    p = editar(p, { tipo: 'inserir', fonteId: 'f2', faixa: 'V1' })
    const [c1] = clipsDaFaixa(p, 'V1')
    p = editar(p, { tipo: 'remover', clipIds: [c1.id], ripple: true })
    expect(clipsDaFaixa(p, 'V1').map((c) => [c.inicio, c.fonteId])).toEqual([[0, 'f2']])
    expect(clipsDaFaixa(p, 'A1').map((c) => [c.inicio, c.fonteId])).toEqual([[0, 'f2']])
  })
})

describe('ganho num intervalo', () => {
  it('parte o áudio nas fronteiras e sobe só o trecho', () => {
    let p = comClip(30)
    p = editar(p, { tipo: 'ganho-intervalo', faixa: 'A1', inicio: 10, fim: 14, ganhoDb: 6 })
    expect(clipsDaFaixa(p, 'A1').map((c) => [c.inicio, fimDoClip(c), c.ganhoDb])).toEqual([
      [0, 10, 0],
      [10, 14, 6],
      [14, 30, 0],
    ])
    // A imagem não muda o que mostra (parte-se, mas continua contínua).
    expect(clipsDaFaixa(p, 'V1').every((c) => c.cor.exposicao === 0)).toBe(true)
  })
})

describe('velocidade e congelar', () => {
  it('mudar a velocidade encolhe o clipe e puxa os seguintes', () => {
    let p = comClip(10)
    p = editar(p, { tipo: 'fonte', fonte: fonteAv('f2', 5) })
    p = editar(p, { tipo: 'inserir', fonteId: 'f2', faixa: 'V1' })
    const [c1] = clipsDaFaixa(p, 'V1')
    p = editar(p, { tipo: 'velocidade', clipId: c1.id, velocidade: 2 })
    expect(clipsDaFaixa(p, 'V1').map((c) => [c.inicio, duracaoDoClip(c)])).toEqual([
      [0, 5],
      [5, 5],
    ])
    expect(tempoNaFonte(clipsDaFaixa(p, 'V1')[0], 2.5)).toBe(5)
  })

  it('a velocidade fica entre 0,25× e 4×', () => {
    const p = comClip(10)
    const c = clipsDaFaixa(p, 'V1')[0]
    expect(clipsDaFaixa(editar(p, { tipo: 'velocidade', clipId: c.id, velocidade: 50 }), 'V1')[0].velocidade).toBe(4)
  })

  it('congelar insere o frame parado e empurra imagem, som e marcadores', () => {
    let p = comClip(10)
    p = editar(p, { tipo: 'marcador', marcador: { id: 'm', t: 6, rotulo: 'x', tipo: 'marcador' } })
    p = editar(p, { tipo: 'congelar', t: 4, duracao: 2 })
    const v = clipsDaFaixa(p, 'V1')
    expect(v.map((c) => [c.inicio, fimDoClip(c), c.congelado])).toEqual([
      [0, 4, null],
      [4, 6, 2],
      [6, 12, null],
    ])
    expect(v[1].entrada).toBe(4)
    expect(clipsDaFaixa(p, 'A1').map((c) => [c.inicio, fimDoClip(c)])).toEqual([
      [0, 4],
      [6, 12],
    ])
    // O som fica em silêncio durante o frame parado.
    expect(clipEm(p, 'A1', 5)).toBeNull()
    expect(p.marcadores[0].t).toBe(8)
  })
})

describe('cortar intervalos (pausas e corte pelo texto)', () => {
  it('parte os clipes, fecha os buracos e mantém vídeo e áudio alinhados', () => {
    let p = comClip(30)
    p = editar(p, { tipo: 'cortar-intervalos', intervalos: [{ inicio: 5, fim: 8 }, { inicio: 20, fim: 22 }] })
    const v = clipsDaFaixa(p, 'V1').map((c) => [c.inicio, c.entrada, c.saida])
    expect(v).toEqual([
      [0, 0, 5],
      [5, 8, 20],
      [17, 22, 30],
    ])
    expect(clipsDaFaixa(p, 'A1').map((c) => [c.inicio, c.entrada, c.saida])).toEqual(v)
    const pares = clipsDaFaixa(p, 'V1').map((c, i) => c.grupo === clipsDaFaixa(p, 'A1')[i].grupo)
    expect(pares).toEqual([true, true, true])
    expect(duracaoDoProjecto(p)).toBe(25)
  })

  it('leva as legendas: palavras cortadas saem e as outras mudam de tempo', () => {
    let p = comClip(30)
    p = editar(p, {
      tipo: 'legendas',
      legendas: {
        lingua: 'pt',
        estimadas: false,
        traducoes: { en: [{ id: 't', inicio: 9, fim: 12, texto: 'Hello' }] },
        cues: [
          {
            id: 'c',
            inicio: 1,
            fim: 4,
            texto: 'Fica, pá, registado',
            palavras: [
              { inicio: 1, fim: 1.5, texto: 'Fica,' },
              { inicio: 1.6, fim: 2, texto: 'pá,' },
              { inicio: 2.2, fim: 4, texto: 'registado' },
            ],
          },
          { id: 'd', inicio: 9, fim: 12, texto: 'Depois' },
        ],
      },
    })
    p = editar(p, { tipo: 'cortar-intervalos', intervalos: [{ inicio: 1.6, fim: 2.2 }] })
    const [c, d] = p.legendas!.cues
    expect(c.texto).toBe('Fica, registado')
    expect(c.palavras!.map((w) => [w.inicio, w.fim])).toEqual([
      [1, 1.5],
      [1.6, 3.4],
    ])
    expect(d.inicio).toBeCloseTo(8.4)
    expect(p.legendas!.traducoes.en[0].inicio).toBeCloseTo(8.4)
  })

  it('não mexe em faixas bloqueadas', () => {
    let p = comClip(30)
    p = editar(p, { tipo: 'faixa', id: 'A1', patch: { bloqueada: true } })
    p = editar(p, { tipo: 'cortar-intervalos', intervalos: [{ inicio: 5, fim: 8 }] })
    expect(clipsDaFaixa(p, 'A1')).toHaveLength(1)
    expect(clipsDaFaixa(p, 'V1')).toHaveLength(2)
  })

  it('cues sem palavras encolhem nas pontas', () => {
    const out = cortarCues([{ id: 'a', inicio: 0, fim: 10, texto: 'x' }], [{ inicio: 8, fim: 12 }])
    expect(out[0]).toMatchObject({ inicio: 0, fim: 8 })
  })

  it('pausas em tempo da fonte chegam à linha de tempo pelos clipes', () => {
    let p = comClip(30)
    p = editar(p, { tipo: 'cortar-intervalos', intervalos: [{ inicio: 0, fim: 10 }] })
    // A fonte começa agora aos 10 s: uma pausa de fonte 12–14 está em 2–4.
    expect(intervalosDaFonteNaLinha(p, 'f1', 'A1', [{ inicio: 5, fim: 7 }, { inicio: 12, fim: 14 }])).toEqual([{ inicio: 2, fim: 4 }])
  })
})

describe('projecto inicial a partir de uma gravação', () => {
  it('com faixas isoladas, vídeo em V1 e áudio em A1 LIGADOS', () => {
    let p = novoProjecto('x')
    p = editar(p, { tipo: 'fonte', fonte: { ...fonteAv('c'), origem: 'completo' } })
    p = editar(p, { tipo: 'fonte', fonte: { ...fonteAv('v'), tipo: 'video', origem: 'video' } })
    p = editar(p, { tipo: 'fonte', fonte: { ...fonteAv('a'), tipo: 'audio', origem: 'audio' } })
    p = montarInicial(p)
    expect(clipsDaFaixa(p, 'V1')[0].fonteId).toBe('v')
    expect(clipsDaFaixa(p, 'A1')[0].fonteId).toBe('a')
    expect(clipsDaFaixa(p, 'V1')[0].grupo).toBe(clipsDaFaixa(p, 'A1')[0].grupo)
  })
})

describe('histórico', () => {
  it('desfaz e refaz, e uma nova edição apaga o futuro', () => {
    let h = H.iniciar(comClip(30))
    const original = h.presente
    h = H.aplicar(h, { tipo: 'dividir', t: 10 })
    h = H.aplicar(h, { tipo: 'titulo', titulo: 'Novo' })
    expect(H.podeDesfazer(h)).toBe(true)
    h = H.desfazer(h)
    expect(h.presente.titulo).toBe('Aula')
    h = H.desfazer(h)
    expect(h.presente).toBe(original)
    expect(H.podeDesfazer(h)).toBe(false)
    h = H.refazer(h)
    expect(clipsDaFaixa(h.presente, 'V1')).toHaveLength(2)
    h = H.aplicar(h, { tipo: 'titulo', titulo: 'Outro' })
    expect(H.podeRefazer(h)).toBe(false)
  })

  it('edições sem efeito não criam passo', () => {
    const h = H.iniciar(comClip(30))
    expect(H.aplicar(h, { tipo: 'aparar', clipId: 'nao-existe', saida: 3, ripple: true })).toBe(h)
  })

  it('a mesma chave junta um arrasto num só passo', () => {
    let h = H.iniciar(comClip(30))
    const id = clipsDaFaixa(h.presente, 'V1')[0].id
    for (let i = 1; i <= 10; i++) h = H.aplicar(h, { tipo: 'cor', clipId: id, cor: { contraste: i } }, 'cor:contraste')
    expect(h.passado).toHaveLength(1)
    h = H.desfazer(h)
    expect(clipsDaFaixa(h.presente, 'V1')[0].cor.contraste).toBe(0)
  })

  it('não guarda mais do que o limite', () => {
    let h = H.iniciar(comClip(30))
    for (let i = 0; i < H.LIMITE + 20; i++) h = H.aplicar(h, { tipo: 'titulo', titulo: `t${i}` })
    expect(h.passado).toHaveLength(H.LIMITE)
  })

  it('várias edições como um passo', () => {
    let h = H.iniciar(comClip(30))
    h = H.aplicarVarias(h, [
      { tipo: 'dividir', t: 5 },
      { tipo: 'dividir', t: 8 },
    ])
    expect(clipsDaFaixa(h.presente, 'V1')).toHaveLength(3)
    expect(h.passado).toHaveLength(1)
  })
})
