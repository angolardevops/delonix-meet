// Telemóvel (DelonixMobile, reunião activa): um convidado a 390×844, com quem
// está na sala à volta. Corre sobre `sala.mjs` (CENARIO=./cenario-mobile.mjs).
export default async function ({ hp, convidados, esperar, fotografar, log }) {
  const [joaquim] = convidados
  await hp.locator('.rm-tile[data-peer="remoto"]').nth(2).waitFor({ timeout: 20000 }).catch(() => {})
  await esperar(1500)
  await joaquim.setViewportSize({ width: 390, height: 844 })
  await joaquim.reload()
  const entrar = joaquim.getByRole('button', { name: /^entrar na sess/i })
  if (await entrar.waitFor({ timeout: 6000 }).then(() => true).catch(() => false)) await entrar.click()
  await joaquim.locator('.rm-shell').waitFor({ timeout: 30000 }).catch((e) => log('telemóvel', e.message.split('\n')[0]))
  await esperar(5000)
  await fotografar(joaquim, 'DelonixMobile')
}
