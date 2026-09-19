import { describe, expect, it } from 'vitest'
import { alignDeltas, distributeDeltas, groupShortcut, isApplePlatform, unionBox } from './arrange'

const b = (id: string, x: number, y: number, w = 10, h = 10) => ({ id, x, y, w, h })

describe('selecção múltipla · alinhar e distribuir', () => {
  it('alinhar à esquerda leva todos ao x mais pequeno e não mexe no que já lá está', () => {
    const d = alignDeltas([b('a', 5, 0), b('b', 20, 30), b('c', 42, 7)], 'left')
    expect(d.has('a')).toBe(false)
    expect(d.get('b')).toEqual({ dx: -15, dy: 0 })
    expect(d.get('c')).toEqual({ dx: -37, dy: 0 })
  })

  it('centrar na horizontal usa o centro da caixa que envolve todos', () => {
    const d = alignDeltas([b('a', 0, 0, 10), b('b', 90, 0, 10)], 'centerX')
    expect(d.get('a')).toEqual({ dx: 45, dy: 0 })
    expect(d.get('b')).toEqual({ dx: -45, dy: 0 })
  })

  it('alinhar ao topo', () => {
    const d = alignDeltas([b('a', 0, 12), b('b', 40, 3)], 'top')
    expect(d.get('a')).toEqual({ dx: 0, dy: -9 })
    expect(d.has('b')).toBe(false)
  })

  it('com uma só unidade não há nada a alinhar', () => {
    expect(alignDeltas([b('a', 0, 0)], 'left').size).toBe(0)
  })

  it('distribuir deixa as pontas e iguala o espaço ENTRE BORDAS', () => {
    // 0..10, 12..32 (w 20), 90..100 → vão livre 100 - 40 = 60 → 30 entre cada
    const d = distributeDeltas([b('c', 90, 0), b('a', 0, 0), b('m', 12, 0, 20)], 'x')
    expect(d.has('a')).toBe(false)
    expect(d.has('c')).toBe(false)
    expect(d.get('m')).toEqual({ dx: 28, dy: 0 })
  })

  it('distribuir precisa de três; na vertical mexe só no y', () => {
    expect(distributeDeltas([b('a', 0, 0), b('b', 0, 50)], 'y').size).toBe(0)
    const d = distributeDeltas([b('a', 0, 0), b('b', 7, 11), b('c', 3, 60)], 'y')
    expect(d.get('b')).toEqual({ dx: 0, dy: 19 })
  })

  it('unionBox', () => {
    expect(unionBox([])).toBeNull()
    expect(unionBox([b('a', 0, 0), b('b', 20, 5, 5, 30)])).toEqual({ x: 0, y: 0, w: 25, h: 35 })
  })
})

describe('selecção múltipla · atalho de agrupar', () => {
  const ev = (key: string, o: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean }> = {}) => ({
    key,
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    ...o,
  })
  it('⌘G e Ctrl+G agrupam; com ⇧ desagrupam', () => {
    expect(groupShortcut(ev('g', { metaKey: true }))).toBe('group')
    expect(groupShortcut(ev('g', { ctrlKey: true }))).toBe('group')
    expect(groupShortcut(ev('G', { ctrlKey: true, shiftKey: true }))).toBe('ungroup')
  })
  it('não é o Ctrl+K, nem G sem modificador, nem com Alt', () => {
    expect(groupShortcut(ev('k', { ctrlKey: true }))).toBeNull()
    expect(groupShortcut(ev('g'))).toBeNull()
    expect(groupShortcut(ev('g', { ctrlKey: true, altKey: true }))).toBeNull()
  })
  it('o símbolo ⌘ só na Apple', () => {
    expect(isApplePlatform({ platform: 'MacIntel' })).toBe(true)
    expect(isApplePlatform({ platform: 'Linux x86_64', userAgent: 'X11' })).toBe(false)
    expect(isApplePlatform({ platform: 'Win32' })).toBe(false)
  })
})
