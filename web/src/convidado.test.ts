/**
 * A sessão do convidado sem conta (`convidado.ts`): o bilhete guarda-se por
 * sala, renova-se quando o token expira, e é por ele que a sala sabe quem está
 * deste lado. O servidor está provado em `web/e2e/convidado.mjs`; aqui prova-se
 * a decisão do cliente — sobretudo a que não pode falhar: quem tem conta nunca
 * entra como convidado.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'

const armazem = () => {
  const m = new Map<string, string>()
  return {
    m,
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, String(v)),
    removeItem: (k: string) => void m.delete(k),
    clear: () => m.clear(),
  }
}
const local = armazem()
const sessao = armazem()
vi.stubGlobal('localStorage', local)
vi.stubGlobal('sessionStorage', sessao)
vi.stubGlobal('window', { dispatchEvent: () => true, addEventListener: () => {} })
const endereco = { hash: '', protocol: 'https:', host: 'meet.teste' }
vi.stubGlobal('location', endereco)

const { bilheteExpirado, convidadoDe, entrarNaSala, esquecerConvidado, guardarConvidado, MARGEM_DE_RENOVACAO_MS, nomeDeConvidado, participanteLocal, souConvidado } =
  await import('./convidado')

const CODE = 'abc-defg-hij'
const bilhete = (token = 'tok-1', nome = 'Ana') => ({
  room: { code: CODE, name: 'Conselho', topology: 'sfu', e2ee: false, format: 'normal' },
  room_token: token,
  ws_path: `/ws?token=${token}`,
  expires_in: 300,
  guest: { id: 'g-1', display_name: nome },
  ice_servers: { iceServers: [{ urls: 'turn:meet.teste:3478' }] },
})
const resposta = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })

beforeEach(() => {
  local.clear()
  sessao.clear()
  esquecerConvidado(CODE)
  endereco.hash = ''
  vi.restoreAllMocks()
})

describe('o bilhete do convidado', () => {
  it('guarda-se por sala, lembra o nome, e esquece-se quando ele sai', () => {
    const s = guardarConvidado(bilhete(), 1_000)
    expect(s).toMatchObject({ code: CODE, roomToken: 'tok-1', expiraEm: 301_000 })
    expect(convidadoDe(CODE)?.guest.display_name).toBe('Ana')
    expect(convidadoDe('outra-sala')).toBeNull()
    expect(nomeDeConvidado()).toBe('Ana')
    esquecerConvidado(CODE)
    expect(convidadoDe(CODE)).toBeNull()
    // O nome fica: é o que vem preenchido da próxima vez.
    expect(nomeDeConvidado()).toBe('Ana')
  })

  it('está expirado a 30 segundos do fim, não só depois dele', () => {
    const s = guardarConvidado(bilhete(), 0)
    expect(bilheteExpirado(s, 0)).toBe(false)
    expect(bilheteExpirado(s, 300_000 - MARGEM_DE_RENOVACAO_MS - 1)).toBe(false)
    expect(bilheteExpirado(s, 300_000 - MARGEM_DE_RENOVACAO_MS + 1)).toBe(true)
    expect(bilheteExpirado(s, 400_000)).toBe(true)
  })

  it('um bilhete ilegível no armazenamento é como não haver', () => {
    sessao.setItem(`dx_convidado_${CODE}`, '{isto não é json')
    expect(convidadoDe(CODE)).toBeNull()
  })
})

describe('entrarNaSala', () => {
  it('um convidado entra com o que o bilhete traz, sem pedir nada ao servidor', async () => {
    const rede = vi.fn()
    vi.stubGlobal('fetch', rede)
    guardarConvidado(bilhete(), 0)
    const [{ room, room_token, scheduled }, ice] = await entrarNaSala(CODE, 10_000)
    expect(rede).not.toHaveBeenCalled()
    expect(room_token).toBe('tok-1')
    expect(ice).toEqual({ iceServers: [{ urls: 'turn:meet.teste:3478' }] })
    // Nunca dono, e sempre pela sala de espera — diga o que disser a sala.
    expect(room).toMatchObject({ code: CODE, name: 'Conselho', topology: 'sfu', owner_id: '', waiting_room: true })
    expect(scheduled).toBeUndefined()
  })

  it('com o token expirado pede outro com o MESMO nome, e é esse que usa', async () => {
    const rede = vi.fn(async () => resposta(200, bilhete('tok-2')))
    vi.stubGlobal('fetch', rede)
    guardarConvidado(bilhete('tok-1'), 0)
    const [{ room_token }] = await entrarNaSala(CODE, 290_000)
    expect(room_token).toBe('tok-2')
    expect(rede).toHaveBeenCalledTimes(1)
    const [url, init] = rede.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe(`/api/rooms/${CODE}/guest-join`)
    expect(JSON.parse(init.body as string)).toEqual({ display_name: 'Ana' })
    expect(convidadoDe(CODE)?.roomToken).toBe('tok-2')
  })

  it('a recusa da renovação chega a quem chama — não se entra com o token velho', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => resposta(403, { error: 'fechada' })))
    guardarConvidado(bilhete('tok-1'), 0)
    await expect(entrarNaSala(CODE, 400_000)).rejects.toMatchObject({ reason: 'closed' })
  })

  it('QUEM TEM CONTA nunca entra como convidado, mesmo com um bilhete antigo guardado', async () => {
    guardarConvidado(bilhete('tok-de-convidado'), 0)
    local.setItem('dx_user', JSON.stringify({ id: 'u-1', username: 'Membro' }))
    const rede = vi.fn(async (url: string) =>
      url.includes('/join')
        ? resposta(200, { room: { id: 'r', code: CODE, name: 'Conselho', owner_id: 'u-1', topology: 'sfu', waiting_room: false, e2ee: false }, room_token: 'tok-de-membro' })
        : resposta(200, { iceServers: [] }),
    )
    vi.stubGlobal('fetch', rede)
    const [{ room, room_token }] = await entrarNaSala(CODE, 10_000)
    expect(room_token).toBe('tok-de-membro')
    expect(room.owner_id).toBe('u-1')
    expect(rede.mock.calls.map((c) => c[0])).toEqual([`/api/rooms/${CODE}/join`, '/api/ice-servers'])
  })

  it('sem conta e sem bilhete vai pelo caminho dos membros (e leva o 401 que lhe cabe)', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => resposta(401, { error: 'unauthorized' })))
    await expect(entrarNaSala(CODE)).rejects.toMatchObject({ status: 401 })
  })
})

describe('quem está deste lado da sala', () => {
  it('é a conta, quando há conta', () => {
    local.setItem('dx_user', JSON.stringify({ id: 'u-1', username: 'Membro' }))
    endereco.hash = `#/r/${CODE}`
    guardarConvidado(bilhete(), 0)
    expect(participanteLocal()).toEqual({ id: 'u-1', username: 'Membro' })
    expect(souConvidado()).toBe(false)
  })

  it('é o convidado da sala que o endereço mostra, quando não há conta', () => {
    guardarConvidado(bilhete('t', 'João Baptista'), 0)
    endereco.hash = `#/r/${CODE}`
    expect(participanteLocal()).toEqual({ id: 'g-1', username: 'João Baptista' })
    expect(souConvidado()).toBe(true)
    // Com o `?voice` da chamada de voz também.
    endereco.hash = `#/r/${CODE}?voice`
    expect(souConvidado()).toBe(true)
  })

  it('fora da sala do bilhete não é ninguém — o bilhete de uma sala não vale noutra', () => {
    guardarConvidado(bilhete(), 0)
    endereco.hash = '#/r/outra-sala-xyz'
    expect(participanteLocal()).toEqual({ id: null, username: '' })
    expect(souConvidado()).toBe(false)
    endereco.hash = '#/login'
    expect(souConvidado()).toBe(false)
  })
})
