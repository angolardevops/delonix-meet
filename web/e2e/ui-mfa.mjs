#!/usr/bin/env node
// MFA visto pelo UTILIZADOR — inscrição, QR, códigos de recuperação e login em
// duas fases, conduzidos num Chromium a sério contra o servidor a sério.
//
// Existe porque o backend estar certo não faz um produto. A doutrina da casa é
// explícita: uma capacidade que só existe na API não conta. Este teste é o que
// separa as duas coisas.
//
// O código TOTP é gerado aqui em JavaScript — a mesma segunda implementação
// independente do RFC 6238 que o `mfa.mjs` usa.
//
// Uso (com o servidor em :8180 e o vite em :5174):
//   APP=http://localhost:5174 node web/e2e/ui-mfa.mjs

import { chromium } from '@playwright/test'
import crypto from 'node:crypto'
const API='http://127.0.0.1:8180', APP=process.env.APP||'http://localhost:5174', PW='UmaPasswordForte123!'
const ALF='ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'
const b32=(s)=>{let b=0,n=0;const o=[];for(const c of s.replace(/[=\s]/g,'').toUpperCase()){const v=ALF.indexOf(c);b=(b<<5)|v;n+=5;if(n>=8){n-=8;o.push((b>>n)&0xff)}}return Buffer.from(o)}
const totp=(s,t=Math.floor(Date.now()/1000))=>{const c=Buffer.alloc(8);c.writeBigUInt64BE(BigInt(Math.floor(t/30)));const m=crypto.createHmac('sha1',b32(s)).update(c).digest();const f=m[19]&15;return String((((m[f]&127)<<24)|(m[f+1]<<16)|(m[f+2]<<8)|m[f+3])%1e6).padStart(6,'0')}
const marca=Math.random().toString(36).slice(2,7)
const email=`ui${marca}@ui${marca}.local`
await fetch(`${API}/api/auth/register`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({org_name:`UI ${marca}`,email,username:`ui${marca}`,password:PW})})
const b=await chromium.launch()
const p=await (await b.newContext()).newPage()
p.on('pageerror',e=>console.log('ERRO DE PÁGINA:',e.message))
let falhas=0
const chk=(c,n)=>{console.log(`  ${c?'✓':'✗'} ${n}`); if(!c) falhas++}

await p.goto(`${APP}/#/login`, { waitUntil: 'domcontentloaded' })
// Dispensa o tour de introdução: aparece para um utilizador novo e o overlay
// `.tour-dim` intercepta os cliques todos. Um utilizador fecha-o; o teste
// marca-o como visto, que dá no mesmo e não depende do desenho do tour.
await p.evaluate(() => localStorage.setItem('dx_tour_v1', 'done'))
await p.waitForTimeout(3000)
await p.fill('input[type=email]', email)
await p.fill('input[type=password]', PW)
// Espera a verificação de SSO assentar: ela dispara 500 ms depois do email e
// re-renderiza o formulário, o que destaca o botão a meio do clique.
await p.waitForTimeout(2000)
await p.locator('form button.primary').first().click()
await p.waitForTimeout(3000)
chk(await p.locator('input[type=email]').count()===0, 'entrou com password (sem MFA)')

await p.evaluate(()=>{ const b=[...document.querySelectorAll('button')].find(x=>/definiç|settings/i.test((x.getAttribute('aria-label')||'')+(x.title||'')+(x.textContent||''))); b?.click() })
await p.waitForTimeout(800)
chk(await p.locator('.settings-drawer').count()>0, 'gaveta de definições abre')
await p.locator('.settings-tab', { hasText: 'Segurança' }).click()
await p.waitForTimeout(500)
chk(await p.locator('.mfa-panel').count()>0, 'separador Segurança mostra o painel de MFA')

await p.locator('.mfa-panel button', { hasText: 'Activar' }).first().click()
await p.waitForSelector('.mfa-qr svg', { timeout: 25000 })
chk(true, 'código QR renderiza (SVG)')
const segredo=(await p.locator('.mfa-secret').textContent()).replace(/\s/g,'')
await p.screenshot({ path: '/tmp/mfa-qr.png' })
chk(segredo.length>=32, `chave legível para introdução manual (${segredo.length} chars)`)

await p.fill('.mfa-panel input[inputmode=numeric]', totp(segredo))
await p.locator('.mfa-panel button', { hasText: 'Activar' }).click()
await p.waitForSelector('.mfa-codes', { timeout: 25000 })
await p.screenshot({ path: '/tmp/mfa-codes.png' })
chk(await p.locator('.mfa-codes li').count()===10, 'mostra 10 códigos de recuperação')
chk(await p.locator('.mfa-panel button:has-text("Concluir")').isDisabled(), 'Concluir BLOQUEADO até confirmar que os guardou')
await p.locator('.mfa-confirm input').check()
chk(!await p.locator('.mfa-panel button:has-text("Concluir")').isDisabled(), 'confirmar desbloqueia Concluir')
await p.locator('.mfa-panel button', { hasText: 'Concluir' }).click()
await p.waitForTimeout(800)
chk(await p.locator('.mfa-on').count()>0, 'painel passa a mostrar MFA activa')

await p.evaluate(()=>{ localStorage.clear(); localStorage.setItem('dx_tour_v1','done') })
await p.goto(`${APP}/#/login`, { waitUntil: 'domcontentloaded' }); await p.reload(); await p.waitForTimeout(1500)
await p.fill('input[type=email]', email)
await p.fill('input[type=password]', PW)
// Espera a verificação de SSO assentar: ela dispara 500 ms depois do email e
// re-renderiza o formulário, o que destaca o botão a meio do clique.
await p.waitForTimeout(2000)
await p.locator('form button.primary').first().click()
await p.waitForSelector('.auth-mfa', { timeout: 25000 })
chk(true, 'login pede o CÓDIGO em vez de entrar')
chk(await p.locator('input[type=email]').count()===0, 'o formulário de password desaparece (já foi aceite)')
await p.screenshot({ path: '/tmp/mfa-login.png' })

const espera=(30-(Math.floor(Date.now()/1000)%30))*1000+1500
console.log(`  · aguarda ${Math.round(espera/1000)}s pela janela TOTP seguinte`)
await p.waitForTimeout(espera)
await p.fill('.mfa-code-input', totp(segredo))
await p.locator('.auth-mfa button[type=submit]').first().click()
await p.waitForTimeout(3000)
chk(await p.locator('.auth-mfa').count()===0, 'código correcto → entra')
await p.screenshot({ path: '/tmp/mfa-final.png' })
console.log(`\n=== ${falhas===0?'TODAS PASSARAM':falhas+' FALHARAM'} ===`)
await b.close()
process.exit(falhas?1:0)
