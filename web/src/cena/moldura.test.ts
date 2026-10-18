/**
 * A geometria da moldura de transmissão, testada sem canvas nenhum (não há
 * jsdom neste repo — ver `room/compositor.test.ts`).
 *
 * O que se afirma aqui são PROPRIEDADES, não números decorados: as peças não
 * se sobrepõem, cobrem o espaço que lhes toca, não saem do canvas, e a cena em
 * 2160p é a de 1080p vezes dois. Um número solto («o herói mede 1234 px»)
 * passaria a ser falso na primeira vez que alguém afinasse uma fracção, e teria
 * de ser reescrito sem que nada estivesse partido — um teste assim treina as
 * pessoas a actualizá-lo em vez de o ler.
 */
import { describe, expect, it } from 'vitest'
import {
  deslocamentoDaFaixa,
  FRACCAO_DA_MARGEM,
  MAXIMO_DE_CAMARAS,
  rectsDaMoldura,
  VELOCIDADE_DA_FAIXA,
  type Rect,
} from './moldura'

/** Duas áreas sobrepõem-se? (com folga, para não acusar encostos por arredondamento) */
function sobrepoe(a: Rect, b: Rect): boolean {
  const e = 1e-9
  return a.x < b.x + b.w - e && b.x < a.x + a.w - e && a.y < b.y + b.h - e && b.y < a.y + a.h - e
}

function todasAsPecas(r: ReturnType<typeof rectsDaMoldura>): Rect[] {
  return [r.faixa, r.heroi, r.logo, ...r.camaras, r.assinatura].filter((x): x is Rect => !!x)
}

describe('rectsDaMoldura — as peças repartem o espaço sem se pisarem', () => {
  it('nenhum par de peças se sobrepõe, para qualquer número de câmaras', () => {
    for (const n of [0, 1, 2, 3, 4, 9]) {
      const pecas = todasAsPecas(rectsDaMoldura(n, 1920, 1080))
      for (let i = 0; i < pecas.length; i++) {
        for (let j = i + 1; j < pecas.length; j++) {
          expect(sobrepoe(pecas[i], pecas[j]), `n=${n}: peça ${i} pisa a ${j}`).toBe(false)
        }
      }
    }
  })

  it('nenhuma peça sai do canvas', () => {
    for (const [W, H] of [
      [1920, 1080],
      [3840, 2160],
      [2560, 1080], // 21:9 — a margem é da ALTURA, e isto apanha quem a mudar para a largura
    ]) {
      for (const r of todasAsPecas(rectsDaMoldura(3, W, H))) {
        expect(r.x).toBeGreaterThanOrEqual(-1e-9)
        expect(r.y).toBeGreaterThanOrEqual(-1e-9)
        expect(r.x + r.w).toBeLessThanOrEqual(W + 1e-9)
        expect(r.y + r.h).toBeLessThanOrEqual(H + 1e-9)
        expect(r.w).toBeGreaterThan(0)
        expect(r.h).toBeGreaterThan(0)
      }
    }
  })

  it('a MOLDURA existe: nenhuma peça de conteúdo encosta à borda — só a faixa vai de ponta a ponta', () => {
    // Esta asserção nasceu de um controlo negativo que FALHOU em acusar: ao
    // tirar o vão entre a coluna e o herói, o herói passava a comer a margem
    // direita e encostava ao bordo do canvas. Não havia sobreposição nem
    // transbordo, e por isso nenhum dos outros testes dava por isso — mas uma
    // moldura sem moldura deixou de ser o que se pediu.
    for (const [W, H] of [
      [1920, 1080],
      [3840, 2160],
      [2560, 1080],
    ]) {
      for (const lado of ['esquerda', 'direita'] as const) {
        const r = rectsDaMoldura(3, W, H, { lado })
        const margem = H * FRACCAO_DA_MARGEM
        // O conteúdo (tudo menos a faixa) vive dentro da caixa da margem.
        const conteudo = [r.heroi, r.logo!, ...r.camaras, r.assinatura!]
        for (const p of conteudo) {
          expect(p.x, `${lado} ${W}×${H}: encosta à esquerda`).toBeGreaterThanOrEqual(margem - 1e-9)
          expect(p.x + p.w, `${lado} ${W}×${H}: encosta à direita`).toBeLessThanOrEqual(W - margem + 1e-9)
          expect(p.y + p.h, `${lado} ${W}×${H}: encosta em baixo`).toBeLessThanOrEqual(H - margem + 1e-9)
        }
        // E há mesmo um vão ENTRE a coluna e o herói, não só nas bordas.
        const [esquerdo, direito] = r.heroi.x < r.logo!.x ? [r.heroi, r.logo!] : [r.logo!, r.heroi]
        expect(direito.x - (esquerdo.x + esquerdo.w), `${lado}: sem vão entre coluna e herói`).toBeCloseTo(
          margem,
          6,
        )

        // E a moldura é VISÍVEL. O limite é um LITERAL, de propósito: as
        // asserções acima calculam a margem esperada a partir da MESMA
        // constante que o código lê, por isso pôr `FRACCAO_DA_MARGEM` a zero
        // punha as duas pontas de acordo e passava — provavam consistência, não
        // que houvesse moldura. Medido: um controlo negativo com a margem a
        // zero dava exit 0 antes desta linha existir.
        expect(margem, 'a margem desapareceu — não há moldura nenhuma').toBeGreaterThan(H * 0.01)
      }
    }
  })

  it('a faixa é uma barra de ponta a ponta no topo — não um cartão com margem', () => {
    const { faixa } = rectsDaMoldura(2, 1920, 1080)
    expect(faixa).not.toBeNull()
    expect(faixa!.x).toBe(0)
    expect(faixa!.y).toBe(0)
    expect(faixa!.w).toBe(1920)
  })

  it('a coluna empilha logo, câmaras e assinatura e COBRE a altura do conteúdo, sem deixar tira por pintar', () => {
    const r = rectsDaMoldura(3, 1920, 1080)
    const naColuna = [r.logo!, ...r.camaras, r.assinatura!]
    // Todas na mesma coluna (mesmo x e mesma largura).
    for (const p of naColuna) {
      expect(p.x).toBeCloseTo(r.logo!.x, 9)
      expect(p.w).toBeCloseTo(r.logo!.w, 9)
    }
    // Contíguas de cima a baixo: o fim de cada uma é o início da seguinte.
    for (let i = 1; i < naColuna.length; i++) {
      expect(naColuna[i].y).toBeCloseTo(naColuna[i - 1].y + naColuna[i - 1].h, 9)
    }
    // E juntas ocupam exactamente a altura do herói (= a altura do conteúdo).
    const somada = naColuna.reduce((s, p) => s + p.h, 0)
    expect(somada).toBeCloseTo(r.heroi.h, 9)
    expect(naColuna[0].y).toBeCloseTo(r.heroi.y, 9)
  })

  it('o herói é bem maior que a coluna — senão não é um herói', () => {
    const r = rectsDaMoldura(2, 1920, 1080)
    expect(r.heroi.w).toBeGreaterThan(r.logo!.w * 2)
  })

  it('2160p é 1080p vezes dois, peça a peça — as fracções não escondem píxeis fixos', () => {
    const pequeno = todasAsPecas(rectsDaMoldura(3, 1920, 1080))
    const grande = todasAsPecas(rectsDaMoldura(3, 3840, 2160))
    expect(grande).toHaveLength(pequeno.length)
    pequeno.forEach((p, i) => {
      expect(grande[i].x).toBeCloseTo(p.x * 2, 6)
      expect(grande[i].y).toBeCloseTo(p.y * 2, 6)
      expect(grande[i].w).toBeCloseTo(p.w * 2, 6)
      expect(grande[i].h).toBeCloseTo(p.h * 2, 6)
    })
  })

  it('lado direito: espelha a coluna e o herói, e nada sai do canvas', () => {
    const esq = rectsDaMoldura(2, 1920, 1080, { lado: 'esquerda' })
    const dir = rectsDaMoldura(2, 1920, 1080, { lado: 'direita' })
    expect(dir.logo!.x).toBeGreaterThan(dir.heroi.x) // coluna à direita do herói
    expect(esq.logo!.x).toBeLessThan(esq.heroi.x) // e ao contrário à esquerda
    // As LARGURAS não mudam com o lado — só as posições.
    expect(dir.heroi.w).toBeCloseTo(esq.heroi.w, 9)
    expect(dir.logo!.w).toBeCloseTo(esq.logo!.w, 9)
    expect(dir.logo!.x + dir.logo!.w).toBeLessThanOrEqual(1920 + 1e-9)
  })

  it('sem câmaras: a coluna fica só com a marca e o HERÓI NÃO MUDA — a cena não salta quando alguém sai', () => {
    const com = rectsDaMoldura(2, 1920, 1080)
    const sem = rectsDaMoldura(0, 1920, 1080)
    expect(sem.camaras).toEqual([])
    expect(sem.logo).not.toBeNull()
    expect(sem.heroi).toEqual(com.heroi)
  })

  it('mais câmaras do que a coluna mostra: corta no tecto em vez de as espremer a zero', () => {
    const r = rectsDaMoldura(99, 1920, 1080)
    expect(r.camaras).toHaveLength(MAXIMO_DE_CAMARAS)
    for (const c of r.camaras) expect(c.h).toBeGreaterThan(0)
  })

  it('desligar a faixa devolve o espaço ao conteúdo, não deixa uma tira preta', () => {
    const com = rectsDaMoldura(2, 1920, 1080, { faixa: true })
    const sem = rectsDaMoldura(2, 1920, 1080, { faixa: false })
    expect(sem.faixa).toBeNull()
    expect(sem.heroi.y).toBeLessThan(com.heroi.y)
    expect(sem.heroi.h).toBeGreaterThan(com.heroi.h)
  })

  it('desligar logo e assinatura dá a coluna inteira às câmaras', () => {
    const r = rectsDaMoldura(2, 1920, 1080, { logo: false, assinatura: false })
    expect(r.logo).toBeNull()
    expect(r.assinatura).toBeNull()
    const somada = r.camaras.reduce((s, c) => s + c.h, 0)
    expect(somada).toBeCloseTo(r.heroi.h, 9)
  })
})

describe('deslocamentoDaFaixa — determinístico, porque o instante é ARGUMENTO', () => {
  it('o mesmo instante dá sempre o mesmo deslocamento', () => {
    // Se a função lesse o relógio por dentro, esta asserção era impossível de
    // escrever — e é precisamente por isso que ela existe.
    const a = deslocamentoDaFaixa(1_234_567, 300, 1920)
    const b = deslocamentoDaFaixa(1_234_567, 300, 1920)
    expect(a).toBe(b)
  })

  it('o resultado nunca sai de [0, passo) — nem ao fim de dez horas de directo', () => {
    for (const ms of [0, 1, 999, 60_000, 36_000_000]) {
      const d = deslocamentoDaFaixa(ms, 300, 1920)
      expect(d).toBeGreaterThanOrEqual(0)
      expect(d).toBeLessThan(300)
    }
  })

  it('avança à velocidade anunciada', () => {
    // Num segundo, a faixa anda VELOCIDADE_DA_FAIXA da largura. Com um passo
    // maior que isso, o deslocamento ao fim de 1 s é esse valor exacto.
    const largura = 1920
    const esperado = largura * VELOCIDADE_DA_FAIXA
    expect(deslocamentoDaFaixa(1000, largura, largura)).toBeCloseTo(esperado, 6)
  })

  it('instante negativo não atira a faixa para fora do ecrã (o % do JS devolve negativo)', () => {
    const d = deslocamentoDaFaixa(-1000, 300, 1920)
    expect(d).toBeGreaterThanOrEqual(0)
    expect(d).toBeLessThan(300)
  })

  it('passo zero ou entrada não-finita: devolve 0 em vez de NaN — um NaN no canvas apaga o desenho todo', () => {
    expect(deslocamentoDaFaixa(1000, 0, 1920)).toBe(0)
    expect(deslocamentoDaFaixa(Number.NaN, 300, 1920)).toBe(0)
    expect(deslocamentoDaFaixa(1000, 300, Number.POSITIVE_INFINITY)).toBe(0)
  })
})
