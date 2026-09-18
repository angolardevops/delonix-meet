#!/usr/bin/env node
// O DIRECTO com vários destinos, contra um servidor RTMP A SÉRIO.
//
// Prova o que o `directo.mjs` não consegue (ele não tem para onde emitir):
//
//   1. dois destinos na mesma emissão — um GUARDADO pela organização e válido
//      (referido por `{id}`, chave decifrada no servidor), outro INVÁLIDO
//      (porta fechada) —, e o válido fica NO AR enquanto o inválido cai com
//      motivo e reinicia com backoff (a DESISTÊNCIA leva ~2 min com a política
//      de produção e está provada nos testes do `broadcast.rs`, não aqui);
//   2. a media chega mesmo ao servidor RTMP (ffprobe lê H.264 + AAC de lá);
//   3. o estado por destino chega pelo WebSocket, com débito;
//   4. se o servidor RTMP reiniciar a meio, o destino válido cai e VOLTA ao ar
//      sozinho — o que obriga o ffmpeg novo a reentrar a meio do fluxo com o
//      cabeçalho Matroska guardado (só com RTMP_CONTENTOR);
//   5. os webhooks `stream.published` e `stream.ended` saem (só com o servidor
//      arrancado com `WEBHOOK_ALLOW_HOSTS=localhost`);
//   6. a chave de emissão nunca aparece nas mensagens do WebSocket.
//
// A media é gerada aqui por um ffmpeg local em Matroska ao vivo (H.264 + Opus),
// que é o que o MediaRecorder do browser produz — o browser não é o que se mede.
//
// Uso (ver `scripts/e2e-fora-do-ci.txt` para o porquê de não correr no CI):
//   docker run -d --name mtx -p 127.0.0.1:19350:1935 bluenviron/mediamtx
//   API=http://127.0.0.1:8203 RTMP=rtmp://127.0.0.1:19350 RTMP_CONTENTOR=mtx \
//     node web/e2e/directo-destinos.mjs          # e outra vez com SEM_CRC=1
import { spawn } from 'node:child_process'
import { createServer } from 'node:http'
import WebSocket from 'ws'
import { criarConta } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8203'
const WS = API.replace(/^http/, 'ws')
const RTMP = process.env.RTMP ?? 'rtmp://127.0.0.1:19350'
const CONTENTOR = process.env.RTMP_CONTENTOR ?? ''
const PORTA_HOOK = Number(process.env.PORTA_HOOK ?? 18999)

let falhas = 0
const ok = (n, c, d = '') => {
  console.log(`${c ? '  ok  ' : ' FALHA'}  ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
const dormir = (ms) => new Promise((r) => setTimeout(r, ms))

async function req(path, { token, method = 'GET', body } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: {
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(body ? { 'Content-Type': 'application/json' } : {}),
    },
    ...(body ? { body: JSON.stringify(body) } : {}),
  })
  const texto = await r.text()
  let json = null
  try {
    json = JSON.parse(texto)
  } catch {
    /* sem corpo */
  }
  return { status: r.status, json }
}

// ---------------------------------------------------------------- preparação
const conta = await criarConta(API, 'dst')
const login = await req('/api/auth/login', { method: 'POST', body: conta })
const token = login.json?.access_token
const orgId = (await req('/api/orgs', { token })).json?.[0]?.id
if (!token || !orgId) throw new Error(`sem sessão: ${JSON.stringify(login.json)}`)

// Receptor de webhooks. Só recebe se o servidor tiver `WEBHOOK_ALLOW_HOSTS=localhost`.
const hooks = []
const receptor = createServer((q, s) => {
  let corpo = ''
  q.on('data', (c) => (corpo += c))
  q.on('end', () => {
    try {
      hooks.push(JSON.parse(corpo))
    } catch {
      /* ignora */
    }
    s.end('ok')
  })
}).listen(PORTA_HOOK, '127.0.0.1')
const hook = await req(`/api/orgs/${orgId}/webhooks`, {
  token,
  method: 'POST',
  body: { kind: 'generic', url: `http://localhost:${PORTA_HOOK}/hook`, events: 'stream.published,stream.ended' },
})
const comWebhooks = hook.status === 200
if (!comWebhooks) console.log(`  (webhooks por provar: o servidor recusou o receptor local — ${hook.status})`)

const CHAVE = `b1emissao${Date.now().toString(36)}`
const guardado = await req(`/api/orgs/${orgId}/stream-destinations`, {
  token,
  method: 'POST',
  body: { label: 'RTMP local', rtmp_url: `${RTMP}/live`, stream_key: CHAVE },
})
ok('o destino válido fica guardado (201) sem a chave na resposta',
   guardado.status === 201 && !JSON.stringify(guardado.json).includes(CHAVE), `${guardado.status}`)

const sala = (await req('/api/rooms', { token, method: 'POST', body: { name: 'directo', topology: 'sfu' } })).json
const join = await req(`/api/rooms/${sala.code}/join`, { token, method: 'POST' })
const destinos = [
  { id: guardado.json.id },
  { url: 'rtmp://127.0.0.1:1/live', chave: 'chave-do-invalido', rotulo: 'Inválido' },
]

// ------------------------------------------------------------------ emissão
const q = new URLSearchParams({ token: join.json.room_token, destinos: JSON.stringify(destinos), codec: 'video/h264' })
const ws = new WebSocket(`${WS}/api/rooms/${sala.code}/live?${q}`)
const historico = [] // [ms, estadoValido, estadoInvalido, kbpsValido]
let ultimo = null
let erroDoServidor = null
let tramasComChave = 0
const t0 = Date.now()
ws.on('message', (d, binario) => {
  if (binario) return
  const texto = d.toString()
  if (texto.includes(CHAVE) || texto.includes('chave-do-invalido')) tramasComChave++
  const m = JSON.parse(texto)
  if (m.erro) erroDoServidor = m.erro
  if (m.tipo === 'destinos') {
    ultimo = m.destinos
    historico.push([Date.now() - t0, m.destinos[0]?.estado, m.destinos[1]?.estado, m.destinos[0]?.kbps])
  }
})
await new Promise((r, j) => {
  ws.once('open', r)
  ws.once('error', j)
})

// A media: Matroska ao vivo, H.264 + Opus, em pedaços — como o MediaRecorder.
const gerador = spawn('ffmpeg', [
  '-hide_banner', '-loglevel', 'error', '-re',
  '-f', 'lavfi', '-i', 'testsrc2=size=640x360:rate=30',
  '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000',
  '-c:v', 'libx264', '-preset', 'ultrafast', '-tune', 'zerolatency', '-g', '30', '-pix_fmt', 'yuv420p',
  '-b:v', '1500k', '-c:a', 'libopus', '-b:a', '96k',
  '-f', 'matroska', '-live', '1', '-cluster_time_limit', '1000',
  // SEM_CRC=1: Clusters sem CRC-32, como os do MediaRecorder do Chromium. Por
  // omissão o ffmpeg escreve-o, e os dois formatos têm de reentrar.
  ...(process.env.SEM_CRC === '1' ? ['-write_crc32', '0'] : []),
  'pipe:1',
], { stdio: ['ignore', 'pipe', 'inherit'] })
let enviados = 0
gerador.stdout.on('data', (c) => {
  if (ws.readyState === WebSocket.OPEN) {
    ws.send(c)
    enviados += c.length
  }
})

const esperarPor = async (o_que, cond, ms) => {
  const fim = Date.now() + ms
  while (Date.now() < fim) {
    if (ultimo && cond(ultimo)) return true
    if (erroDoServidor) break
    await dormir(200)
  }
  console.log(`      (à espera de ${o_que}; último estado: ${JSON.stringify(ultimo)}; erro: ${erroDoServidor})`)
  return false
}

console.log('\ndois destinos, um inválido')
ok('o destino válido fica NO AR', await esperarPor('válido no ar', (d) => d[0]?.estado === 'no-ar' && d[0]?.kbps > 0, 20000))
ok('o inválido cai com motivo, sem a chave',
   await esperarPor('inválido a cair', (d) => ['erro', 'parado'].includes(d[1]?.estado) && /recusou/.test(d[1]?.motivo ?? ''), 20000),
   ultimo?.[1]?.motivo ?? '')
// Deixa correr: o inválido reinicia com backoff (1, 2, 4… s) e desiste à 9.ª queda.
await dormir(12000)
const desdeNoAr = historico.findIndex((h) => h[1] === 'no-ar')
const sempreNoAr = desdeNoAr >= 0 && historico.slice(desdeNoAr).every((h) => h[1] === 'no-ar')
ok('e o válido NUNCA saiu do ar enquanto o inválido caía e reiniciava', sempreNoAr,
   `${historico.length} estados; inválido passou por ${[...new Set(historico.map((h) => h[2]))].join('/')}`)
ok('o inválido tentou mais do que uma vez (backoff, não desistência imediata)', (ultimo?.[1]?.tentativas ?? 0) >= 2,
   `tentativas=${ultimo?.[1]?.tentativas}`)

console.log('\na media chega mesmo ao servidor RTMP')
const probe = await new Promise((resolve) => {
  const p = spawn('ffprobe', ['-v', 'error', '-rw_timeout', '8000000', '-show_entries', 'stream=codec_name',
    '-of', 'csv=p=0', `${RTMP}/live/${CHAVE}`])
  let out = ''
  p.stdout.on('data', (c) => (out += c))
  p.on('close', () => resolve(out.trim().split('\n').sort().join(',')))
  setTimeout(() => p.kill(), 15000)
})
ok('ffprobe lê H.264 + AAC do RTMP', probe === 'aac,h264', probe || '(nada)')

if (CONTENTOR) {
  console.log('\no servidor RTMP reinicia a meio')
  const antes = ultimo?.[0]?.tentativas ?? 0
  await new Promise((r) => spawn('docker', ['restart', '-t', '0', CONTENTOR], { stdio: 'ignore' }).on('close', r))
  ok('o válido dá pela queda', await esperarPor('a queda', (d) => d[0]?.tentativas > antes, 30000),
     ultimo?.[0]?.motivo ?? '')
  ok('e VOLTA ao ar sozinho (reentrada a meio do fluxo)',
     await esperarPor('o regresso', (d) => d[0]?.tentativas > antes && d[0]?.estado === 'no-ar' && d[0]?.kbps > 0, 40000))
  await dormir(3000)
  const probe2 = await new Promise((resolve) => {
    const p = spawn('ffprobe', ['-v', 'error', '-rw_timeout', '8000000', '-show_entries', 'stream=codec_name',
      '-of', 'csv=p=0', `${RTMP}/live/${CHAVE}`])
    let out = ''
    p.stdout.on('data', (c) => (out += c))
    p.on('close', () => resolve(out.trim().split('\n').sort().join(',')))
    setTimeout(() => p.kill(), 15000)
  })
  ok('ffprobe volta a ler H.264 + AAC depois do reinício', probe2 === 'aac,h264', probe2 || '(nada)')
} else {
  console.log('\n  (reinício do RTMP por provar: define RTMP_CONTENTOR)')
}

console.log('\nfim')
ok('nenhuma trama do WebSocket levou uma chave', tramasComChave === 0, `${tramasComChave}`)
gerador.kill('SIGINT')
ws.close(1000, 'fim')
await dormir(4000)
const depois = await req(`/api/orgs/${orgId}/stream-destinations/${guardado.json.id}`, { token })
ok('o destino guardado fica com last_status=ok e last_used_at', depois.json?.last_status === 'ok' && !!depois.json?.last_used_at,
   JSON.stringify({ s: depois.json?.last_status, e: depois.json?.last_error }))
if (comWebhooks) {
  const nomes = hooks.map((h) => h.event)
  ok('webhook stream.published saiu', nomes.includes('stream.published'), nomes.join(','))
  ok('webhook stream.ended saiu, com duração', hooks.some((h) => h.event === 'stream.ended' && h.data?.duration_secs > 0))
  ok('nenhum webhook leva URL nem chave', !JSON.stringify(hooks).includes(CHAVE) && !JSON.stringify(hooks).includes('rtmp://'))
}
console.log(`\n  bytes enviados pelo «browser»: ${(enviados / 1e6).toFixed(1)} MB; estados recebidos: ${historico.length}`)
receptor.close()
console.log(falhas === 0 ? '\nTUDO VERDE\n' : `\n${falhas} FALHA(S)\n`)
process.exit(falhas === 0 ? 0 : 1)
