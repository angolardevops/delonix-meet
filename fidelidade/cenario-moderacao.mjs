// Moderação (DelonixModeration): papéis, mão no ar, duas pessoas à porta e
// três salas paralelas com gente. Corre sobre `sala.mjs`.
export default async function ({ hp, convidados, code, esperar, fotografar, APP, log }) {
  const [, teresa] = convidados
  const passo = async (nome, fn) => {
    try {
      await fn()
      log('passo', nome)
    } catch (e) {
      log('FALHOU', nome, e.message.split('\n')[0])
    }
  }
  await hp.locator('.rm-tile[data-peer="remoto"]').nth(2).waitFor({ timeout: 20000 }).catch(() => {})
  const lp = await hp.context().newPage()
  await lp.goto(`${APP}/#/lobby/${code}`)
  await lp.locator('.lb-table').waitFor({ timeout: 20000 })
  await esperar(2500)
  const linha = (nome) => lp.locator('.lb-table tr', { hasText: nome })
  await passo('papéis', async () => {
    await linha('Joaquim').getByRole('button', { name: 'Promover' }).click({ timeout: 8000 })
    await linha('Teresa').getByRole('button', { name: 'Promover' }).click({ timeout: 8000 })
    await linha('Teresa').getByRole('button', { name: 'Passar palco' }).click({ timeout: 8000 })
  })
  await passo('mão', () => teresa.getByRole('button', { name: /levantar a mão/i }).click({ timeout: 8000 }))
  await passo('salas paralelas', async () => {
    await lp.locator('.rm-bocard select').selectOption('15').catch(() => {})
    await lp.getByRole('button', { name: /^3 grupos$/ }).click({ timeout: 8000 })
  })
  await esperar(12000)
  if (process.env.DEPURAR) {
    const [j] = convidados
    await j.screenshot({ path: `${process.env.DEPURAR}/joaquim-grupo.png` })
    log('joaquim está em', j.url())
  }
  await lp.mouse.move(5, 5)
  await fotografar(lp, 'DelonixModeration')
}
