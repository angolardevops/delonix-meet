import { describe, expect, it } from 'vitest'
import { AmbienteDoBrowser, chaveEmBytes, suportePush } from './webPushSuporte'

const chrome: AmbienteDoBrowser = {
  temServiceWorker: true,
  temPushManager: true,
  temNotification: true,
  permissao: 'default',
  ios: false,
  instalada: false,
}

describe('suportePush — o que o ecrã pode oferecer', () => {
  it('Chrome/Firefox/Edge com service worker: pronto', () => {
    expect(suportePush(chrome)).toBe('pronto')
  })

  it('iPhone sem instalar: pede a instalação — mesmo sem PushManager, a resposta não é «sem suporte»', () => {
    expect(suportePush({ ...chrome, ios: true, temPushManager: false, temNotification: false })).toBe('ios-instalar')
  })

  it('iPhone com a aplicação instalada: pronto', () => {
    expect(suportePush({ ...chrome, ios: true, instalada: true })).toBe('pronto')
  })

  it('sem service worker (servidor de desenvolvimento) ou sem PushManager: sem suporte', () => {
    expect(suportePush({ ...chrome, temServiceWorker: false })).toBe('sem-suporte')
    expect(suportePush({ ...chrome, temPushManager: false })).toBe('sem-suporte')
  })

  it('notificações recusadas a este site: bloqueado — só a pessoa o desfaz', () => {
    expect(suportePush({ ...chrome, permissao: 'denied' })).toBe('bloqueado')
  })
})

describe('chaveEmBytes', () => {
  it('converte o base64url da chave VAPID nos 65 bytes do ponto P-256', () => {
    // Um ponto P-256 não comprimido: 0x04 seguido de 64 bytes — 87 caracteres em base64url.
    const bytes = new Uint8Array(65)
    bytes[0] = 4
    bytes[64] = 255
    const b64url = btoa(String.fromCharCode(...bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
    const out = chaveEmBytes(b64url)
    expect(out.length).toBe(65)
    expect(out[0]).toBe(4)
    expect(out[64]).toBe(255)
  })
})
