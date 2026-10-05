/**
 * Portão do catálogo de atalhos.
 *
 * Três coisas que só um teste apanha:
 *
 * 1. **A gramática fecha.** Cada combinação escrita no catálogo é
 *    reconhecida pelo próprio comparador — um `mod+shift+2` mal escrito
 *    (`cmd+2`, `ctrl-shift-2`) passaria a revisão e nunca dispararia.
 * 2. **Ninguém pisa ninguém.** Duas acções com a mesma tecla no mesmo escopo,
 *    ou um atalho global roubado por um de ecrã, é um defeito silencioso: a
 *    primeira entrada ganha e a outra deixa de existir.
 * 3. **A mesa de corte ignora as vistas do Estúdio.** As vistas mudam com
 *    ⌘⇧1–5 EXACTAMENTE porque a mesa não lê dígitos com ⇧ e modificador. Se
 *    alguém alargar `accaoDaTecla`, mudar de vista no ar passa a cortar uma
 *    fonte — e é aqui que isso fica vermelho, não no ar.
 *
 * E o de sempre: cada rótulo existe nas quatro línguas (o i18next escreve o
 * identificador no ecrã quando falta, sem avisar).
 */
import { describe, expect, it } from 'vitest'
import en from '../locales/en'
import fr from '../locales/fr'
import pt from '../locales/pt'
import zh from '../locales/zh'
import { accaoDaTecla, type TeclaDaMesa } from '../studio/tv/atalhos'
import {
  analisarCombinacao,
  atalhoPorId,
  atalhosDoEscopo,
  CATALOGO_DE_ATALHOS,
  combina,
  escreverAtalho,
  escreverFaixa,
  ESCOPOS,
  type EventoDeTecla,
} from './atalhos'

/** Um evento sintético a partir de uma combinação do catálogo. */
function evento(spec: string, extra: Partial<EventoDeTecla> = {}): EventoDeTecla & TeclaDaMesa {
  const c = analisarCombinacao(spec)
  const t = c.tecla
  let key = t
  let code = ''
  if (/^[0-9]$/.test(t)) {
    key = t
    code = `Digit${t}`
  } else if (/^f([1-9]|1[0-2])$/.test(t)) {
    key = t.toUpperCase()
    code = key
  } else if (t === 'space') {
    key = ' '
    code = 'Space'
  } else if (t === 'enter') {
    key = 'Enter'
    code = 'Enter'
  } else if (t === 'esc') {
    key = 'Escape'
    code = 'Escape'
  } else if (t === '?') {
    key = '?'
    code = 'Slash'
  } else {
    key = t
    code = `Key${t.toUpperCase()}`
  }
  return {
    key,
    code,
    ctrlKey: c.mod,
    metaKey: false,
    altKey: c.alt,
    shiftKey: c.shift || t === '?',
    repeat: false,
    target: null,
    ...extra,
  }
}

describe('combina', () => {
  it('o `mod` aceita o Ctrl e o ⌘, e exige um deles', () => {
    expect(combina('mod+k', { ...evento('mod+k') }, null)).toBe(true)
    expect(combina('mod+k', { ...evento('mod+k'), ctrlKey: false, metaKey: true }, null)).toBe(true)
    expect(combina('mod+k', { ...evento('mod+k'), ctrlKey: false }, null)).toBe(false)
  })

  it('um modificador a mais não combina — ⌘⇧K não é ⌘K', () => {
    expect(combina('mod+k', { ...evento('mod+k'), shiftKey: true }, null)).toBe(false)
    expect(combina('mod+k', { ...evento('mod+k'), altKey: true }, null)).toBe(false)
    expect(combina('alt+c', { ...evento('alt+c'), ctrlKey: true }, null)).toBe(false)
  })

  it('o dígito lê-se do `code`: com ⇧ a `key` de «1» não é «1»', () => {
    const e = { ...evento('mod+shift+2'), key: '"' }
    expect(combina('mod+shift+2', e, null)).toBe(true)
  })

  it('o «?» combina com ⇧+Slash mesmo quando a `key` não é «?»', () => {
    expect(combina('?', { ...evento('?'), key: '/' }, null)).toBe(true)
    expect(combina('?', { ...evento('?'), key: '?', shiftKey: false, code: '' }, null)).toBe(true)
  })

  it('não rouba teclas a quem escreve — nem pelo alvo nem pelo foco', () => {
    const campo = { tagName: 'INPUT' }
    expect(combina('alt+c', { ...evento('alt+c'), target: campo }, null)).toBe(false)
    expect(combina('alt+c', evento('alt+c'), campo)).toBe(false)
    expect(combina('alt+c', evento('alt+c'), { tagName: 'DIV', isContentEditable: true })).toBe(false)
  })

  it('ignora a repetição, a composição (IME) e um evento já tratado', () => {
    expect(combina('alt+c', { ...evento('alt+c'), repeat: true }, null)).toBe(false)
    expect(combina('alt+c', { ...evento('alt+c'), isComposing: true }, null)).toBe(false)
    expect(combina('alt+c', { ...evento('alt+c'), defaultPrevented: true }, null)).toBe(false)
  })
})

describe('escreverAtalho', () => {
  it('símbolos no Mac, palavras nos outros', () => {
    expect(escreverAtalho('mod+shift+2', true)).toBe('⌘⇧2')
    expect(escreverAtalho('mod+shift+2', false)).toBe('Ctrl+Shift+2')
    expect(escreverAtalho('alt+c', true)).toBe('⌥C')
    expect(escreverAtalho('alt+c', false)).toBe('Alt+C')
  })

  it('as teclas com nome escrevem-se por extenso', () => {
    expect(escreverAtalho('space', false)).toBe('Space')
    expect(escreverAtalho('esc', false)).toBe('Esc')
    expect(escreverAtalho('f1', false)).toBe('F1')
    expect(escreverAtalho('?', false)).toBe('?')
  })

  it('uma faixa escreve-se da primeira à última tecla', () => {
    expect(escreverFaixa('shift+1', '6', true)).toBe('⇧1–6')
    expect(escreverFaixa('alt+1', '4', false)).toBe('Alt+1–4')
    expect(escreverFaixa('f1', 'f6', false)).toBe('F1–F6')
  })
})

describe('o catálogo', () => {
  it('não repete um `id`', () => {
    const ids = CATALOGO_DE_ATALHOS.map((a) => a.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('cada combinação é reconhecida pelo próprio comparador', () => {
    for (const a of CATALOGO_DE_ATALHOS) {
      expect(combina(a.combinacao, evento(a.combinacao), null), `${a.id} (${a.combinacao})`).toBe(true)
    }
  })

  it('nenhum escopo repete uma combinação, e nenhum pisa o global', () => {
    const globais = new Set(atalhosDoEscopo('global').map((a) => a.combinacao))
    for (const escopo of ESCOPOS) {
      const vistas = new Map<string, string>()
      for (const a of atalhosDoEscopo(escopo)) {
        expect(vistas.get(a.combinacao), `${a.id} repete ${a.combinacao}`).toBeUndefined()
        vistas.set(a.combinacao, a.id)
        if (escopo !== 'global') expect(globais.has(a.combinacao), `${a.id} rouba um atalho global`).toBe(false)
      }
    }
  })

  it('a mesa de corte trata os oito atalhos que a folha lhe atribui', () => {
    const esperado: Record<string, string> = {
      mesaPrevia: 'previa',
      mesaAr: 'ar',
      mesaCortar: 'cortar',
      mesaMisturar: 'misturar',
      mesaLimpar: 'limpar',
      mesaStinger: 'stinger',
      mesaSobreposicao: 'sobreposicao',
      mesaMacro: 'macro',
    }
    const naMesa = atalhosDoEscopo('mesa')
    expect(naMesa.map((a) => a.id).sort()).toEqual(Object.keys(esperado).sort())
    for (const a of naMesa) {
      expect(accaoDaTecla(evento(a.combinacao))?.tipo, `${a.id} (${a.combinacao})`).toBe(esperado[a.id])
    }
  })

  it('e IGNORA todas as vistas do Estúdio — mudar de vista no ar não corta uma fonte', () => {
    for (const a of atalhosDoEscopo('estudio')) {
      expect(accaoDaTecla(evento(a.combinacao)), `${a.id} (${a.combinacao}) chega à mesa`).toBeNull()
    }
  })

  it('cada rótulo existe nas quatro línguas', () => {
    const dicionarios = { pt, en, fr, zh } as Record<string, unknown>
    const ler = (dic: unknown, chave: string) =>
      chave.split('.').reduce<unknown>((o, k) => (o && typeof o === 'object' ? (o as Record<string, unknown>)[k] : undefined), dic)
    for (const a of CATALOGO_DE_ATALHOS) {
      for (const [lingua, dic] of Object.entries(dicionarios)) {
        expect(typeof ler(dic, a.rotulo), `${a.rotulo} em ${lingua}`).toBe('string')
      }
    }
  })

  it('`atalhoPorId` devolve a entrada e recusa um `id` inventado', () => {
    expect(atalhoPorId('vistaEdicao').combinacao).toBe('mod+shift+2')
    // @ts-expect-error — o tipo já o impede; a guarda é para quem chamar de JS.
    expect(() => atalhoPorId('naoExiste')).toThrow()
  })
})
