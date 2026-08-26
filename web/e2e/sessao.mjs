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
  await page.waitForSelector('input[type=email]', { timeout: 120_000 })
  await page.evaluate(() => localStorage.setItem('dx_tour_v1', 'done'))
  await page.fill('input[type=email]', email)
  await page.fill('input[type=password]', password)
  // O cartão tem DOIS «Entrar»: o separador e o botão de submeter. Apanhar o
  // primeiro troca de aba e não envia nada — falha sem erro visível.
  await page.locator('form button.primary, .auth-card button[type=submit]').first().click()
  await page.waitForFunction(() => !document.querySelector('input[type=email]'), null, { timeout: 60_000 })
  await page.waitForSelector('.shell', { timeout: 60_000 })
}
