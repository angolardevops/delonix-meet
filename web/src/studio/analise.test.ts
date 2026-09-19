/**
 * A detecção de pausas é testável de verdade: constrói-se um áudio com
 * silêncios NOS SÍTIOS QUE SE SABEM e verifica-se que os encontra lá.
 *
 * Não há jsdom neste repo, por isso o `AudioContext` e o `decodeAudioData` são
 * esboçados — o que se está a testar é a matemática do detector, não a
 * capacidade do browser de descodificar WebM.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'

const SR = 16_000

/** Áudio com fala (ruído) e silêncios nos intervalos pedidos. */
function fabricar(duracao: number, silencios: [number, number][]): Float32Array {
  const d = new Float32Array(Math.round(duracao * SR))
  for (let i = 0; i < d.length; i++) {
    const t = i / SR
    const calado = silencios.some(([a, b]) => t >= a && t < b)
    // Fala = ruído a ~0,2 RMS. Silêncio = chão de ruído a ~0,001 — como uma
    // sala real, não um zero perfeito que qualquer limiar apanharia.
    d[i] = calado ? (Math.random() - 0.5) * 0.002 : (Math.random() - 0.5) * 0.6
  }
  return d
}

function esboçarAudioContext(dados: Float32Array) {
  class FakeAudioBuffer {
    length: number
    numberOfChannels: number
    sampleRate: number
    duration: number
    private canais: Float32Array[]
    constructor(o: { length: number; numberOfChannels: number; sampleRate: number }) {
      this.length = o.length
      this.numberOfChannels = o.numberOfChannels
      this.sampleRate = o.sampleRate
      this.duration = o.length / o.sampleRate
      this.canais = Array.from({ length: o.numberOfChannels }, () => new Float32Array(o.length))
    }
    getChannelData(c: number) { return this.canais[c] }
    copyToChannel(src: Float32Array, c: number, offset = 0) { this.canais[c].set(src, offset) }
  }
  const buffer = new FakeAudioBuffer({ length: dados.length, numberOfChannels: 1, sampleRate: SR })
  buffer.copyToChannel(dados, 0)
  vi.stubGlobal('AudioBuffer', FakeAudioBuffer)
  vi.stubGlobal('AudioContext', class {
    async decodeAudioData() { return buffer }
    async close() {}
  })
}

const blobFalso = { arrayBuffer: async () => new ArrayBuffer(8) } as unknown as Blob

describe('analisarPausas', () => {
  beforeEach(() => vi.unstubAllGlobals())

  it('encontra as pausas onde elas estão', async () => {
    esboçarAudioContext(fabricar(10, [[2, 4], [7, 8.5]]))
    const { analisarPausas } = await import('./analise')
    const a = await analisarPausas(blobFalso)
    expect(a.pausas.length).toBe(2)
    // Medido: com margem de 0,12 s o silêncio [2, 4] devolve [2.12, 3.88].
    // Os limites são apertados de propósito — `>= 2` passaria com margem ZERO
    // e o teste não guardaria nada (foi o que a primeira versão fazia).
    expect(a.pausas[0].inicio).toBeGreaterThan(2.05)
    expect(a.pausas[0].fim).toBeLessThan(3.95)
    expect(a.pausas[1].inicio).toBeGreaterThan(7.05)
    expect(a.pausas[1].fim).toBeLessThan(8.45)
  })

  it('ignora respirações — pausas abaixo do mínimo não contam', async () => {
    esboçarAudioContext(fabricar(6, [[2, 2.3]]))
    const { analisarPausas } = await import('./analise')
    expect((await analisarPausas(blobFalso)).pausas).toEqual([])
  })

  it('uma aula corrida não dá pausas nenhumas', async () => {
    esboçarAudioContext(fabricar(5, []))
    const { analisarPausas } = await import('./analise')
    expect((await analisarPausas(blobFalso)).pausas).toEqual([])
  })

  it('o limiar é RELATIVO: uma gravação baixinha continua a ser detectada', async () => {
    // Mesma forma, tudo dez vezes mais baixo. Um limiar fixo em dB falharia.
    const baixo = fabricar(10, [[3, 5]]).map((v) => v * 0.1) as Float32Array
    esboçarAudioContext(baixo)
    const { analisarPausas } = await import('./analise')
    const a = await analisarPausas(blobFalso)
    expect(a.pausas.length).toBe(1)
    // A parte que importa: a pausa tem de estar ONDE O SILÊNCIO ESTÁ. Um
    // limiar fixo (ex.: 0,02) fica acima do nível da fala desta gravação
    // (medido: RMS 0,017) e devolve UMA pausa de dez segundos — que passaria
    // no `length === 1` acima sem detectar coisa nenhuma.
    expect(a.pausas[0].inicio).toBeGreaterThan(3)
    expect(a.pausas[0].fim).toBeLessThan(5)
    expect(a.pausas[0].fim - a.pausas[0].inicio).toBeLessThan(2.1)
  })
})

describe('trocosSemPausas', () => {
  it('devolve o que FICA, não o que sai', async () => {
    const { trocosSemPausas } = await import('./analise')
    const t = trocosSemPausas({
      duracao: 10, poupanca: 2, referencia: 0.2, limiar: 0.01,
      pausas: [{ inicio: 3, fim: 5 }],
    })
    expect(t).toEqual([{ inicio: 0, fim: 3 }, { inicio: 5, fim: 10 }])
  })

  it('descarta troços curtos demais para sobreviverem ao reencode', async () => {
    const { trocosSemPausas } = await import('./analise')
    // Uma pausa que começa a 0,05 s deixaria um troço de 50 ms antes dela —
    // menos de dois frames, que só acrescenta um salto na imagem.
    const t = trocosSemPausas({
      duracao: 10, poupanca: 2, referencia: 0.2, limiar: 0.01,
      pausas: [{ inicio: 0.05, fim: 2 }],
    })
    expect(t).toEqual([{ inicio: 2, fim: 10 }])
  })

  it('uma pausa no início não deixa um troço de comprimento zero', async () => {
    const { trocosSemPausas } = await import('./analise')
    const t = trocosSemPausas({
      duracao: 10, poupanca: 2, referencia: 0.2, limiar: 0.01,
      pausas: [{ inicio: 0, fim: 2 }],
    })
    expect(t).toEqual([{ inicio: 2, fim: 10 }])
  })

  it('a soma dos troços é a duração menos a poupança', async () => {
    const { trocosSemPausas } = await import('./analise')
    const a = {
      duracao: 20, poupanca: 5, referencia: 0.2, limiar: 0.01,
      pausas: [{ inicio: 2, fim: 4 }, { inicio: 10, fim: 13 }],
    }
    const soma = trocosSemPausas(a).reduce((x, t) => x + (t.fim - t.inicio), 0)
    expect(soma).toBeCloseTo(a.duracao - a.poupanca, 5)
  })
  // As duas fronteiras que o teste por MUTAÇÃO encontrou sem guarda
  // (`scripts/mutantes.mjs`): a bateria ficava verde com o código alterado.

  // MUTAÇÃO SOBREVIVENTE: `fim - inicio >= minimo` → `>`.
  // Uma pausa com EXACTAMENTE a duração mínima deixava de contar — e a
  // duração mínima é precisamente o número que o utilizador escreve no cursor.
  it('uma pausa com exactamente a duração mínima CONTA', async () => {
    // Silêncio de 1,00 s. Com `margem: 0` não há encolhimento, por isso o que
    // chega ao teste do mínimo é 1,00 s certo — a fronteira.
    esboçarAudioContext(fabricar(6, [[2, 3]]))
    const { analisarPausas } = await import('./analise')
    const a = await analisarPausas(blobFalso, { minimo: 1, margem: 0 })
    expect(a.pausas.length).toBe(1)
  })

  it('e uma pausa MAIS CURTA do que o mínimo continua a não contar', async () => {
    esboçarAudioContext(fabricar(6, [[2, 2.5]]))
    const { analisarPausas } = await import('./analise')
    const a = await analisarPausas(blobFalso, { minimo: 1, margem: 0 })
    expect(a.pausas).toEqual([])
  })

  // MUTAÇÃO SOBREVIVENTE: `for (let j = 0; j <= n; j++)` → `<`.
  // O último bloco de análise deixava de ser visitado, e com ele o silêncio que
  // vai até ao FIM da gravação — que é o caso mais comum de todos, porque toda
  // a gente deixa uns segundos de silêncio antes de parar de gravar.
  it('um silêncio que vai até ao fim da gravação é encontrado', async () => {
    esboçarAudioContext(fabricar(6, [[4, 6]]))
    const { analisarPausas } = await import('./analise')
    const a = await analisarPausas(blobFalso, { minimo: 1, margem: 0 })
    expect(a.pausas.length).toBe(1)
    expect(a.pausas[0].fim).toBeGreaterThan(5.5)
  })

})
