#!/usr/bin/env node
// Chamada de VOZ a partir dos contactos, passagem a vídeo na mesma sessão, e
// SMS a um contacto — contra um servidor, um Postgres e um SFU A SÉRIO, com
// dois Chromium de media falsa.
//
// O que prova:
//   1. Ligar a quem está offline abre o ecrã de voz e diz «Indisponível» — não
//      finge que está a tocar.
//   2. Quem liga e desliga antes de atenderem pára o toque do outro lado.
//   3. A liga em voz a B, B atende em voz: os dois no ecrã de voz; NENHUM
//      `getUserMedia` com vídeo, NENHUM sender de vídeo com track em nenhuma
//      RTCPeerConnection, e áudio a fluir nos dois sentidos (bytes RTP de
//      áudio a subir no `getStats`, enviados e recebidos).
//   4. A passa a vídeo: a câmara é pedida só agora, há sender de vídeo com track
//      a enviar bytes NA MESMA RTCPeerConnection, e a vista passa à sala. B vê
//      «ligou a câmara · Ver vídeo», passa à sala e recebe bytes de vídeo.
//   5. SMS: o botão só existe com `can_sms`; o contador mostra GSM-7/UCS-2; o
//      envio chega ao SMSC falso com o prefixo do remetente; e as recusas
//      409 (recusou SMS) e 422 (sem número) aparecem com a mensagem certa.
//
// Não prova: som audível (a media é falsa), NAT/TURN reais, nem entrega por um
// operador real (o SMSC é falso, como no `sms-gateway.mjs`).
//
// O servidor tem de arrancar com o SMSC falso que este script abre:
//   SMS_UNITEL_SMPP='smpp://delonix:p%40ss-unitel@127.0.0.1:12793?source_addr=DELONIX'
// e o Vite a apontar para ele (API_PORT).
//
// Uso:  API=http://127.0.0.1:8293 APP=http://127.0.0.1:5393 SMSC_PORT=12793 \
//       [SHOTS=<pasta>] node web/e2e/chamada-voz-sms.mjs
import net from 'node:net'
import { mkdirSync } from 'node:fs'
import { chromium } from '@playwright/test'
import { criarConta, entrar, PASSWORD } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://127.0.0.1:5174'
const SMSC_PORT = Number(process.env.SMSC_PORT ?? 12775)
const SHOTS = process.env.SHOTS ?? ''
// O CI e um host carregado esfomeiam o ICE: as esperas por media escalam (R118).
const FATOR = Number(process.env.E2E_TIMEOUT_FACTOR) || 1
if (SHOTS) mkdirSync(SHOTS, { recursive: true })

let falhas = 0
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
const esperar = (ms) => new Promise((r) => setTimeout(r, ms))
const foto = async (page, nome) => {
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/${nome}.png` })
}

async function req(path, { token, method = 'GET', body } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}), ...(body ? { 'Content-Type': 'application/json' } : {}) },
    ...(body ? { body: JSON.stringify(body) } : {}),
  })
  let json = null
  try {
    json = await r.json()
  } catch {
    /* sem corpo */
  }
  return { status: r.status, json }
}

// ---------- SMSC falso (o mesmo protocolo mínimo do sms-gateway.mjs) ----------
const submits = []
const smsc = net.createServer((sock) => {
  let buf = Buffer.alloc(0)
  sock.on('data', (chunk) => {
    buf = Buffer.concat([buf, chunk])
    while (buf.length >= 16 && buf.length >= buf.readUInt32BE(0)) {
      const len = buf.readUInt32BE(0)
      const id = buf.readUInt32BE(4)
      const seq = buf.readUInt32BE(12)
      const body = buf.subarray(16, len)
      buf = buf.subarray(len)
      let status = 0
      let rbody = Buffer.alloc(0)
      if (id === 0x02) {
        const [, password] = body.toString('latin1').split('\0')
        status = password === 'p@ss-unitel' ? 0 : 0x0e
        rbody = Buffer.from('FAKE\0')
      } else if (id === 0x04) {
        submits.push(Buffer.from(body))
        rbody = Buffer.from(`fake-${submits.length}\0`)
      }
      const head = Buffer.alloc(16)
      head.writeUInt32BE(16 + rbody.length, 0)
      head.writeUInt32BE((id | 0x80000000) >>> 0, 4)
      head.writeUInt32BE(status, 8)
      head.writeUInt32BE(seq, 12)
      sock.write(Buffer.concat([head, rbody]))
      if (id === 0x06 || status) sock.end()
    }
  })
  sock.on('error', () => {})
})
await new Promise((r) => smsc.listen(SMSC_PORT, '127.0.0.1', r))

function submitHeader(body) {
  let i = body.indexOf(0) + 1
  i += 2
  const srcEnd = body.indexOf(0, i)
  i = srcEnd + 3
  const destEnd = body.indexOf(0, i)
  const dest = body.subarray(i, destEnd).toString('latin1')
  const dataCoding = body[destEnd + 8]
  const smLength = body[destEnd + 10]
  const sm = body.subarray(destEnd + 11, destEnd + 11 + smLength)
  return { dest, dataCoding, text: dataCoding === 0 ? sm.toString('latin1') : null }
}

// ---------- contas: A (admin) e B (membro com telefone) ----------
const contaA = await criarConta(API, 'voz')
const loginA = await req('/api/auth/login', { method: 'POST', body: contaA })
const tokenA = loginA.json?.access_token
const orgs = (await req('/api/orgs', { token: tokenA })).json ?? []
const orgId = orgs[0]?.id
const nomeA = (await req('/api/users/me', { token: tokenA })).json?.username ?? contaA.email.split('@')[0].slice(0, 30)
const dominio = contaA.email.split('@')[1]
const contaB = { email: `bento@${dominio}`, password: PASSWORD }
const nomeB = `Bento ${dominio.slice(3, 9)}`
const criada = await req(`/api/orgs/${orgId}/employees`, {
  token: tokenA, method: 'POST', body: { email: contaB.email, username: nomeB, password: PASSWORD },
})
const userB = criada.json?.user_id
const tel = await req(`/api/orgs/${orgId}/employees/${userB}/phone`, { token: tokenA, method: 'PUT', body: { phone: '923 700 800' } })
const tokenB = (await req('/api/auth/login', { method: 'POST', body: contaB })).json?.access_token
if (!tokenA || !orgId || !userB || tel.status !== 200 || !tokenB) {
  console.error('não consegui montar a org de teste', { orgId, criada, tel: tel.status })
  process.exit(1)
}
console.log(`\n=== Chamada de voz, passagem a vídeo e SMS (org ${orgId}) ===\n`)

// ---------- browsers com media falsa e instrumentação ----------
const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', '--autoplay-policy=no-user-gesture-required'],
})
const instrumentar = () => {
  window.__gum = []
  const md = navigator.mediaDevices
  const gum = md.getUserMedia.bind(md)
  md.getUserMedia = (c) => {
    window.__gum.push({ audio: !!c?.audio, video: !!c?.video })
    return gum(c)
  }
  window.__pcs = []
  const Orig = window.RTCPeerConnection
  const Wrapped = function (...args) {
    const pc = new Orig(...args)
    window.__pcs.push(pc)
    return pc
  }
  Wrapped.prototype = Orig.prototype
  Object.setPrototypeOf(Wrapped, Orig)
  window.RTCPeerConnection = Wrapped
}
async function novaPagina(conta, viewport = { width: 1440, height: 900 }) {
  const ctx = await browser.newContext({ locale: 'pt-PT', viewport, permissions: ['camera', 'microphone'] })
  await ctx.addInitScript(instrumentar)
  const page = await ctx.newPage()
  if (process.env.DEBUG) page.on('console', (m) => /\[sfu\]/.test(m.text()) && console.log(`    [${conta.email.slice(0, 5)}] ${m.text()}`))
  await entrar(page, APP, conta)
  return page
}

/** Media desta página: pedidos de câmara, senders e bytes RTP por tipo. */
const medir = (page) =>
  page.evaluate(async () => {
    const pcs = window.__pcs.filter((pc) => pc.connectionState !== 'closed')
    const out = { gumVideo: window.__gum.filter((g) => g.video).length, gum: window.__gum.length, pcs: pcs.length, videoSenders: 0, audioSenders: 0 }
    const bytes = { audioOut: 0, audioIn: 0, videoOut: 0, videoIn: 0 }
    for (const pc of pcs) {
      for (const s of pc.getSenders()) {
        if (s.track?.kind === 'video') out.videoSenders++
        if (s.track?.kind === 'audio') out.audioSenders++
      }
      const st = await pc.getStats()
      st.forEach((r) => {
        const kind = r.kind ?? r.mediaType
        if (r.type === 'outbound-rtp') bytes[kind === 'video' ? 'videoOut' : 'audioOut'] += r.bytesSent ?? 0
        if (r.type === 'inbound-rtp') bytes[kind === 'video' ? 'videoIn' : 'audioIn'] += r.bytesReceived ?? 0
      })
    }
    return { ...out, ...bytes }
  })

/**
 * Espera que um contador de bytes SUBA dentro do prazo. Medido neste host: com
 * dois participantes, o ICE do SFU cai para `disconnected` ~5–10 s depois de
 * ligar e recupera por ICE restart (acontece igual numa videochamada normal,
 * sem o ecrã de voz — ver o relatório). Uma janela fixa de 3 s media o soluço,
 * não a funcionalidade; por isso espera-se pela subida, com prazo.
 */
async function sobe(page, chave, ms = 45_000 * FATOR) {
  const inicio = (await medir(page))[chave]
  const fim = Date.now() + ms
  while (Date.now() < fim) {
    await esperar(1000)
    const m = await medir(page)
    if (m[chave] > inicio) return { ok: true, delta: m[chave] - inicio, m }
  }
  return { ok: false, delta: 0, m: await medir(page) }
}

async function abrirContacto(page, nome) {
  // Recarrega o diretório (os contactos e o `can_sms` lêem-se ao montar).
  await page.goto(`${APP}/#/directory`, { waitUntil: 'domcontentloaded' })
  await page.reload({ waitUntil: 'domcontentloaded' })
  await page.locator('.call-row__main', { hasText: nome }).first().click({ timeout: 60_000 * FATOR })
  await page.waitForSelector('[data-testid=call-stage]', { timeout: 30_000 * FATOR })
}
const fase = (page) => page.locator('[data-testid=voice-phase]').textContent({ timeout: 30_000 * FATOR }).catch(() => null)
const esperarFase = (page, re, ms = 60_000 * FATOR) =>
  page
    .waitForFunction((src) => new RegExp(src).test(document.querySelector('[data-testid=voice-phase]')?.textContent ?? ''), re.source, { timeout: ms })
    .then(() => true)
    .catch(() => false)

const paginaA = await novaPagina(contaA)

// ---------- 1. B offline: indisponível ----------
await abrirContacto(paginaA, nomeB)
await foto(paginaA, '01-contacto-voz-video-sms')
ok(await paginaA.getByRole('button', { name: `Enviar SMS a ${nomeB}` }).count() > 0, 'o contacto com telemóvel mostra o SMS na linha')
await paginaA.locator('.call-controls').getByRole('button', { name: /chamada de voz/i }).click()
await paginaA.waitForSelector('[data-testid=voice-call]', { timeout: 60_000 * FATOR })
ok(await esperarFase(paginaA, /Indisponível/), 'B offline: o ecrã de voz diz «Indisponível»', await fase(paginaA))
ok(/#\/r\/[a-z-]+\?voice$/.test(await paginaA.evaluate(() => location.hash)), 'voz abre a sala com ?voice (a mesma sessão/SFU)')
await foto(paginaA, '02-voz-indisponivel')
await paginaA.getByRole('button', { name: 'Desligar a chamada' }).click()
await paginaA.waitForSelector('.shell', { timeout: 30_000 * FATOR })

// ---------- 2. desligar antes de atenderem pára o toque ----------
const paginaB = await novaPagina(contaB)
await esperar(1500)
await abrirContacto(paginaA, nomeB)
await paginaA.locator('.call-controls').getByRole('button', { name: /chamada de voz/i }).click()
const tocou = await paginaB.waitForSelector('.call-ring', { timeout: 30_000 * FATOR }).then(() => true).catch(() => false)
ok(tocou, 'B vê o cartão de chamada a entrar')
ok(/Chamada de voz/.test((await paginaB.locator('.call-ring').first().textContent().catch(() => '')) ?? ''), 'o cartão diz que é chamada de voz')
await foto(paginaB, '03-b-toque-voz')
ok(await esperarFase(paginaA, /A chamar/), 'A vê «A chamar…» enquanto toca', await fase(paginaA))
await foto(paginaA, '04-a-a-chamar')
await paginaA.getByRole('button', { name: 'Desligar a chamada' }).click()
const parou = await paginaB.waitForSelector('.call-ring', { state: 'detached', timeout: 15_000 * FATOR }).then(() => true).catch(() => false)
ok(parou, 'A desliga antes de B atender: o toque de B pára')
await paginaA.waitForSelector('.shell', { timeout: 30_000 * FATOR })

// ---------- 3. A liga em voz, B atende em voz ----------
await abrirContacto(paginaA, nomeB)
await paginaA.locator('.call-controls').getByRole('button', { name: /chamada de voz/i }).click()
await paginaB.waitForSelector('.call-ring', { timeout: 30_000 * FATOR })
await paginaB.locator('.call-ring').getByRole('button', { name: 'Atender' }).click()
await paginaB.waitForSelector('[data-testid=voice-call]', { timeout: 60_000 * FATOR })
ok(/\?voice$/.test(await paginaB.evaluate(() => location.hash)), 'B atende em voz (sala com ?voice)')
const emA = await esperarFase(paginaA, /Em chamada/)
const emB = await esperarFase(paginaB, /Em chamada/)
ok(emA && emB, 'os dois vêem «Em chamada»', `${await fase(paginaA)} / ${await fase(paginaB)}`)
await esperar(2000)
const a1 = await medir(paginaA)
const b1 = await medir(paginaB)
await esperar(3000)
const a2 = await medir(paginaA)
const b2 = await medir(paginaB)
ok(a2.gumVideo === 0 && b2.gumVideo === 0 && a2.gum > 0 && b2.gum > 0, 'nenhum getUserMedia com vídeo (só áudio)', `A ${JSON.stringify({ gum: a2.gum, video: a2.gumVideo })} B ${JSON.stringify({ gum: b2.gum, video: b2.gumVideo })}`)
ok(a2.pcs > 0 && a2.videoSenders === 0 && b2.videoSenders === 0 && a2.audioSenders > 0 && b2.audioSenders > 0, 'nenhum sender de vídeo com track; há sender de áudio', `A ${a2.audioSenders}a/${a2.videoSenders}v B ${b2.audioSenders}a/${b2.videoSenders}v`)
ok(a2.videoOut === 0 && b2.videoOut === 0, 'zero bytes de vídeo enviados', `A ${a2.videoOut} B ${b2.videoOut}`)
ok(a2.audioOut > a1.audioOut && b2.audioOut > b1.audioOut, 'áudio a sair dos dois lados', `A +${a2.audioOut - a1.audioOut} B +${b2.audioOut - b1.audioOut}`)
ok(a2.audioIn > a1.audioIn && b2.audioIn > b1.audioIn, 'áudio a chegar aos dois lados (pelo SFU)', `A +${a2.audioIn - a1.audioIn} B +${b2.audioIn - b1.audioIn}`)
ok((await paginaA.locator('.rm-tile').count()) === 0 && (await paginaB.locator('.rm-tile').count()) === 0, 'sem grelha de vídeo no ecrã de voz')
const nomeNoEcraA = await paginaA.locator('.vc-name').textContent()
ok(nomeNoEcraA === nomeB, 'A vê o nome de B no ecrã de voz', nomeNoEcraA)
await esperarFase(paginaA, /Em chamada/, 45_000 * FATOR)
const relogio = await paginaA.locator('.vc-state').textContent()
ok(/Em chamada · \d\d:\d\d/.test(relogio ?? ''), 'o cronómetro corre', relogio)
await foto(paginaA, '05-a-em-chamada-voz')
await foto(paginaB, '06-b-em-chamada-voz')

// ---------- 4. A passa a vídeo na mesma sessão ----------
const pcsAntes = a2.pcs
await paginaA.getByRole('button', { name: 'Passar a vídeo (liga a câmara)' }).click()
const naSala = await paginaA.waitForSelector('.rm-tile', { timeout: 30_000 * FATOR }).then(() => true).catch(() => false)
ok(naSala && (await paginaA.locator('[data-testid=voice-call]').count()) === 0, 'A passa à vista de vídeo')
const saiVideo = await sobe(paginaA, 'videoOut')
const a4 = saiVideo.m
ok(a4.gumVideo >= 1, 'a câmara só é pedida agora', `${a4.gumVideo} pedido(s) com vídeo`)
ok(a4.videoSenders >= 1 && a4.pcs === pcsAntes, 'há sender de vídeo com track, na MESMA RTCPeerConnection', `${a4.videoSenders} sender(s), ${a4.pcs} PC (antes ${pcsAntes})`)
ok(saiVideo.ok, 'vídeo a sair de A', `+${saiVideo.delta} bytes`)
ok(/\?voice$/.test(await paginaA.evaluate(() => location.hash)), 'sem navegação: a sala é a mesma')
await foto(paginaA, '07-a-passou-a-video')
const chip = await paginaB.waitForSelector('.vc-chip', { timeout: 60_000 * FATOR }).then(() => true).catch(() => false)
ok(chip, 'B vê «ligou a câmara · Ver vídeo» sem sair da voz')
await foto(paginaB, '08-b-a-ligou-camara')
if (chip) {
  await paginaB.locator('.vc-chip').click()
  await paginaB.waitForSelector('.rm-tile', { timeout: 30_000 * FATOR })
  const chegaVideo = await sobe(paginaB, 'videoIn')
  ok(chegaVideo.ok && chegaVideo.m.gumVideo === 0, 'B recebe o vídeo de A e continua sem câmara', `+${chegaVideo.delta} bytes de vídeo; ${chegaVideo.m.gumVideo} pedidos de câmara`)
  const avatar = await paginaB.locator('.rm-tile .rm-tile__avatar, .rm-tile [class*=avatar]').count()
  ok(avatar > 0, 'na sala, quem não tem vídeo (B) aparece como avatar')
  await foto(paginaB, '09-b-sala-video-avatar')
}
await paginaA.goto(`${APP}/#/`)
await paginaB.goto(`${APP}/#/`)

// ---------- 5. SMS ----------
await abrirContacto(paginaA, nomeB)
await paginaA.locator('.call-controls').getByRole('button', { name: 'SMS' }).click()
await paginaA.waitForSelector('#sms-body')
await paginaA.fill('#sms-body', 'Ola Bento, liga-me quando puderes.')
const contador = await paginaA.locator('[data-testid=sms-count]').textContent()
ok(/GSM-7/.test(contador ?? '') && /1 parte de 6/.test(contador ?? ''), 'contador: texto sem acentos é GSM-7 numa parte', contador)
await paginaA.fill('#sms-body', 'Reunião às 10h, não te esqueças.')
const contadorU = await paginaA.locator('[data-testid=sms-count]').textContent()
ok(/UCS-2/.test(contadorU ?? ''), 'contador: «ã»/«ç» passam a UCS-2', contadorU)
await foto(paginaA, '10-sms-dialogo-ucs2')
await paginaA.fill('#sms-body', 'Ola Bento, liga-me quando puderes.')
const antesSms = submits.length
await paginaA.locator('[data-testid=sms-send]').click()
ok(await paginaA.waitForSelector('[data-testid=sms-sent]', { timeout: 15_000 * FATOR }).then(() => true).catch(() => false), 'o envio é aceite (202, em fila)')
await foto(paginaA, '11-sms-enviado')
let chegou = null
for (let i = 0; i < 60 && !chegou; i++) {
  chegou = submits.slice(antesSms).map(submitHeader).find((h) => h.dest === '244923700800')
  if (!chegou) await esperar(500)
}
ok(chegou?.text === `${nomeA} (Delonix Meet): Ola Bento, liga-me quando puderes.`, 'o SMS chegou ao SMSC, para o número de B, com o nome de quem envia', JSON.stringify(chegou))
await paginaA.getByRole('button', { name: 'Fechar' }).first().click()

// 409: B desliga os SMS de contactos com o diálogo de A aberto.
await paginaA.locator('.call-controls').getByRole('button', { name: 'SMS' }).click()
await paginaA.fill('#sms-body', 'Outra mensagem')
await req('/api/users/me/sms-preferences', { token: tokenB, method: 'PUT', body: { contact_opt_out: true } })
await paginaA.locator('[data-testid=sms-send]').click()
const erro409 = await paginaA.locator('[data-testid=sms-error]').textContent({ timeout: 15_000 * FATOR }).catch(() => null)
ok(/desligou os SMS/.test(erro409 ?? ''), '409 sms.recipient_opted_out aparece como «desligou os SMS»', erro409)
await foto(paginaA, '12-sms-409-recusou')
await paginaA.getByRole('button', { name: 'Cancelar' }).click()
const semBotao = await paginaA
  .waitForFunction((n) => !document.querySelector(`button[aria-label="Enviar SMS a ${n}"]`), nomeB, { timeout: 15_000 * FATOR })
  .then(() => true)
  .catch(() => false)
ok(semBotao, 'depois da recusa o diretório recarrega e o SMS deixa de aparecer para B')

// 422: B volta a aceitar; o admin apaga o número com o diálogo aberto.
await req('/api/users/me/sms-preferences', { token: tokenB, method: 'PUT', body: { contact_opt_out: false } })
await abrirContacto(paginaA, nomeB)
await paginaA.locator('.call-controls').getByRole('button', { name: 'SMS' }).click()
await paginaA.fill('#sms-body', 'Mais uma')
await req(`/api/orgs/${orgId}/employees/${userB}/phone`, { token: tokenA, method: 'PUT', body: { phone: null } })
await paginaA.locator('[data-testid=sms-send]').click()
const erro422 = await paginaA.locator('[data-testid=sms-error]').textContent({ timeout: 15_000 * FATOR }).catch(() => null)
ok(/não tem telemóvel/.test(erro422 ?? ''), '422 sms.recipient_no_phone aparece como «não tem telemóvel»', erro422)
await foto(paginaA, '13-sms-422-sem-numero')
ok(submits.length - antesSms === 1, 'as recusas não chegaram ao SMSC', `${submits.length - antesSms} submit(s)`)

// ---------- telemóvel: ecrã de voz a 390 px ----------
if (SHOTS) {
  await req(`/api/orgs/${orgId}/employees/${userB}/phone`, { token: tokenA, method: 'PUT', body: { phone: '923 700 800' } })
  const telA = await novaPagina(contaA, { width: 390, height: 844 })
  await telA.goto(`${APP}/#/directory`)
  await telA.locator('.call-row__main', { hasText: nomeB }).first().waitFor({ timeout: 60_000 * FATOR })
  await foto(telA, '14-telemovel-contactos')
  await telA.locator('.call-row', { hasText: nomeB }).getByRole('button', { name: `Ligar por voz a ${nomeB}` }).click()
  await paginaB.waitForSelector('.call-ring', { timeout: 30_000 * FATOR })
  await paginaB.locator('.call-ring').getByRole('button', { name: 'Atender' }).click()
  await esperarFase(telA, /Em chamada/)
  await esperar(2500)
  await foto(telA, '15-telemovel-em-chamada-voz')
  await telA.getByRole('button', { name: 'Desligar a chamada' }).click()
}

await browser.close()
smsc.close()
console.log(`\n${falhas === 0 ? 'tudo verde' : `${falhas} falha(s)`}\n`)
process.exit(falhas ? 1 : 0)
