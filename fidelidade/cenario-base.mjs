// Cenário mínimo (antes do lote 2): admite três e fotografa.
export default async function ({ hp, convidados, esperar, fotografar, want, code, APP, log }) {
  for (const nome of ['Joaquim', 'Teresa', 'Domingos']) {
    const row = hp.locator('.rm-notice__row', { hasText: nome })
    await row.locator('.rm-admit-accept').click({ timeout: 15000 }).catch((e) => log('admitir', nome, e.message))
    await esperar(600)
  }
  await esperar(6000)
  if (want.has('DelonixRoomGrid')) await fotografar(hp, 'DelonixRoomGrid')
  await convidados[0].getByRole('button', { name: /chat/i }).first().click().catch(() => {})
  await hp.getByRole('button', { name: /chat/i }).first().click().catch(() => {})
  await esperar(1500)
  if (want.has('DelonixRoomChat')) await fotografar(hp, 'DelonixRoomChat')
  await hp.getByRole('button', { name: /quadro/i }).first().click().catch(() => {})
  await esperar(2000)
  if (want.has('DelonixWhiteboard')) await fotografar(hp, 'DelonixWhiteboard')
  const lp = await hp.context().newPage()
  await lp.goto(`${APP}/#/lobby/${code}`)
  await esperar(4000)
  if (want.has('DelonixModeration')) await fotografar(lp, 'DelonixModeration')
}
