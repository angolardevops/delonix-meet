/**
 * O som que pertence a uma imagem.
 *
 * Uma câmara entra nas Fontes sem som, e é certo que entre: pedir
 * `getUserMedia({ video: { deviceId }, audio: true })` devolve o microfone por
 * OMISSÃO do sistema, não o da câmara — dois dispositivos diferentes a fingir
 * ser um. Quem os casa é o `groupId`, e é essa regra que se prende aqui.
 */
import { describe, expect, it } from 'vitest'
import { microfoneDoGrupo } from './fontes'

const mics = [
  { deviceId: 'm1', groupId: 'g-webcam' },
  { deviceId: 'm2', groupId: 'g-hdmi' },
  { deviceId: 'm3', groupId: '' },
]

describe('o microfone de uma fonte', () => {
  it('é o do mesmo grupo de hardware', () => {
    expect(microfoneDoGrupo(mics, 'g-hdmi')?.deviceId).toBe('m2')
    expect(microfoneDoGrupo(mics, 'g-webcam')?.deviceId).toBe('m1')
  })

  it('e um grupo que não existe não inventa um microfone', () => {
    expect(microfoneDoGrupo(mics, 'g-nenhum')).toBeNull()
  })

  it('um grupo VAZIO não casa com nada — nem com o microfone sem grupo', () => {
    // Antes da permissão o browser devolve `groupId: ''` em tudo: sem esta
    // guarda, a primeira câmara ligada «trazia» o microfone do portátil.
    expect(microfoneDoGrupo(mics, '')).toBeNull()
    expect(microfoneDoGrupo(mics, undefined)).toBeNull()
  })
})
