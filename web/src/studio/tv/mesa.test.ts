import { describe, expect, it } from 'vitest'
import { accaoDaTecla, type TeclaDaMesa } from './atalhos'
import { correrMacro, MACROS_INICIAIS, type Passo } from './macros'
import {
  auto,
  avancar,
  bandaDoStinger,
  cortar,
  directoAoAr,
  limparFontes,
  MESA_INICIAL,
  moverTbar,
  planoDe,
  porEmPrevia,
  progresso,
  quadroDaMesa,
  tallyDe,
  type EstadoDaMesa,
} from './mesa'
import { CORTE_DE_VOZ_ZERO, decidirCorteDeVoz } from './vozCorte'

const comDuas = (): EstadoDaMesa => ({ ...MESA_INICIAL, programa: planoDe('cam1'), previa: planoDe('cam2') })

describe('mesa de corte', () => {
  it('cortar troca programa e pré e marca a hora de entrada no ar', () => {
    const e = cortar(comDuas(), 1000)
    expect(e.programa?.fontes).toEqual(['cam2'])
    expect(e.previa?.fontes).toEqual(['cam1'])
    expect(e.noArDesde).toBe(1000)
  })

  it('sem pré, cortar não faz nada', () => {
    const e = { ...MESA_INICIAL, programa: planoDe('cam1') }
    expect(cortar(e, 1)).toBe(e)
  })

  it('o tally conta a fonte que está a ENTRAR numa transição como no ar', () => {
    const e = auto(comDuas(), 0, 'misturar', 1000)
    expect(tallyDe(e, 'cam2')).toBe('programa')
    expect(tallyDe(e, 'cam1')).toBe('programa')
    expect(tallyDe(e, 'cam3')).toBe('livre')
  })

  it('AUTO a misturar dura o que foi pedido e só troca no fim', () => {
    let e = auto(comDuas(), 0, 'misturar', 600)
    expect(progresso(e, 300)).toBeCloseTo(0.5)
    expect(quadroDaMesa(e, 300).transicao?.p).toBeCloseTo(0.5)
    e = avancar(e, 599)
    expect(e.emCurso).not.toBeNull()
    e = avancar(e, 600)
    expect(e.emCurso).toBeNull()
    expect(e.programa?.fontes).toEqual(['cam2'])
  })

  it('AUTO com CORTAR escolhido é um corte seco', () => {
    const e = auto(comDuas(), 0)
    expect(e.emCurso).toBeNull()
    expect(e.programa?.fontes).toEqual(['cam2'])
  })

  it('cortar a meio de uma transição acaba-a no destino', () => {
    const e = cortar(auto(comDuas(), 0, 'limpar', 1000), 200)
    expect(e.emCurso).toBeNull()
    expect(e.programa?.fontes).toEqual(['cam2'])
  })

  it('a T-bar desce, conclui em baixo e a próxima transição faz-se a subir', () => {
    let e: EstadoDaMesa = { ...comDuas(), transicao: 'misturar' }
    e = moverTbar(e, 0.38, 0)
    expect(progresso(e, 0)).toBeCloseTo(0.38)
    expect(e.programa?.fontes).toEqual(['cam1'])
    e = moverTbar(e, 1, 10)
    expect(e.emCurso).toBeNull()
    expect(e.programa?.fontes).toEqual(['cam2'])
    expect(e.tbarEmBaixo).toBe(true)
    e = moverTbar(e, 0.75, 20)
    expect(progresso(e, 20)).toBeCloseTo(0.25)
    e = moverTbar(e, 0, 30)
    expect(e.programa?.fontes).toEqual(['cam1'])
  })

  it('voltar a T-bar ao ponto de partida anula a transição', () => {
    let e: EstadoDaMesa = { ...comDuas(), transicao: 'misturar' }
    e = moverTbar(e, 0.5, 0)
    e = moverTbar(e, 0, 1)
    expect(e.emCurso).toBeNull()
    expect(e.programa?.fontes).toEqual(['cam1'])
  })

  it('⇧n manda directo ao ar e deixa o anterior em pré', () => {
    const e = directoAoAr(comDuas(), planoDe('cam3'), 5)
    expect(e.programa?.fontes).toEqual(['cam3'])
    expect(e.previa?.fontes).toEqual(['cam1'])
  })

  it('durante uma transição a pré espera', () => {
    const e = auto(comDuas(), 0, 'misturar', 1000)
    expect(porEmPrevia(e, planoDe('cam3'))).toBe(e)
  })

  it('uma fonte que desaparece sai do programa e da pré', () => {
    const e = limparFontes({ ...comDuas(), previa: { fontes: ['cam2', 'cam3'], layout: 'lado-a-lado' } }, new Set(['cam1', 'cam3']))
    expect(e.programa?.fontes).toEqual(['cam1'])
    expect(e.previa).toEqual({ fontes: ['cam3'], layout: 'solo' })
    const sem = limparFontes(comDuas(), new Set())
    expect(sem.programa).toBeNull()
  })

  it('o stinger tapa o centro no meio e a banda sai do quadro nos extremos', () => {
    const W = 1920
    const meio = bandaDoStinger(0.5, W)
    expect(meio.x).toBeLessThan(W / 2)
    expect(meio.x + meio.w).toBeGreaterThan(W / 2)
    expect(bandaDoStinger(0, W).x + bandaDoStinger(0, W).w).toBeLessThanOrEqual(0)
    expect(bandaDoStinger(1, W).x).toBeGreaterThanOrEqual(W)
  })
})

const tecla = (p: Partial<TeclaDaMesa>): TeclaDaMesa => ({ key: '', code: '', shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, ...p })

describe('atalhos da mesa', () => {
  it('1–6 põem em pré e ⇧1–⇧6 mandam ao ar, lidos do código físico', () => {
    expect(accaoDaTecla(tecla({ key: '3', code: 'Digit3' }))).toEqual({ tipo: 'previa', n: 3 })
    expect(accaoDaTecla(tecla({ key: '!', code: 'Digit1', shiftKey: true }))).toEqual({ tipo: 'ar', n: 1 })
    expect(accaoDaTecla(tecla({ key: '7', code: 'Digit7' }))).toBeNull()
  })
  it('espaço corta, enter mistura, W limpa, S stinger', () => {
    expect(accaoDaTecla(tecla({ key: ' ', code: 'Space' }))).toEqual({ tipo: 'cortar' })
    expect(accaoDaTecla(tecla({ key: 'Enter', code: 'Enter' }))).toEqual({ tipo: 'misturar' })
    expect(accaoDaTecla(tecla({ key: 'w', code: 'KeyW' }))).toEqual({ tipo: 'limpar' })
    expect(accaoDaTecla(tecla({ key: 's', code: 'KeyS' }))).toEqual({ tipo: 'stinger' })
  })
  it('⌘/Ctrl/Alt 1–4 são sobreposições; F1–F6 macros; repetição ignorada', () => {
    expect(accaoDaTecla(tecla({ key: '2', code: 'Digit2', metaKey: true }))).toEqual({ tipo: 'sobreposicao', n: 2 })
    expect(accaoDaTecla(tecla({ key: '¡', code: 'Digit4', altKey: true }))).toEqual({ tipo: 'sobreposicao', n: 4 })
    expect(accaoDaTecla(tecla({ key: '5', code: 'Digit5', ctrlKey: true }))).toBeNull()
    expect(accaoDaTecla(tecla({ key: 'F6', code: 'F6' }))).toEqual({ tipo: 'macro', n: 6 })
    expect(accaoDaTecla(tecla({ key: ' ', code: 'Space', repeat: true }))).toBeNull()
  })
})

describe('macros', () => {
  it('correm os passos por ordem e seguem depois de um indisponível', async () => {
    const vistos: Passo['tipo'][] = []
    const entrevista = MACROS_INICIAIS.find((m) => m.id === 'entrevista')!
    const r = await correrMacro(
      entrevista,
      (p) => {
        vistos.push(p.tipo)
        return p.tipo === 'luz' ? { estado: 'indisponivel', razao: 'semAgente' } : { estado: 'feito' }
      },
      new AbortController().signal,
    )
    expect(vistos).toEqual(['conteudo', 'previa', 'luz', 'transicao'])
    expect(r.map((x) => x.estado)).toEqual(['feito', 'feito', 'indisponivel', 'feito'])
  })

  it('abortar pára a macro a meio de uma espera', async () => {
    const c = new AbortController()
    const encerrar = MACROS_INICIAIS.find((m) => m.id === 'encerrar')!
    const feitos: string[] = []
    const p = correrMacro(encerrar, (x) => (feitos.push(x.tipo), { estado: 'feito' }), c.signal)
    await new Promise((r) => setTimeout(r, 5))
    c.abort()
    await expect(p).rejects.toThrow()
    expect(feitos).not.toContain('terminarEmissao')
  })

  it('as teclas das macros do template são F1, F2, F3, F4 e F6', () => {
    expect(MACROS_INICIAIS.map((m) => m.tecla)).toEqual([1, 2, 3, 4, 6])
  })
})

describe('corte automático por voz', () => {
  const niveis = (o: Record<string, number>) => new Map(Object.entries(o))

  it('corta para quem fala só depois de segurar a voz', () => {
    let e = CORTE_DE_VOZ_ZERO
    let d = decidirCorteDeVoz(e, niveis({ a: -20, b: -60 }), 'b', 0)
    expect(d.cortarPara).toBeNull()
    e = d.estado
    d = decidirCorteDeVoz(e, niveis({ a: -20, b: -60 }), 'b', 500)
    expect(d.cortarPara).toBeNull()
    d = decidirCorteDeVoz(d.estado, niveis({ a: -20, b: -60 }), 'b', 950)
    expect(d.cortarPara).toBe('a')
  })

  it('duas pessoas ao mesmo nível não fazem cortar', () => {
    let d = decidirCorteDeVoz(CORTE_DE_VOZ_ZERO, niveis({ a: -20, b: -22 }), 'b', 0)
    d = decidirCorteDeVoz(d.estado, niveis({ a: -20, b: -22 }), 'b', 2000)
    expect(d.cortarPara).toBeNull()
  })

  it('abaixo do limiar é silêncio, e entre dois cortes há um intervalo mínimo', () => {
    expect(decidirCorteDeVoz(CORTE_DE_VOZ_ZERO, niveis({ a: -55 }), null, 5000).cortarPara).toBeNull()
    let d = decidirCorteDeVoz({ candidato: 'b', desde: 0, ultimoCorte: 0 }, niveis({ b: -10, a: -60 }), 'a', 1000)
    expect(d.cortarPara).toBeNull() // ainda dentro dos 3 s
    d = decidirCorteDeVoz(d.estado, niveis({ b: -10, a: -60 }), 'a', 3100)
    expect(d.cortarPara).toBe('b')
  })
})
