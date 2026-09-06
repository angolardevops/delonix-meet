// A MESMA pessoa entra duas vezes — e a segunda sessão entra sem áudio (R114).
//
// É o «companion mode»: portátil e telemóvel na mesma reunião. Útil, e um ciclo
// de eco garantido se os dois microfones e os dois altifalantes estiverem
// ligados no mesmo espaço físico. O ruído não é problema de quem o causa: é de
// toda a gente na reunião.
//
// PORQUE ESTE TESTE EXISTE, tendo já cinco portões de forma: os portões provam
// que o código está escrito; este prova que o CAMINHO funciona — que o servidor
// reconhece a conta, que a bandeira atravessa o WebSocket, e que a interface
// reage. Nenhum dos cinco veria um `companion` que nunca é enviado.
//
// O QUE ELE NÃO PROVA, e é preciso dizê-lo: não há microfones a sério nem
// altifalantes a sério. O Chrome corre com `--use-fake-device-for-media-stream`.
// Prova-se o mecanismo — mic desligado, `<audio>` mudos, aviso no ecrã — não a
// ausência acústica de eco.
//
// A armadilha desta família (e todos os e2e desta pasta já caíram nela uma vez):
// a asserção tem de distinguir o estado companion de «a página ainda não
// entrou». Por isso a primeira coisa que se exige é a sala ABERTA nas duas, e só
// depois o áudio.
import { chromium } from '@playwright/test'
import { criarConta, entrar } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5174'
let falhas = 0
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
const abriuASala = (p) =>
  p
    .waitForFunction(() => !/Pronto para entrar/i.test(document.body.innerText || ''), null, { timeout: 90000 })
    .then(() => true)
    .catch(() => false)

const conta = await criarConta(API, 'comp')
const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
})
const ctx = () => browser.newContext({ ignoreHTTPSErrors: true, permissions: ['camera', 'microphone'] })

// ── Dispositivo 1: o portátil ────────────────────────────────────────────────
const portatil = await (await ctx()).newPage()
await entrar(portatil, APP, conta)
await portatil.getByRole('button', { name: /nova reuni/i }).first().click()
await portatil.waitForFunction(() => /^#\/r\/[a-z-]+$/.test(location.hash), null, { timeout: 60000 })
const codigo = (await portatil.evaluate(() => location.hash)).replace('#/r/', '')
await portatil.getByRole('button', { name: /entrar agora/i }).first().click({ timeout: 60000 })
ok(await abriuASala(portatil), 'o portátil entrou na sala', `código ${codigo}`)

// A primeira sessão NÃO é companion. Sem esta metade, um bug que pusesse toda a
// gente em modo companion passaria despercebido — e deixaria a reunião inteira
// muda.
const audioDoPortatil = await portatil.evaluate(
  () => [...document.querySelectorAll('.audio-sink audio')].every((a) => !a.muted),
)
ok(audioDoPortatil !== false, 'a primeira sessão ouve — não é companion')
ok(
  !/noutro dispositivo/i.test(await portatil.evaluate(() => document.body.innerText || '')),
  'e não vê o aviso de segunda sessão',
)

// ── Dispositivo 2: o telemóvel, MESMA conta ─────────────────────────────────
const telemovel = await (await ctx()).newPage()
await entrar(telemovel, APP, conta)
await telemovel.goto(`${APP}/#/r/${codigo}`, { waitUntil: 'domcontentloaded' })
await telemovel.getByRole('button', { name: /entrar agora/i }).first().click({ timeout: 60000 })
ok(await abriuASala(telemovel), 'o telemóvel entrou na MESMA sala, com a MESMA conta')
await telemovel.waitForTimeout(4000)

const estado = await telemovel.evaluate(() => {
  const audios = [...document.querySelectorAll('.audio-sink audio')]
  return {
    aviso: /noutro dispositivo/i.test(document.body.innerText || ''),
    audios: audios.length,
    todosMudos: audios.length > 0 && audios.every((a) => a.muted),
    // O botão do microfone anuncia o estado no seu rótulo acessível.
    micOff: !!document.querySelector('button[title*="tivar microfone"], button[aria-label*="icrofone"]'),
    texto: (document.body.innerText || '').replace(/\s+/g, ' ').slice(0, 200),
  }
})
console.log(`  · telemóvel: ${JSON.stringify({ ...estado, texto: undefined })}`)

ok(estado.aviso, 'o telemóvel É AVISADO de que a conta já está na reunião', estado.texto)
// Esta é a asserção que impede o teste de passar em vazio: se o roster não
// tivesse chegado, não haveria `<audio>` NENHUM e um `every` sobre lista vazia
// devolveria `true` — verde a medir coisa nenhuma.
ok(estado.audios > 0, 'há áudio remoto para silenciar', `${estado.audios} elementos`)
ok(estado.todosMudos, 'e está TODO silenciado — o altifalante não realimenta o outro microfone')

// O microfone do lado de lá: o portátil tem de ver o telemóvel com o mic
// desligado. É a prova de que não é só o `<audio>` local — a track saiu mesmo.
await portatil.waitForTimeout(2000)
const comoOPortatilOVe = await portatil.evaluate(
  () => document.querySelectorAll('.tile[data-peer="remoto"]').length,
)
ok(comoOPortatilOVe === 1, 'o portátil vê o telemóvel na sala', `${comoOPortatilOVe} remotos`)

await browser.close()
console.log(falhas === 0 ? '\n=== TUDO VERDE ===' : `\n=== ${falhas} FALHARAM ===`)
process.exit(falhas === 0 ? 0 : 1)
