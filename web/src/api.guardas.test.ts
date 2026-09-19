/**
 * Guardas de sessão e de aborto — o padrão do `delonix-portal`
 * (src/api/client.ts) trazido para o meet.
 *
 * Estes três testes existem porque o portal já pagou por eles: o `isAbort` em
 * falta punha a consola no login sozinha em desenvolvimento (eram onze sítios),
 * e a confusão entre «sessão inválida» e «não falei com o servidor» tirava o
 * utilizador de onde estava por causa de um 502 que não era problema dele.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'

// O `api.ts` lê o localStorage no topo do módulo e a bateria corre em `node`
// (não há jsdom instalado neste repo). O esboço tem de existir ANTES do import,
// por isso o módulo entra por `await import()` e não por `import` estático.
const memoria = new Map<string, string>()
vi.stubGlobal('localStorage', {
  getItem: (k: string) => memoria.get(k) ?? null,
  setItem: (k: string, v: string) => void memoria.set(k, String(v)),
  removeItem: (k: string) => void memoria.delete(k),
  clear: () => memoria.clear(),
})
vi.stubGlobal('window', { dispatchEvent: () => true, addEventListener: () => {} })

const { ApiError, apiErrorMessage, isAbort, isAuthFailure, listMeetings } = await import('./api')

describe('isAbort', () => {
  it('reconhece o AbortError de um controller real', async () => {
    const ctrl = new AbortController()
    ctrl.abort()
    const err = await fetch('http://127.0.0.1:1/nada', { signal: ctrl.signal }).catch((e) => e)
    expect(isAbort(err)).toBe(true)
  })

  it('não confunde uma falha de rede com um aborto', () => {
    expect(isAbort(new TypeError('Failed to fetch'))).toBe(false)
    expect(isAbort(new ApiError(500, null, 'boom'))).toBe(false)
    expect(isAbort(null)).toBe(false)
  })
})

describe('isAuthFailure', () => {
  it('é verdade só quando o servidor DIZ que a sessão não serve', () => {
    expect(isAuthFailure(new ApiError(401, null, ''))).toBe(true)
    expect(isAuthFailure(new ApiError(403, null, ''))).toBe(true)
  })

  it('um servidor avariado NÃO é sessão inválida', () => {
    // Era isto que tirava a pessoa de onde estava por um problema que não é dela.
    for (const st of [500, 502, 503, 504]) {
      expect(isAuthFailure(new ApiError(st, null, ''))).toBe(false)
    }
    expect(isAuthFailure(new TypeError('Failed to fetch'))).toBe(false)
  })
})

describe('apiErrorMessage', () => {
  it('lê as duas formas que a API devolve', () => {
    expect(apiErrorMessage(new ApiError(400, { error: 'código inválido' }, ''), 'x')).toBe('código inválido')
    expect(apiErrorMessage(new ApiError(400, 'texto simples', ''), 'x')).toBe('texto simples')
  })
  it('recorre ao fallback quando não há nada legível', () => {
    expect(apiErrorMessage(new ApiError(500, null, ''), 'genérico')).toBe('genérico')
    expect(apiErrorMessage(null, 'genérico')).toBe('genérico')
  })
})

describe('a sessão só termina quando o servidor o diz', () => {
  beforeEach(() => {
    memoria.clear()
    localStorage.setItem('dx_user', JSON.stringify({ id: 1, username: 'w', email: 'w@x' }))
    localStorage.setItem('dx_access', 'tok')
  })

  it('um 503 no refresh NÃO desloga', async () => {
    const respostas = [
      new Response('{}', { status: 401 }),          // o pedido original
      new Response('{}', { status: 503 }),          // o gateway do refresh, avariado
    ]
    vi.stubGlobal('fetch', vi.fn(async () => respostas.shift()!))
    await expect(listMeetings()).rejects.toThrow()
    expect(localStorage.getItem('dx_user')).not.toBeNull()   // continua autenticado
  })
})

describe('renovação de sessão concorrente (R98)', () => {
  // O servidor ROTA o refresh token: usá-lo revoga-o e emite um par novo. Duas
  // renovações ao mesmo tempo mandam o MESMO cookie — a primeira roda-o, a
  // segunda encontra-o revogado e leva 401, e o cliente faz logout. Ou seja: o
  // utilizador é posto na página de entrada por ter feito duas coisas ao mesmo
  // tempo.
  //
  // Este esboço reproduz a rotação: o SEGUNDO `POST /api/auth/refresh` devolve
  // 401, como o servidor faria. Sem a guarda de concorrência, uma das duas
  // chamadas rebenta com «session expired».
  it('duas chamadas que levam 401 renovam UMA vez, e as duas sobrevivem', async () => {
    memoria.set('dx_user', JSON.stringify({ id: 'u1', username: 'eu' }))
    let refreshes = 0
    let acessoValido = false

    vi.stubGlobal('fetch', async (url: string) => {
      const u = String(url)
      if (u.includes('/api/auth/refresh')) {
        refreshes++
        // A rotação: só a PRIMEIRA renovação serve. Assim é o servidor.
        if (refreshes > 1) return { ok: false, status: 401, json: async () => ({}) }
        // Lenta de propósito: é a janela em que a segunda chamada entraria.
        await new Promise((r) => setTimeout(r, 25))
        acessoValido = true
        return { ok: true, status: 200, json: async () => ({ access_token: 'novo', user: { id: 'u1' } }) }
      }
      if (!acessoValido) return { ok: false, status: 401, json: async () => ({}) }
      return { ok: true, status: 200, json: async () => ({ meetings: [] }) }
    })

    // As duas partem juntas, como acontece depois de um F5.
    const [a, b] = await Promise.allSettled([listMeetings(), listMeetings()])
    expect(a.status).toBe('fulfilled')
    expect(b.status).toBe('fulfilled')
    expect(refreshes).toBe(1)
  })
})
