#!/usr/bin/env node
// Verificação do PWA offline num Chromium a sério, com a rede CORTADA de
// verdade (`context.setOffline`), não simulada por um esboço de `fetch`.
//
// Uso:  BASE=http://127.0.0.1:4200 node e2e/offline.mjs
import { chromium } from 'playwright'

const BASE = process.env.BASE ?? 'http://127.0.0.1:4200'
let falhas = 0
const ok = (n, c, d = '') => { console.log(`${c ? '  ok  ' : ' FALHA'}  ${n}${d ? `  — ${d}` : ''}`); if (!c) falhas++ }

const browser = await chromium.launch()
const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 } })
const page = await ctx.newPage()

console.log('\ncom rede')
await page.goto(BASE)
await page.waitForFunction(() => navigator.serviceWorker?.controller != null, null, { timeout: 20000 })
  .then(() => ok('o service worker regista-se e assume o controlo', true))
  .catch(() => ok('o service worker regista-se e assume o controlo', false, 'não assumiu em 20 s'))

const precache = await page.evaluate(async () => {
  const nomes = await caches.keys()
  const principal = nomes.find((n) => n.startsWith('delonix-') && !n.includes('pesados'))
  if (!principal) return null
  const c = await caches.open(principal)
  return { nome: principal, chaves: (await c.keys()).map((r) => new URL(r.url).pathname) }
})
ok('há uma cache com o esqueleto', !!precache, precache?.nome ?? 'nenhuma')
ok('o index.html está lá', !!precache?.chaves.includes('/index.html'), (precache?.chaves.length ?? 0) + ' entradas')
ok('e os assets de arranque também',
   !!precache?.chaves.some((k) => /^\/assets\/index-.*\.js$/.test(k)) &&
   !!precache?.chaves.some((k) => /^\/assets\/index-.*\.css$/.test(k)),
   precache?.chaves.filter((k) => k.startsWith('/assets')).join(' ') ?? '')

console.log('\nsem rede')
await ctx.setOffline(true)
// Página NOVA: é o teste a sério — não basta a que já está carregada continuar
// a funcionar, tem de ABRIR sem rede nenhuma.
const p2 = await ctx.newPage()
const erros = []
p2.on('pageerror', (e) => erros.push(e.message.slice(0, 120)))
const resposta = await p2.goto(BASE).catch(() => null)
ok('a app abre com a rede cortada', !!resposta, resposta ? `HTTP ${resposta.status()}` : 'não abriu')
await p2.waitForSelector('#root > *', { timeout: 15000 })
  .then(() => ok('e monta a interface', true))
  .catch(() => ok('e monta a interface', false, 'root vazio'))

const estado = await p2.evaluate(() => ({
  raiz: document.querySelector('#root')?.firstElementChild?.className ?? '(vazio)',
  titulo: document.title,
}))
ok('a app renderiza um ecrã real', estado.raiz !== '(vazio)', estado.raiz)
// NOTA: não se verifica `navigator.onLine` aqui. O `setOffline` do Playwright
// corta a rede ao nível do CDP mas não vira essa flag — verificá-la testaria o
// Playwright, não a app. O que interessa é a app REAGIR ao evento `offline`,
// que é o que o browser dispara quando a placa de rede cai. É o teste a seguir.

console.log('\na interface reage')
{
  const p3 = await ctx.newPage()
  const falhasP3 = []
  p3.on('pageerror', (e) => falhasP3.push('PAGEERROR ' + e.message.slice(0, 160)))
  p3.on('console', (m) => m.type() === 'error' && falhasP3.push('CONSOLE ' + m.text().slice(0, 160)))
  p3.on('requestfailed', (r) => falhasP3.push(`FALHOU ${new URL(r.url()).pathname} ${r.failure()?.errorText ?? ''}`))
  await p3.goto(BASE)
  await p3.evaluate(() => {
    localStorage.setItem('dx_user', JSON.stringify({ id: 1, username: 'walter', email: 'w@delonix.local', locale: 'pt' }))
    localStorage.setItem('dx_access', 'tok')
    localStorage.setItem('dx_tour_v1', 'done')
  })
  // `goto` só com hash diferente é navegação no MESMO documento: o main.tsx
  // não volta a correr e a sessão acabada de escrever não é lida. Daí o reload.
  await p3.goto(`${BASE}/#/studio`)
  await p3.reload()
  const chegou = await p3
    .waitForSelector('.studio-canvas', { timeout: 20000 })
    .then(() => true)
    .catch(() => false)
  if (!chegou) {
    const diag = await p3.evaluate(() => ({
      raiz: document.querySelector('#root')?.firstElementChild?.className ?? '(vazio)',
      hash: location.hash,
      user: !!localStorage.getItem('dx_user'),
    }))
    ok('o Estúdio abre sem rede', false, JSON.stringify(diag))
    console.log('    diagnóstico:'); falhasP3.slice(0, 8).forEach((f) => console.log('      ' + f))
  } else {
    ok('o Estúdio abre sem rede', true)
  }

  const antes = await p3.locator('.studio-offline').count()
  await p3.evaluate(() => window.dispatchEvent(new Event('offline')))
  await p3.waitForSelector('.studio-offline', { timeout: 5000 })
    .then(() => ok('o aviso de «sem rede» aparece ao cair a ligação', true, `antes havia ${antes}`))
    .catch(() => ok('o aviso de «sem rede» aparece ao cair a ligação', false, 'não apareceu'))

  const texto = await p3.locator('.studio-offline').textContent()
  // A promessa tem de ser a certa: grava e guarda no dispositivo. Se um dia
  // alguém trocar isto por «funcionalidade indisponível», o teste avisa.
  ok('e diz que se pode gravar na mesma', /grava/i.test(texto ?? ''), (texto ?? '').slice(0, 60) + '…')

  await p3.evaluate(() => window.dispatchEvent(new Event('online')))
  await p3.waitForSelector('.studio-offline', { state: 'detached', timeout: 5000 })
    .then(() => ok('e desaparece quando a ligação volta', true))
    .catch(() => ok('e desaparece quando a ligação volta', false, 'ficou visível'))
}
ok('sem erros de página', erros.length === 0, erros[0] ?? 'nenhum')

// A fronteira honesta: /api NÃO pode vir de cache.
const api = await p2.evaluate(async () => {
  try { const r = await fetch('/api/me'); return { ok: r.ok, status: r.status } }
  catch (e) { return { ok: false, erro: String(e).slice(0, 60) } }
})
ok('as chamadas à API falham em vez de servirem dados velhos', !api.ok, JSON.stringify(api))

console.log('\narquivo local')
const arq = await p2.evaluate(async () => {
  const bd = await new Promise((res, rej) => {
    const p = indexedDB.open('delonix-estudio', 1)
    p.onupgradeneeded = () => { const b = p.result; if (!b.objectStoreNames.contains('aulas')) b.createObjectStore('aulas', { keyPath: 'id' }) }
    p.onsuccess = () => res(p.result); p.onerror = () => rej(p.error)
  })
  const blob = new Blob([new Uint8Array(1024)], { type: 'video/webm' })
  await new Promise((res, rej) => {
    const t = bd.transaction('aulas', 'readwrite')
    t.objectStore('aulas').put({ id: 'x', titulo: 'Aula offline', criadaEm: Date.now(), duracao: 10, completo: blob, audio: null, enviada: false })
    t.oncomplete = () => res(); t.onerror = () => rej(t.error)
  })
  const lido = await new Promise((res, rej) => {
    const t = bd.transaction('aulas', 'readonly'); const r = t.objectStore('aulas').get('x')
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error)
  })
  bd.close()
  return { tem: !!lido, tamanho: lido?.completo?.size ?? 0, tipo: lido?.completo?.type ?? '' }
})
ok('o IndexedDB guarda um Blob de vídeo sem rede', arq.tem && arq.tamanho === 1024, JSON.stringify(arq))

await browser.close()
console.log(falhas === 0 ? '\nTUDO VERDE\n' : `\n${falhas} FALHA(S)\n`)
process.exit(falhas === 0 ? 0 : 1)
