import { describe, expect, it } from 'vitest'
import { isPaletteShortcut, isTypingTarget, ShortcutEvent, TypingProbe } from './hotkeys'

const el = (tagName: string, extra: Partial<TypingProbe> = {}): TypingProbe => ({ tagName, closest: () => null, ...extra })
const ev = (over: Partial<ShortcutEvent> = {}): ShortcutEvent => ({
  key: 'k',
  ctrlKey: true,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  target: el('BODY'),
  ...over,
})

describe('Ctrl/Cmd+K — a guarda de quem está a escrever', () => {
  it('abre fora de campos, com Ctrl ou com Cmd, maiúscula ou minúscula', () => {
    expect(isPaletteShortcut(ev(), el('BODY'))).toBe(true)
    expect(isPaletteShortcut(ev({ ctrlKey: false, metaKey: true }), el('BODY'))).toBe(true)
    expect(isPaletteShortcut(ev({ key: 'K' }), el('DIV'))).toBe(true)
    // um botão com foco não é escrita
    expect(isPaletteShortcut(ev({ target: el('BUTTON') }), el('BUTTON'))).toBe(true)
  })

  it('NÃO abre dentro de input, textarea, select ou contenteditable', () => {
    for (const tag of ['INPUT', 'TEXTAREA', 'SELECT', 'input']) {
      expect(isPaletteShortcut(ev({ target: el(tag) }), el(tag))).toBe(false)
    }
    const editavel = el('DIV', { isContentEditable: true })
    expect(isPaletteShortcut(ev({ target: editavel }), editavel)).toBe(false)
  })

  it('NÃO abre quando o FOCO está num campo, mesmo que o alvo do evento seja o body', () => {
    expect(isPaletteShortcut(ev({ target: el('BODY') }), el('INPUT'))).toBe(false)
  })

  it('NÃO abre na zona de escrita do quadro/canvas marcada com data-typing', () => {
    const noCanvas = el('CANVAS', { closest: (s: string) => (s === '[data-typing]' ? {} : null) })
    expect(isTypingTarget(noCanvas)).toBe(true)
    expect(isPaletteShortcut(ev({ target: noCanvas }), noCanvas)).toBe(false)
    // o canvas sem zona de escrita não é escrita
    expect(isPaletteShortcut(ev({ target: el('CANVAS') }), el('CANVAS'))).toBe(true)
  })

  it('outras teclas e combinações não contam, nem eventos já tratados ou em composição (IME)', () => {
    expect(isPaletteShortcut(ev({ ctrlKey: false }), el('BODY'))).toBe(false)
    expect(isPaletteShortcut(ev({ key: 'j' }), el('BODY'))).toBe(false)
    expect(isPaletteShortcut(ev({ shiftKey: true }), el('BODY'))).toBe(false)
    expect(isPaletteShortcut(ev({ altKey: true }), el('BODY'))).toBe(false)
    expect(isPaletteShortcut(ev({ defaultPrevented: true }), el('BODY'))).toBe(false)
    expect(isPaletteShortcut(ev({ isComposing: true }), el('BODY'))).toBe(false)
  })

  it('alvos nulos ou não-elementos não rebentam', () => {
    expect(isTypingTarget(null)).toBe(false)
    expect(isTypingTarget(undefined)).toBe(false)
    expect(isPaletteShortcut(ev({ target: null }), null)).toBe(true)
  })
})
