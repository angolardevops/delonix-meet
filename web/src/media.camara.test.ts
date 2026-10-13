/**
 * A câmara frontal no telemóvel: `facingMode`.
 *
 * PORQUE ESTES TESTES EXISTEM. Até aqui a única forma de escolher câmara era
 * `deviceId: { exact }`, e no telemóvel isso não serve: antes da permissão o
 * `enumerateDevices` não dá etiquetas úteis (no Safari do iOS vêm vazias) e os
 * `deviceId` rodam entre sessões — um `exact` com um id antigo FALHA em vez de
 * cair para outra câmara. O `facingMode` é o que o telefone sabe responder.
 */
import { beforeAll, describe, expect, it, vi } from 'vitest'

type Media = typeof import('./media')
let facingOf: Media['facingOf']
let videoConstraints: Media['videoConstraints']

beforeAll(async () => {
  // O `media.ts` arrasta o `api.ts`, que lê o `localStorage` ao carregar, e
  // esta bateria corre sem ambiente de DOM (nenhum dos outros 101 ficheiros
  // precisa de um). Dar-lhe um mínimo e importar a seguir é mais barato do que
  // acrescentar o `jsdom` ao projecto por causa de duas funções puras.
  const loja = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => loja.get(k) ?? null,
    setItem: (k: string, v: string) => void loja.set(k, String(v)),
    removeItem: (k: string) => void loja.delete(k),
    clear: () => loja.clear(),
    key: () => null,
    length: 0,
  })
  const m = await import('./media')
  facingOf = m.facingOf
  videoConstraints = m.videoConstraints
})

describe('videoConstraints', () => {
  it('sem nada, não impõe câmara nenhuma', () => {
    const c = videoConstraints()
    expect(c.deviceId).toBeUndefined()
    expect(c.facingMode).toBeUndefined()
  })

  it('com deviceId, exige aquele dispositivo', () => {
    expect(videoConstraints('abc').deviceId).toEqual({ exact: 'abc' })
  })

  it('com um lado, pede o lado — e como IDEAL, nunca exact', () => {
    // `exact` fazia o getUserMedia FALHAR num computador com uma só webcam, e
    // o vídeo ficava preto. O `ideal` degrada para o que existe.
    expect(videoConstraints(undefined, 'user').facingMode).toEqual({ ideal: 'user' })
    expect(videoConstraints(undefined, 'environment').facingMode).toEqual({ ideal: 'environment' })
  })

  it('o deviceId ganha ao lado: quem escolheu um dispositivo quer aquele', () => {
    const c = videoConstraints('abc', 'environment')
    expect(c.deviceId).toEqual({ exact: 'abc' })
    expect(c.facingMode).toBeUndefined()
  })

  it('não mexe no resto das restrições', () => {
    const c = videoConstraints(undefined, 'user')
    expect(c.width).toEqual({ ideal: 3840 })
    expect(c.frameRate).toEqual({ ideal: 60 })
  })
})

describe('facingOf', () => {
  const pista = (facingMode?: string) =>
    ({ getSettings: () => (facingMode ? { facingMode } : {}) }) as unknown as MediaStreamTrack

  it('diz o lado quando o browser o dá (telemóvel)', () => {
    expect(facingOf(pista('user'))).toBe('user')
    expect(facingOf(pista('environment'))).toBe('environment')
  })

  it('é undefined num computador — uma webcam não tem frente nem trás', () => {
    // É isto que decide se o botão de virar aparece: só onde o conceito existe.
    expect(facingOf(pista())).toBeUndefined()
  })

  it('não inventa um lado a partir de um valor que não conhece', () => {
    expect(facingOf(pista('left'))).toBeUndefined()
    expect(facingOf(pista('right'))).toBeUndefined()
  })

  it('aguenta não haver pista', () => {
    expect(facingOf(null)).toBeUndefined()
    expect(facingOf(undefined)).toBeUndefined()
  })
})
