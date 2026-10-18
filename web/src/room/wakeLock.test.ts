import { describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { criarGuardaDeEcra, type Sentinela } from './wakeLock'

function sentinelaFalsa(): Sentinela & { largadas: number } {
  const s = {
    released: false,
    largadas: 0,
    release: async () => {
      s.released = true
      s.largadas += 1
    },
  }
  return s
}

describe('o ecrã do telefone não apaga a meio do directo', () => {
  it('pede o lock quando a página está visível', async () => {
    const s = sentinelaFalsa()
    const g = criarGuardaDeEcra({ pedirAoSistema: async () => s, visivel: () => true })
    await g.pedir()
    expect(g.activo()).toBe(true)
    expect(g.pedidos()).toBe(1)
  })

  it('NÃO pede escondido — o browser recusava, e a recusa não é um defeito', async () => {
    let visivel = false
    const s = sentinelaFalsa()
    const g = criarGuardaDeEcra({ pedirAoSistema: async () => s, visivel: () => visivel })
    await g.pedir()
    expect(g.pedidos()).toBe(0)
    expect(g.activo()).toBe(false)
    // E ao voltar à vista, pega.
    visivel = true
    await g.pedir()
    expect(g.activo()).toBe(true)
  })

  it('VOLTA a pedir depois de o browser largar o lock ao esconder a página', async () => {
    // Este é o defeito que o #356 tinha: o lock morre quando a aba sai de
    // vista e não volta sozinho. Quem pede só na montagem fica sem ele para
    // o resto da sessão, com o sintoma igual a nunca o ter pedido.
    const primeira = sentinelaFalsa()
    const segunda = sentinelaFalsa()
    let aDar = primeira
    const g = criarGuardaDeEcra({ pedirAoSistema: async () => aDar, visivel: () => true })
    await g.pedir()
    expect(g.pedidos()).toBe(1)

    primeira.released = true // o browser largou-o por nós
    expect(g.activo()).toBe(false)

    aDar = segunda
    await g.pedir()
    expect(g.pedidos()).toBe(2)
    expect(g.activo()).toBe(true)
  })

  it('não pede duas vezes com um lock vivo, nem com um pedido em voo', async () => {
    const s = sentinelaFalsa()
    let resolver: (v: Sentinela) => void = () => {}
    const g = criarGuardaDeEcra({
      pedirAoSistema: () => new Promise<Sentinela>((r) => (resolver = r)),
      visivel: () => true,
    })
    const a = g.pedir()
    const b = g.pedir() // em voo: não deve contar
    resolver(s)
    await Promise.all([a, b])
    expect(g.pedidos()).toBe(1)

    await g.pedir() // já tem lock
    expect(g.pedidos()).toBe(1)
  })

  it('larga o lock ao sair, e um pedido que chegue DEPOIS de parar não fica pendurado', async () => {
    const s = sentinelaFalsa()
    let resolver: (v: Sentinela) => void = () => {}
    const g = criarGuardaDeEcra({
      pedirAoSistema: () => new Promise<Sentinela>((r) => (resolver = r)),
      visivel: () => true,
    })
    const p = g.pedir()
    g.parar() // a página desmontou enquanto o pedido estava em voo
    resolver(s)
    await p
    // Sem esta guarda o lock sobrevivia à página que o pediu: o ecrã do
    // telefone ficava aceso para sempre depois de sair da câmara.
    expect(s.released).toBe(true)
    expect(g.activo()).toBe(false)
  })

  it('larga é idempotente e não explode se o release falhar', async () => {
    const s = { released: false, release: () => Promise.reject(new Error('fechou')) }
    const g = criarGuardaDeEcra({ pedirAoSistema: async () => s, visivel: () => true })
    await g.pedir()
    await expect(g.largar()).resolves.toBeUndefined()
    await expect(g.largar()).resolves.toBeUndefined()
  })

  it('onde a API não existe não estoura nem mente', async () => {
    const aoFalhar = vi.fn()
    const g = criarGuardaDeEcra({ pedirAoSistema: undefined, visivel: () => true }, aoFalhar)
    await g.pedir()
    expect(g.activo()).toBe(false)
    expect(g.pedidos()).toBe(0)
    // Sem API não há falha a comunicar: o aparelho simplesmente não a tem.
    expect(aoFalhar).not.toHaveBeenCalled()
  })

  it('uma recusa do sistema é comunicada, não engolida', async () => {
    const aoFalhar = vi.fn()
    const g = criarGuardaDeEcra(
      { pedirAoSistema: () => Promise.reject(new Error('NotAllowedError')), visivel: () => true },
      aoFalhar,
    )
    await g.pedir()
    expect(aoFalhar).toHaveBeenCalledOnce()
    expect(g.activo()).toBe(false)
  })

  it('a página da câmara LIGA isto ao visibilitychange — senão o lock só vale até à primeira notificação', () => {
    const src = readFileSync(join(__dirname, '..', 'pages', 'PhoneCamera.tsx'), 'utf8')
    expect(src).toMatch(/criarGuardaDeEcra|usarGuardaDeEcra/)
    // A regressão a travar: pedir na montagem e nunca mais. Sem este
    // ouvinte, voltar à aba deixa o telefone sem lock e o ecrã apaga.
    //
    // Exige-se o `addEventListener` e não a palavra solta: a primeira versão
    // deste teste procurava só `visibilitychange`, e passou com o
    // `addEventListener` REMOVIDO — a palavra sobrevivia no
    // `removeEventListener` da limpeza e no comentário ao lado (medido).
    expect(src).toMatch(/addEventListener\(\s*['"]visibilitychange['"]/)
    expect(src).toMatch(/removeEventListener\(\s*['"]visibilitychange['"]/)
  })
})
