// MÓDULO DE APOIO — não é um teste; é importado pelos testes de interface.
/**
 * Sessão para os testes de interface — registar uma conta NOVA e entrar por ela.
 *
 * PORQUE NÃO SE INJECTA UM TOKEN NO localStorage: parece mais rápido e funciona
 * contra um servidor simulado, mas contra o servidor a sério o token falso leva
 * 401, o cliente tenta renovar, falha, e a app faz logout — o teste acaba no
 * ecrã de login e falha por «selector não encontrado», que é um sintoma que não
 * aponta para a causa. Foi exactamente o que aconteceu ao `layout-consola.mjs`
 * quando passou de um mock para o CI.
 *
 * Cada corrida cria a sua própria organização. Um utilizador fixo partilhado
 * entre testes faz-nos depender da ordem em que correm, e o servidor impõe uma
 * organização por domínio de email.
 */

export const PASSWORD = 'delonix-e2e-2026'

/** Cria uma organização e a conta de admin dela. Devolve as credenciais. */
export async function criarConta(API, prefixo = 'e2e') {
  const marca = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`
  const email = `${prefixo}${marca}@${prefixo}${marca}.local`
  const resposta = await fetch(`${API}/api/auth/register`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      org_name: `${prefixo} ${marca}`,
      email,
      username: `${prefixo}${marca}`.slice(0, 30),
      password: PASSWORD,
    }),
  })
  if (!resposta.ok) {
    throw new Error(`registo falhou: ${resposta.status} ${await resposta.text()}`)
  }
  return { email, password: PASSWORD }
}

/**
 * Entra pela interface. Dispensa o tour de introdução ANTES de qualquer clique:
 * o `.tour-dim` intercepta todos os eventos de ponteiro e faz os cliques
 * seguintes expirarem sem explicação.
 */
export async function entrar(page, APP, { email, password }) {
  await page.goto(`${APP}/#/login`, { waitUntil: 'domcontentloaded', timeout: 120_000 })
  await page.waitForSelector('[data-testid=auth-email]', { timeout: 120_000 })
  await page.evaluate(() => localStorage.setItem('dx_tour_v1', 'done'))
  await page.fill('[data-testid=auth-email]', email)
  await page.fill('[data-testid=auth-password]', password)
  // Selectores por `data-testid`: o formulário tem vários «Entrar» (o submeter,
  // a troca para criar organização, a caixa do código de sala) e apanhar o
  // errado não envia nada — falha sem erro visível.
  await page.locator('[data-testid=auth-submit]').click()
  await page.waitForFunction(() => !document.querySelector('[data-testid=auth-email]'), null, { timeout: 60_000 })
  await page.waitForSelector('.shell', { timeout: 60_000 })
}
