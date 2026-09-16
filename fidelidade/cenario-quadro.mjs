// Quadro: o anfitrião abre, escreve um título, desenha formas e setas; o
// Joaquim deixa uma nota e a Teresa desenha; os cursores de ambos ficam no
// quadro do anfitrião. Depois, o quadro partilhado (ajustes abertos).
export default async function ({ hp, convidados, esperar, fotografar, want, log, passo }) {
  const [joaquim, teresa] = convidados
  const ferramenta = (p, nome) => p.locator('.rm-wb__tools button', { hasText: new RegExp(`^${nome}$`) }).first().click({ timeout: 6000 })
  const caixa = async (p) => p.locator('.rm-wb__live').boundingBox()
  const escrever = async (p, x, y, texto) => {
    const b = await caixa(p)
    await p.mouse.click(b.x + x, b.y + y)
    const ed = p.locator('.rm-wbeditor')
    await ed.waitFor({ timeout: 4000 })
    await ed.fill(texto)
    await ed.press('Enter')
    await esperar(300)
  }
  const arrastar = async (p, x0, y0, x1, y1, passos = 12, onda = 0) => {
    const b = await caixa(p)
    await p.mouse.move(b.x + x0, b.y + y0)
    await p.mouse.down()
    for (let k = 1; k <= passos; k++) {
      await p.mouse.move(b.x + x0 + ((x1 - x0) * k) / passos, b.y + y0 + ((y1 - y0) * k) / passos + (onda ? Math.sin(k / 2) * onda : 0))
    }
    await p.mouse.up()
    await esperar(200)
  }

  await passo('fechar painéis e abrir o quadro', async () => {
    // Quem estava à porta entra (admit-all), e os avisos flutuantes (sondagem)
    // saem — não fazem parte do ecrã do template.
    await hp.locator('.rm-notices button', { hasText: /^admitir todos/i }).first().click({ timeout: 3000 }).catch(() => {})
    for (const p of [hp, ...convidados]) await p.locator('.rm-notices button[aria-label="Dispensar"]').first().click({ timeout: 1500 }).catch(() => {})
    await hp.getByRole('button', { name: /^chat$/i }).first().click({ timeout: 5000 }).catch(() => {})
    await hp.getByRole('button', { name: /^quadro branco$/i }).first().click({ timeout: 8000 })
    await hp.locator('.rm-wb__live').waitFor({ timeout: 8000 })
    await esperar(1500)
  })
  await passo('título e caixas (anfitrião)', async () => {
    await ferramenta(hp, 'Texto')
    await escrever(hp, 34, 26, 'Fluxo de chamada — entrada PSTN')
    await ferramenta(hp, 'Formas')
    await arrastar(hp, 34, 84, 192, 150)
    await arrastar(hp, 278, 84, 436, 150)
    await arrastar(hp, 522, 84, 694, 150)
    await hp.getByRole('radio', { name: 'Seta' }).click()
    await arrastar(hp, 200, 112, 270, 112)
    await arrastar(hp, 444, 112, 514, 112)
    await ferramenta(hp, 'Texto')
    await escrever(hp, 46, 100, 'SBC')
    await escrever(hp, 290, 100, 'Kamailio')
    await escrever(hp, 534, 100, 'FreeSWITCH')
  })
  await passo('nota (Joaquim) e traço (Teresa)', async () => {
    for (const p of [joaquim, teresa]) await p.locator('.rm-wb__live').waitFor({ timeout: 10000 })
    await ferramenta(joaquim, 'Nota')
    await escrever(joaquim, 34, 264, 'Se o SBC primário cair, a rota alternativa tem de manter o PIN da sala.')
    await ferramenta(teresa, 'Caneta')
    await arrastar(teresa, 420, 300, 700, 280, 20, 14)
  })
  await passo('cursores no quadro do anfitrião', async () => {
    const bj = await caixa(joaquim)
    const bt = await caixa(teresa)
    for (let k = 0; k < 10; k++) {
      await joaquim.mouse.move(bj.x + 230 + k * 2, bj.y + 126)
      await teresa.mouse.move(bt.x + 680 + k * 2, bt.y + 236)
      await esperar(80)
    }
  })
  await esperar(900)
  await hp.mouse.move(5, 5)
  if (want.has('DelonixWhiteboard')) await fotografar(hp, 'DelonixWhiteboard')

  await passo('quadro partilhado (ajustes)', async () => {
    await ferramenta(hp, 'Ajustes')
    await esperar(1200)
    const bj = await caixa(joaquim)
    for (let k = 0; k < 8; k++) {
      await joaquim.mouse.move(bj.x + 600 + k * 2, bj.y + 210)
      await esperar(80)
    }
  })
  await esperar(600)
  if (want.has('DelonixBoardShared')) await fotografar(hp, 'DelonixBoardShared')
  log('quadro: objectos no anfitrião', await hp.locator('.rm-wb').getAttribute('data-objects'))
}
