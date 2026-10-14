// Prova de SALA contra uma instalação a SERVIR (produção ou stage): um convidado
// SEM CONTA entra por um link que alguém lhe dá, e mede do seu lado o que prova
// que há media — não que o serviço «está de pé».
//
// Existe por causa de 2026-10-13. Nesse dia a sala foi provada com uma sonda
// escrita num ramo temporário, que depois se apagou; e o que ela media (que a
// 3478 responde) não era media. Resultado: a sala «funcionava» porque quem a
// provava estava na LAN, e fora dela as pessoas entravam e não ouviam ninguém,
// sem um único erro. Esta sonda mede três coisas que essa não media:
//
//   1. o par ICE escolhido, com o ENDEREÇO do candidato remoto — é aí que se vê
//      se o TURN que o browser recebeu é o público ou um 192.168;
//   2. os bytes de áudio E de vídeo a crescer nos DOIS sentidos;
//   3. um retrato REMOTO (`.rm-tile[data-peer=remoto]`), e não o próprio.
//
// O que ela NÃO faz, e é de propósito: não cria contas nem entra com password
// numa instalação que não é local. Precisa de alguém DENTRO da sala para a
// admitir — a sala de espera é obrigatória para convidados, por desenho.
//
// Uso (o anfitrião tem de estar na sala e admitir):
//   cd web && node e2e/producao-sala.mjs 'https://meet.exemplo.ao/#/r/abc-defg-hij' 'Sonda'
// Espera até `WAIT_SECS` (180 por omissão) pela admissão.
import { createRequire } from 'node:module'
const require = createRequire(import.meta.url)
const { chromium } = require('playwright')

const url = process.argv[2]
const name = process.argv[3] || 'Sonda'
const waitSecs = Number(process.env.WAIT_SECS || 180)
if (!url) {
  console.error('uso: node e2e/producao-sala.mjs <url-da-sala> [nome]')
  process.exit(2)
}
let failed = 0
const ok = (cond, msg, extra = '') => {
  if (!cond) failed++
  console.log(`  ${cond ? '✓' : '✗'} ${msg}${extra ? '  ' + extra : ''}`)
}

const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
})
// `locale` fixo: sem ele o Chromium arranca em inglês e os nomes dos botões não
// casam (custou duas corridas a 2026-10-13).
const context = await browser.newContext({ permissions: ['camera', 'microphone'], locale: 'pt-PT' })
// Guardar cada RTCPeerConnection para lhe pedir `getStats()`, sem depender de a
// aplicação as expor numa variável global.
await context.addInitScript(() => {
  const Native = window.RTCPeerConnection
  window.__pcs = []
  window.RTCPeerConnection = function (...args) {
    const pc = new Native(...args)
    window.__pcs.push(pc)
    return pc
  }
  window.RTCPeerConnection.prototype = Native.prototype
})
const page = await context.newPage()
const consoleErrors = []
page.on('console', (m) => {
  // Os 401 de quem não tem sessão são o normal no ecrã de convidado.
  if (m.type() === 'error' && !/status of 401/.test(m.text())) consoleErrors.push(m.text().slice(0, 160))
})

console.log(`\n### ${url}`)
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60000 })
ok((await page.locator('#convidado-nome').count()) === 1, 'o link abre a entrada de convidado, sem sessão')
await page.fill('#convidado-nome', name)
await page.locator('[data-testid=convidado-entrar]').click()
await page.getByRole('button', { name: /entrar na sessão|join session/i }).first().click({ timeout: 60000 })
console.log(`  · pedido enviado como «${name}» — à espera de admissão (até ${waitSecs}s)`)

const probe = () =>
  page.evaluate(async () => {
    const out = { pairs: [], audioIn: 0, videoIn: 0, audioOut: 0, videoOut: 0, tiles: 0, remote: null, pcs: '' }
    for (const pc of window.__pcs) {
      if (pc.connectionState === 'closed') continue
      const report = await pc.getStats()
      let selected = null
      report.forEach((s) => {
        if (s.type === 'transport' && s.selectedCandidatePairId) selected = s.selectedCandidatePairId
      })
      report.forEach((s) => {
        if (s.type === 'candidate-pair' && (s.id === selected || s.selected || (s.nominated && s.state === 'succeeded'))) {
          const local = report.get(s.localCandidateId)
          const remote = report.get(s.remoteCandidateId)
          if (local && remote) out.pairs.push(`${local.candidateType}→${remote.candidateType} ${remote.address}:${remote.port}`)
        }
        if (s.type === 'inbound-rtp') {
          if (s.kind === 'audio') out.audioIn += s.bytesReceived || 0
          if (s.kind === 'video') out.videoIn += s.bytesReceived || 0
        }
        if (s.type === 'outbound-rtp') {
          if (s.kind === 'audio') out.audioOut += s.bytesSent || 0
          if (s.kind === 'video') out.videoOut += s.bytesSent || 0
        }
      })
    }
    const tiles = [...document.querySelectorAll('.rm-tile')]
    const remotes = tiles.filter((t) => t.getAttribute('data-peer') === 'remoto')
    out.tiles = tiles.length
    out.remote = remotes.length
      ? remotes
          .map((t) => {
            const v = t.querySelector('video')
            return `${(t.textContent || '').trim().slice(0, 24)} ${v ? `${v.videoWidth}x${v.videoHeight} t=${v.currentTime.toFixed(1)}s` : 'sem <video>'}`
          })
          .join(' | ')
      : null
    out.pcs = window.__pcs.map((pc) => `${pc.connectionState}/${pc.iceConnectionState}`).join(',') || '(nenhuma)'
    return out
  })

// Admitido = há um retrato REMOTO. O próprio retrato não conta: contá-lo deu um
// falso «admitido» imediato na primeira versão desta sonda.
let admitted = false
for (let i = 0; i < Math.ceil(waitSecs / 3); i++) {
  const s = await probe()
  if (s.remote) {
    admitted = true
    break
  }
  if (i % 10 === 0) console.log(`    … ${i * 3}s: retratos=${s.tiles} remoto=nenhum PCs=${s.pcs}`)
  await page.waitForTimeout(3000)
}
ok(admitted, admitted ? 'admitido na sala' : `ninguém admitiu em ${waitSecs}s — sem anfitrião DENTRO da sala esta prova não corre`)

if (admitted) {
  const before = await probe()
  await page.waitForTimeout(6000)
  const after = await probe()
  const pairs = [...new Set(after.pairs)].join(' ; ') || '(nenhum par seleccionado)'
  console.log(`  · par ICE: ${pairs}`)
  console.log(`  · recebido: áudio ${before.audioIn}→${after.audioIn} B, vídeo ${before.videoIn}→${after.videoIn} B`)
  console.log(`  · enviado:  áudio ${before.audioOut}→${after.audioOut} B, vídeo ${before.videoOut}→${after.videoOut} B`)
  console.log(`  · retratos: ${after.tiles} (remoto: ${after.remote})   PCs: ${after.pcs}`)
  ok(after.audioIn > before.audioIn && after.videoIn > before.videoIn, 'recebe áudio E vídeo, a crescer em 6 s')
  ok(after.audioOut > before.audioOut && after.videoOut > before.videoOut, 'envia áudio E vídeo, a crescer em 6 s')
  ok(/relay→relay/.test(pairs), 'o par escolhido é relay→relay (a media atravessa o TURN, como `FORCE_TURN_RELAY=1` manda)')
  console.log(`  · nota: confirma que o endereço acima é o PÚBLICO do relay. Um 10./172.16./192.168. aqui é a armadilha de 2026-10-13 — a sala abre e, de fora da LAN, ninguém ouve ninguém.`)
}
if (consoleErrors.length) console.log(`  · erros de consola: ${consoleErrors.slice(0, 3).join(' | ')}`)
await browser.close()
process.exit(failed ? 1 : 0)
