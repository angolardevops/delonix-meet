// MÓDULO DE APOIO dos scripts `revisao-*.mjs` — contexto com sessão guardada que
// volta a entrar quando o refresh token já rodou (o servidor de validação
// limita logins: entra-se só quando é preciso).
export const PORTA = process.env.PORTA ?? '5647'
export const APP = `http://127.0.0.1:${PORTA}`
export const STATE = new URL(PORTA !== '5647' ? `./.sessao-revisao-${PORTA}.json` : './.sessao-revisao.json', import.meta.url).pathname

export async function pagina(b, w, h, extra = {}) {
  const { existsSync } = await import('node:fs')
  const ctx = await b.newContext({ viewport: { width: w, height: h }, permissions: ['camera', 'microphone'], storageState: existsSync(STATE) ? STATE : undefined, ...extra })
  const p = await ctx.newPage()
  const erros = []
  p.on('pageerror', (e) => erros.push(e.message.slice(0, 160)))
  const irOriginal = p.goto.bind(p)
  p.goto = async (url, o) => {
    const r = await irOriginal(url, o)
    await p.waitForTimeout(1500)
    if (await p.locator('[data-testid=auth-email]').count()) {
      await p.evaluate(() => localStorage.setItem('dx_tour_v1', 'done'))
      await p.fill('[data-testid=auth-email]', process.env.DX_USER ?? 'demo@delonix.co.ao')
      await p.fill('[data-testid=auth-password]', process.env.DX_PASS ?? 'demo12345')
      await p.press('[data-testid=auth-password]', 'Enter')
      await p.waitForSelector('.shell', { timeout: 30000 })
      await ctx.storageState({ path: STATE })
      return irOriginal(url, o)
    }
    return r
  }
  const fechar = async () => {
    await ctx.storageState({ path: STATE }).catch(() => {})
    await ctx.close()
  }
  return { ctx, p, erros, fechar }
}
