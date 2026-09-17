import { describe, expect, it } from 'vitest'
import { createStore, immersiveTarget, sameTarget, sharpenTarget, type TargetInput } from './target'

const peers = [
  { peerId: 'a', username: 'Ana', camOn: true },
  { peerId: 'b', username: 'Bruno', camOn: false },
]
const base: TargetInput = { presentationPeerId: null, viewMode: 'stage', stagePeerId: 'a', stageOnSelf: false, peers }

describe('alvo do realce e do palco imersivo', () => {
  it('orador remoto em palco, com câmara: é o alvo dos dois', () => {
    expect(sharpenTarget(base)).toEqual({ kind: 'peer', peerId: 'a', name: 'Ana' })
    expect(immersiveTarget(base)).toEqual({ kind: 'peer', peerId: 'a', name: 'Ana' })
  })
  it('apresentação remota: o realce vai para ela; o imersivo não se aplica', () => {
    const i = { ...base, presentationPeerId: 'b' }
    expect(sharpenTarget(i)).toEqual({ kind: 'presentation', peerId: 'b' })
    expect(immersiveTarget(i)).toBeNull()
  })
  it('a minha apresentação ou a minha imagem em palco: nada (não é vídeo recebido)', () => {
    expect(sharpenTarget({ ...base, presentationPeerId: 'me' })).toBeNull()
    expect(sharpenTarget({ ...base, stageOnSelf: true })).toBeNull()
    expect(immersiveTarget({ ...base, stageOnSelf: true })).toBeNull()
  })
  it('grelha: não há destaque', () => {
    expect(sharpenTarget({ ...base, viewMode: 'grid' })).toBeNull()
    expect(immersiveTarget({ ...base, viewMode: 'grid' })).toBeNull()
  })
  it('orador sem câmara, ou que já saiu: nada', () => {
    expect(immersiveTarget({ ...base, stagePeerId: 'b' })).toBeNull()
    expect(sharpenTarget({ ...base, stagePeerId: 'zz' })).toBeNull()
  })
  it('sameTarget compara tipo e pessoa', () => {
    expect(sameTarget(null, null)).toBe(true)
    expect(sameTarget({ kind: 'peer', peerId: 'a', name: 'x' }, { kind: 'peer', peerId: 'a', name: 'y' })).toBe(true)
    expect(sameTarget({ kind: 'peer', peerId: 'a', name: 'x' }, { kind: 'presentation', peerId: 'a' })).toBe(false)
  })
})

describe('createStore — só notifica quando muda (R21)', () => {
  it('não dispara com o mesmo valor', () => {
    const s = createStore<number | null>(null)
    let n = 0
    const off = s.subscribe(() => n++)
    s.set(null)
    s.set(1)
    s.set(1)
    off()
    s.set(2)
    expect(n).toBe(1)
    expect(s.get()).toBe(2)
  })
})
