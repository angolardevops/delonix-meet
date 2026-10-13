// A cadeia de voz da chamada, MEDIDA: a nova contra a anterior, sobre o mesmo sinal.
//
// Existe porque a cadeia anterior (RNNoise → gate → nivelador → limitador,
// 94f74208) entrou sem nunca ter sido ouvida numa chamada, e em produção
// ouvia-se «um eco, um pouco agudo». Este arnês NÃO OUVE — mede, e foi ele que
// escolheu os parâmetros de `src/voiceChain.ts`.
//
// COMO. Chromium headless, `OfflineAudioContext` a 48 kHz, com os worklets
// REAIS servidos pelo Vite do próprio web/: o WASM do RNNoise e o
// `noiseGateWorklet.js`. A cadeia NOVA é a do produto (`buildVoiceChain`); a
// ANTIGA é a de e55a5f87 reconstruída com o gate congelado
// (`voz-natural.entry.ts`). Um só RNNoise alimenta as duas, na mesma
// renderização — o que as distingue é só o que vem depois dele.
//
// O SINAL (42 s, o mesmo em todas as corridas): fala real, rajadas brancas de
// 3–15 ms a −35…−15 dBFS, teclas com corpo (sinusóides amortecidas) a
// −35…−10 dBFS, batidas na mesa, os transitórios gravados que o sistema tiver
// (obturador, clique, ficha), a mesma fala a −30 dB como eco residual — sozinha,
// e 200 ms atrás da própria fala —, fala baixa (−15 dB) e ruído rosa a −60 dBFS.
// A fala vem das frases de teste do alsa-utils (/usr/share/sounds/alsa, voz
// humana gravada); sem elas o sinal usa um substituto SINTÉTICO, avisa, e os
// limiares que dependem da voz não se aplicam.
//
// O QUE MEDIU (2026-10-13, fala do alsa-utils; antiga → nova):
//   (a) teclas com corpo, pico da pior:      −12,4 → −38,6 dBFS (entrada −10)
//       rajadas brancas, pico:               −119  → −61,3 dBFS  ← PIOROU: ver abaixo
//       cliques durante a fala (8–16 kHz):   −4,5  → −11,7 dB face à entrada
//   (b) nível da fala:                       +5,4  → 0,0 dB;  fala baixa +8,8 → −0,1
//   (c) «agudo» (4–8 kHz / 300–3400 Hz):     +3,1  → +0,1 dB
//   (d) eco residual sozinho:                +7,1  → −1,1 dB;  cauda −1,8 → −10,6
//   (e) ataque, mediana dos primeiros 30 ms: +6,2  → −0,3 dB;  pior frase −202 → −34
//   (f) latência acrescentada:               32,7  → 50,7 ms     ← PIOROU
//
// O QUE CONTRARIOU O DESENHO, e ficou assim:
//   · «menos latência» é falso: os compressores custavam 12 ms e a antecipação
//     do gate custa 30;
//   · as rajadas brancas sozinhas, a cadeia antiga JÁ as calava: o RNNoise
//     deixa-as abaixo do limiar da porta, que as cortava a silêncio. A nova
//     deixa-as a −61 dBFS porque o chão não é silêncio. (Com o processamento
//     do Chromium à frente — modo APM — a antiga já não as cala: −38 contra −42.)
//     O que a antiga deixava passar, e amplificava, são as teclas com corpo;
//   · o «agudo» não vem de duas supressões em série: a supressão do Chromium,
//     sozinha, mexe −0,2 dB na inclinação da fala (modo APM). Vem do nivelador;
//   · o limiar do gate não subiu, DESCEU (−50 → −58): quem trava os cliques é
//     a regra dos transitórios, e um limiar alto comia inícios de frase;
//   · o RNNoise come, sozinho, o início brando de algumas frases (2 em 9 aqui,
//     5 em 9 numa leitura corrida). Não é desta cadeia; a antiga punha-os em
//     silêncio digital e a nova deixa-os como o RNNoise os deixou, ou 24 dB abaixo.
//
// TRÊS CONTROLOS NEGATIVOS, sem os quais o quadro não provaria nada:
//   C1 a ANTIGA tem de mostrar o defeito (eco amplificado, agudos subidos,
//      teclas a passar) — se não mostrasse, a régua não o via;
//   C2 a NOVA sem a rejeição de transitórios deixa passar as teclas;
//   C3 o RNNoise carregou MESMO: o WASM chegou e é válido, a saída não tem
//      silêncio digital no primeiro segundo (é o que um RNNoise por carregar
//      devolve) e o ruído de fundo desce. Mais C0: o canal «entrada» sai igual.
//
// O QUE NÃO PROVA:
//   · que SOA melhor. Ninguém ouviu. A variável SAIDA escreve entrada.wav,
//     antiga.wav e nova.wav numa pasta, para alguém ouvir;
//   · nada sobre o cancelamento de eco: o eco aqui é fala atenuada posta no
//     sinal, não o que um altifalante devolve a um microfone;
//   · teclados a sério: as teclas são sintéticas, e três sons do sistema não
//     são uma amostra. Uma batida ou um som de 50 ms passa — dura como uma sílaba;
//   · um microfone, uma sala, outro browser. O modo APM usa o processamento do
//     Chromium sobre um microfone FALSO (um ficheiro), sem altifalante.
//
// MODO APM (variável APM, em tempo real, ~2 min; mede e não afirma): o mesmo
// sinal como microfone falso do Chromium, com o pedido de HOJE (supressão e
// ganho nativos → cadeia antiga) e o NOVO (só ganho nativo → cadeia nova), e
// se `applyConstraints` muda a supressão numa pista viva (no 151: não muda).
//
//   node web/e2e/voz-natural.mjs                      (a partir da raiz ou de web/)
//   SAIDA=/caminho/pasta node web/e2e/voz-natural.mjs   escreve os .wav para ouvir
//   FALA=/caminho/voz.wav node web/e2e/voz-natural.mjs  outra voz (ficheiro ou pasta; precisa de ffmpeg se não for WAV a 48 kHz)
//   APM=1 node web/e2e/voz-natural.mjs                  mais o quadro ponta a ponta com o Chromium

import { chromium } from '@playwright/test'
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { createServer as criarServidorTcp } from 'node:net'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const AQUI = dirname(fileURLToPath(import.meta.url))
const WEB = join(AQUI, '..')
const TAXA = 48_000
const SAIDA = process.env.SAIDA ?? null
const FALA = process.env.FALA ?? null
const APP = process.env.APP ?? null
const APM = !!process.env.APM
const VERBOSO = !!process.env.VERBOSO

// ───────────────────────── utilitários de sinal ─────────────────────────

const db = (x) => 20 * Math.log10(Math.max(x, 1e-12))
const lin = (d) => Math.pow(10, d / 20)
const amostras = (s) => Math.round(s * TAXA)

/** Gerador determinístico (mulberry32): o sinal é o MESMO em todas as corridas. */
function semente(a) {
  return () => {
    a |= 0
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

function rms(x, a = 0, b = x.length) {
  a = Math.max(0, a)
  b = Math.min(x.length, b)
  let s = 0
  for (let i = a; i < b; i++) s += x[i] * x[i]
  return Math.sqrt(s / Math.max(1, b - a))
}

function pico(x, a = 0, b = x.length) {
  a = Math.max(0, a)
  b = Math.min(x.length, b)
  let m = 0
  for (let i = a; i < b; i++) {
    const v = Math.abs(x[i])
    if (v > m) m = v
  }
  return m
}

/** FFT radix-2 no sítio (re, im com comprimento potência de dois). */
function fft(re, im) {
  const n = re.length
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1
    for (; j & bit; bit >>= 1) j ^= bit
    j ^= bit
    if (i < j) {
      ;[re[i], re[j]] = [re[j], re[i]]
      ;[im[i], im[j]] = [im[j], im[i]]
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len
    const wr = Math.cos(ang)
    const wi = Math.sin(ang)
    for (let i = 0; i < n; i += len) {
      let cr = 1
      let ci = 0
      for (let k = 0; k < len / 2; k++) {
        const ur = re[i + k]
        const ui = im[i + k]
        const vr = re[i + k + len / 2] * cr - im[i + k + len / 2] * ci
        const vi = re[i + k + len / 2] * ci + im[i + k + len / 2] * cr
        re[i + k] = ur + vr
        im[i + k] = ui + vi
        re[i + k + len / 2] = ur - vr
        im[i + k + len / 2] = ui - vi
        const nr = cr * wr - ci * wi
        ci = cr * wi + ci * wr
        cr = nr
      }
    }
  }
}

/**
 * Energia média por banda (Welch, Hann, N=2048, salto N/2) sobre uma lista de
 * janelas [a, b). Devolve a potência média em cada banda pedida (Hz).
 */
function energiaPorBanda(x, janelas, bandas) {
  const N = 2048
  const hann = new Float64Array(N)
  for (let i = 0; i < N; i++) hann[i] = 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / N)
  const soma = bandas.map(() => 0)
  let quadros = 0
  const re = new Float64Array(N)
  const im = new Float64Array(N)
  for (const [a, b] of janelas) {
    for (let p = a; p + N <= b; p += N / 2) {
      for (let i = 0; i < N; i++) {
        re[i] = x[p + i] * hann[i]
        im[i] = 0
      }
      fft(re, im)
      bandas.forEach(([f0, f1], k) => {
        const k0 = Math.ceil((f0 * N) / TAXA)
        const k1 = Math.floor((f1 * N) / TAXA)
        let e = 0
        for (let i = k0; i <= k1; i++) e += re[i] * re[i] + im[i] * im[i]
        soma[k] += e
      })
      quadros++
    }
  }
  return soma.map((e) => e / Math.max(1, quadros))
}

/**
 * Atraso (amostras) de `y` face a `x`: o `L` em [min, max] que maximiza a
 * correlação de `x[i]` com `y[i + L]`, para `i` em [a, b).
 */
function atraso(x, y, a, b, min, max) {
  const corr = (lag, passo) => {
    let s = 0
    for (let i = a; i < b; i += passo) {
      const j = i + lag
      if (j >= 0 && j < y.length) s += x[i] * y[j]
    }
    return s
  }
  // Passo grosso e depois fino, para não custar um minuto.
  let melhor = min
  let melhorV = -Infinity
  for (let lag = min; lag <= max; lag += 4) {
    const v = corr(lag, 3)
    if (v > melhorV) {
      melhorV = v
      melhor = lag
    }
  }
  let fino = melhor
  melhorV = -Infinity
  for (let lag = Math.max(min, melhor - 6); lag <= Math.min(max, melhor + 6); lag++) {
    const v = corr(lag, 1)
    if (v > melhorV) {
      melhorV = v
      fino = lag
    }
  }
  return fino
}

// ───────────────────────── a fala ─────────────────────────

/** Lê um WAV PCM de 16 bits (mono ou estéreo) a 48 kHz sem dependências. */
function lerWav48k(caminho) {
  const b = readFileSync(caminho)
  if (b.toString('ascii', 0, 4) !== 'RIFF' || b.toString('ascii', 8, 12) !== 'WAVE') return null
  let p = 12
  let fmt = null
  let dados = null
  while (p + 8 <= b.length) {
    const id = b.toString('ascii', p, p + 4)
    const tam = b.readUInt32LE(p + 4)
    if (id === 'fmt ') fmt = { formato: b.readUInt16LE(p + 8), canais: b.readUInt16LE(p + 10), taxa: b.readUInt32LE(p + 12), bits: b.readUInt16LE(p + 22) }
    if (id === 'data') dados = b.subarray(p + 8, Math.min(b.length, p + 8 + tam))
    p += 8 + tam + (tam & 1)
  }
  if (!fmt || !dados || fmt.formato !== 1 || fmt.bits !== 16 || fmt.taxa !== TAXA) return null
  const n = Math.floor(dados.length / 2 / fmt.canais)
  const x = new Float32Array(n)
  for (let i = 0; i < n; i++) {
    let s = 0
    for (let c = 0; c < fmt.canais; c++) s += dados.readInt16LE((i * fmt.canais + c) * 2)
    x[i] = s / fmt.canais / 32768
  }
  return x
}

/** Qualquer formato, pelo ffmpeg, se existir. */
function lerComFfmpeg(caminho) {
  try {
    const cru = execFileSync('ffmpeg', ['-v', 'error', '-i', caminho, '-t', '120', '-ac', '1', '-ar', String(TAXA), '-f', 'f32le', '-'], {
      maxBuffer: 1 << 28,
    })
    return new Float32Array(cru.buffer, cru.byteOffset, Math.floor(cru.length / 4)).slice()
  } catch {
    return null
  }
}

/**
 * Parte um sinal em frases: troços com energia separados por pausas.
 * Quadros de 10 ms; activo = acima de (o quadro mais forte − 35 dB).
 */
function frases(x, { pausaMin = 0.2, durMin = 0.5, durMax = 4 } = {}) {
  const Q = amostras(0.01)
  const n = Math.floor(x.length / Q)
  const e = new Float32Array(n)
  let maxE = 0
  for (let i = 0; i < n; i++) {
    e[i] = rms(x, i * Q, (i + 1) * Q)
    if (e[i] > maxE) maxE = e[i]
  }
  const limiar = maxE * lin(-35)
  const out = []
  let ini = -1
  let ultimo = -1
  const fechar = () => {
    if (ini < 0) return
    // Uma leitura corrida não pára de 4 em 4 segundos: um troço mais longo do
    // que `durMax` corta-se no quadro mais fraco do seu último segundo e meio.
    let a = ini
    while (((ultimo + 1 - a) * Q) / TAXA > durMax) {
      const lim = a + Math.floor((durMax * TAXA) / Q)
      let corte = lim
      for (let i = lim - Math.floor((1.5 * TAXA) / Q); i < lim; i++) if (e[i] < e[corte]) corte = i
      out.push(x.slice(a * Q, corte * Q))
      a = corte
    }
    const dur = ((ultimo + 1 - a) * Q) / TAXA
    if (dur >= durMin) out.push(x.slice(a * Q, (ultimo + 1) * Q))
    ini = -1
  }
  for (let i = 0; i < n; i++) {
    if (e[i] > limiar) {
      if (ini < 0) ini = i
      ultimo = i
    } else if (ini >= 0 && ((i - ultimo) * Q) / TAXA >= pausaMin) {
      fechar()
    }
  }
  fechar()
  return out
}

const ALSA = '/usr/share/sounds/alsa'
const ALSA_FICHEIROS = ['Front_Center', 'Front_Left', 'Front_Right', 'Rear_Center', 'Rear_Left', 'Rear_Right', 'Side_Left', 'Side_Right']

/** Substituto SINTÉTICO de fala — só quando não há fala real. Não valida nada sobre a voz. */
function falaSintetica(rand, n = 8) {
  const out = []
  for (let k = 0; k < n; k++) {
    const dur = 1.1 + rand() * 0.5
    const N = amostras(dur)
    const x = new Float32Array(N)
    const f0 = 105 + rand() * 90
    // Três formantes por «sílaba», trocados a cada ~180 ms.
    const VOGAIS = [
      [730, 1090, 2440],
      [270, 2290, 3010],
      [300, 870, 2240],
      [530, 1840, 2480],
      [570, 840, 2410],
    ]
    let fase = 0
    const est = [0, 0, 0, 0, 0, 0]
    for (let i = 0; i < N; i++) {
      const t = i / TAXA
      const sil = Math.floor(t / 0.18)
      const dentro = (t % 0.18) / 0.18
      const env = Math.sin(Math.PI * Math.min(1, dentro * 1.15)) ** 0.6
      const vog = VOGAIS[(sil + k) % VOGAIS.length]
      fase += (f0 * (1 + 0.03 * Math.sin(2 * Math.PI * 5 * t))) / TAXA
      if (fase >= 1) fase -= 1
      // Pulso glotal (dente de serra suavizado) + fricativa no início de 1 em 3 sílabas.
      const fricativa = (sil + k) % 3 === 0 && dentro < 0.25
      const fonte = fricativa ? (rand() * 2 - 1) * 0.35 : 2 * fase - 1
      let y = 0
      for (let f = 0; f < 3; f++) {
        const w = (2 * Math.PI * (fricativa ? 4500 + 1500 * f : vog[f])) / TAXA
        const r = 0.985 - 0.004 * f
        const a1 = 2 * r * Math.cos(w)
        const a2 = -r * r
        const s = fonte * (1 - r) + a1 * est[f * 2] + a2 * est[f * 2 + 1]
        est[f * 2 + 1] = est[f * 2]
        est[f * 2] = s
        y += s / (f + 1)
      }
      x[i] = y * env
    }
    out.push(x)
  }
  return out
}

/** As frases de fala a usar e de onde vieram. */
function carregarFala(rand) {
  const candidatos = []
  if (FALA === 'sintetica') {
    return { real: false, origem: 'SINTÉTICA (pedida)', frases: falaSintetica(rand) }
  }
  if (FALA) {
    const lista = statSync(FALA).isDirectory() ? readdirSync(FALA).sort().map((f) => join(FALA, f)) : [FALA]
    for (const f of lista) candidatos.push(f)
  } else if (existsSync(ALSA)) {
    for (const f of ALSA_FICHEIROS) if (existsSync(join(ALSA, `${f}.wav`))) candidatos.push(join(ALSA, `${f}.wav`))
  }
  const fr = []
  for (const f of candidatos) {
    const x = lerWav48k(f) ?? lerComFfmpeg(f)
    if (!x) continue
    // Um ficheiro curto é UMA frase («Front Center» tem uma pausa a meio que
    // não é fim de frase); só um ficheiro longo se parte pelas pausas.
    if (x.length <= amostras(4)) fr.push(x)
    else for (const fx of frases(x)) fr.push(fx)
  }
  // Um ficheiro PEDIDO que não serve é um erro, não um motivo para medir outra coisa.
  if (FALA && fr.length < 4) throw new Error(`${FALA}: não deu quatro frases de fala (deu ${fr.length})`)
  if (fr.length >= 4) {
    return { real: true, origem: FALA ? `${FALA} (escolhida por quem correu)` : `${ALSA} (alsa-utils: voz humana gravada, 48 kHz)`, frases: fr.slice(0, 8) }
  }
  return { real: false, origem: 'SINTÉTICA (sem fala real nesta máquina)', frases: falaSintetica(rand) }
}

// ───────────────────────── o sinal de ensaio ─────────────────────────

const NIVEL_FALA_DB = -23
const NIVEL_RUIDO_DB = -60
const ECO_DB = -30
const ECO_ATRASO = 0.2
const FALA_BAIXA_DB = -15

/** Onde a frase tem energia a sério: primeira e última amostra acima de (RMS − 20 dB), em janelas de 5 ms. */
function limitesActivos(x) {
  const J = amostras(0.005)
  const r = rms(x)
  const lim = r * lin(-20)
  let a = 0
  let b = x.length
  for (let i = 0; i + J <= x.length; i += amostras(0.001)) {
    if (rms(x, i, i + J) > lim) {
      a = i
      break
    }
  }
  for (let i = x.length - J; i >= 0; i -= amostras(0.001)) {
    if (rms(x, i, i + J) > lim) {
      b = i + J
      break
    }
  }
  return [a, b]
}

/** Um clique de teclado: rajada de banda larga com ataque de 0,2 ms e queda exponencial. */
function clique(rand, durMs, picoDb) {
  const N = amostras(durMs / 1000)
  const x = new Float32Array(N)
  const atq = amostras(0.0002)
  const tau = N / 4
  let m = 0
  for (let i = 0; i < N; i++) {
    const env = (i < atq ? i / atq : 1) * Math.exp(-Math.max(0, i - atq) / tau)
    x[i] = (rand() * 2 - 1) * env
    m = Math.max(m, Math.abs(x[i]))
  }
  const g = lin(picoDb) / m
  for (let i = 0; i < N; i++) x[i] *= g
  return x
}

/**
 * Uma tecla com corpo: o estalo (rajada de 1 ms) mais a ressonância da caixa —
 * sinusóides amortecidas. Mais difícil para o RNNoise do que ruído branco, que
 * é o que ele melhor conhece.
 */
function tecla(rand, picoDb, variante) {
  const N = amostras(0.045)
  const x = new Float32Array(N)
  const modos = [
    [2300 + 400 * variante, 0.004, 1],
    [3900 + 300 * variante, 0.003, 0.7],
    [6100 + 500 * variante, 0.002, 0.5],
    [520 + 90 * variante, 0.009, 0.8],
  ]
  let m = 0
  for (let i = 0; i < N; i++) {
    const t = i / TAXA
    let v = i < amostras(0.001) ? (rand() * 2 - 1) * 0.8 : 0
    for (const [f, tau, a] of modos) v += a * Math.sin(2 * Math.PI * f * t) * Math.exp(-t / tau)
    x[i] = v
    m = Math.max(m, Math.abs(v))
  }
  const g = lin(picoDb) / m
  for (let i = 0; i < N; i++) x[i] *= g
  return x
}

/** Uma batida na mesa: grave, amortecida em dezenas de milissegundos. */
function batida(rand, picoDb, variante) {
  const N = amostras(0.16)
  const x = new Float32Array(N)
  const modos = [
    [85 + 15 * variante, 0.03, 1],
    [170 + 25 * variante, 0.022, 0.7],
    [340 + 40 * variante, 0.012, 0.5],
    [1200, 0.004, 0.25],
  ]
  let m = 0
  for (let i = 0; i < N; i++) {
    const t = i / TAXA
    let v = i < amostras(0.0015) ? (rand() * 2 - 1) * 0.5 : 0
    for (const [f, tau, a] of modos) v += a * Math.sin(2 * Math.PI * f * t) * Math.exp(-t / tau)
    x[i] = v
    m = Math.max(m, Math.abs(v))
  }
  const g = lin(picoDb) / m
  for (let i = 0; i < N; i++) x[i] *= g
  return x
}

/** Transitórios GRAVADOS que vêm com o sistema, se lá estiverem (e houver ffmpeg para os ler). */
const TRANSITORIOS_REAIS = [
  ['obturador', '/usr/share/sounds/freedesktop/stereo/camera-shutter.oga'],
  ['clique-gnome', '/usr/share/sounds/gnome/default/alerts/click.ogg'],
  ['ficha', '/usr/share/sounds/freedesktop/stereo/power-plug.oga'],
]

function construirSinal() {
  const rand = semente(20261013)
  const fala = carregarFala(rand)
  const fr = fala.frases.map((x) => {
    const [a, b] = limitesActivos(x)
    const g = lin(NIVEL_FALA_DB) / rms(x, a, b)
    const y = Float32Array.from(x, (v) => v * g)
    return { x: y, a, b }
  })
  while (fr.length < 8) fr.push(fr[fr.length % Math.max(1, fala.frases.length)])

  const pedacos = [] // { em, x }
  const seg = { ruido: [], fala: [], teclado: null, falaTeclado: [], cauda: [], ecoSozinho: [], falaBaixa: [], ataques: [] }
  let t = 0
  const por = (x, em, g = 1) => pedacos.push({ em, x: g === 1 ? x : Float32Array.from(x, (v) => v * g) })

  // 0 · só ruído de fundo (o RNNoise estabiliza; mede-se o resíduo).
  seg.ruido.push([amostras(0.6), amostras(1.5)])
  t = amostras(1.6)

  // A · fala limpa.
  for (let k = 0; k < 4; k++) {
    por(fr[k].x, t)
    seg.fala.push([t + fr[k].a, t + fr[k].b])
    seg.ataques.push({ em: t + fr[k].a, grupo: 'fala' })
    t += fr[k].x.length + amostras(0.9)
  }

  // B · teclado sozinho: 18 cliques, 3 durações × 3 níveis × 2.
  const cliques = []
  const t0B = t
  t += amostras(0.3)
  for (let rep = 0; rep < 2; rep++) {
    for (const durMs of [3, 8, 15]) {
      for (const nivel of [-35, -25, -15]) {
        const c = clique(rand, durMs, nivel)
        por(c, t)
        cliques.push({ em: t, n: c.length, durMs, nivel })
        t += amostras(0.22 + rand() * 0.08)
      }
    }
  }
  t += amostras(0.3)
  seg.teclado = { janela: [t0B, t], cliques }
  t += amostras(0.5)

  // B2 · teclas com corpo, B3 · batidas na mesa, B4 · transitórios gravados.
  const duros = []
  const t0D = t
  t += amostras(0.3)
  for (let v = 0; v < 3; v++) {
    for (const nivel of [-35, -25, -15, -10]) {
      const c = tecla(rand, nivel, v)
      por(c, t)
      duros.push({ em: t, n: c.length, tipo: 'tecla', nivel })
      t += amostras(0.24 + rand() * 0.08)
    }
  }
  for (let v = 0; v < 2; v++) {
    for (const nivel of [-25, -15]) {
      const c = batida(rand, nivel, v)
      por(c, t)
      duros.push({ em: t, n: c.length, tipo: 'batida', nivel })
      t += amostras(0.45)
    }
  }
  for (const [nome, caminho] of TRANSITORIOS_REAIS) {
    const bruto = existsSync(caminho) ? lerComFfmpeg(caminho) : null
    if (!bruto) continue
    const c = bruto.subarray(0, Math.min(bruto.length, amostras(0.9)))
    const g = lin(-15) / pico(c)
    por(c, t, g)
    duros.push({ em: t, n: c.length, tipo: nome, nivel: -15 })
    t += c.length + amostras(0.4)
  }
  t += amostras(0.3)
  seg.duros = { janela: [t0D, t], estimulos: duros }
  t += amostras(0.5)

  // C · fala com teclado por cima (e entre palavras).
  for (let k = 4; k < 6; k++) {
    por(fr[k].x, t)
    const a = t + fr[k].a
    const b = t + fr[k].b
    seg.ataques.push({ em: a, grupo: 'fala+teclado' })
    const dentro = []
    for (let j = 1; j <= 4; j++) {
      const em = a + Math.floor(((b - a) * j) / 5)
      const c = clique(rand, 8, -20)
      por(c, em)
      dentro.push({ em, n: c.length })
    }
    const depois = []
    for (let j = 0; j < 2; j++) {
      const em = b + amostras(0.25 + j * 0.25)
      const c = clique(rand, 8, -20)
      por(c, em)
      depois.push({ em, n: c.length })
    }
    seg.falaTeclado.push({ fala: [a, b], dentro, depois })
    t += fr[k].x.length + amostras(0.9)
  }

  // D1 · fala seguida da sua própria cauda: a mesma fala 200 ms depois, a −30 dB.
  {
    por(fr[6].x, t)
    por(fr[6].x, t + amostras(ECO_ATRASO), lin(ECO_DB))
    const a = t + fr[6].a
    const b = t + fr[6].b
    seg.ataques.push({ em: a, grupo: 'fala' })
    // Só a cauda: desde 20 ms depois de a fala acabar até a cópia acabar.
    seg.cauda.push([b + amostras(0.02), b + amostras(ECO_ATRASO)])
    t += fr[6].x.length + amostras(ECO_ATRASO) + amostras(1.0)
  }

  // D2 · eco residual sozinho: fala do outro lado que o cancelador não tirou toda.
  for (const k of [7, 2]) {
    por(fr[k].x, t, lin(ECO_DB))
    seg.ecoSozinho.push([t + fr[k].a, t + fr[k].b])
    t += fr[k].x.length + amostras(0.9)
  }

  // E · fala BAIXA (−15 dB): quem fala longe do microfone não pode ser comido.
  for (const k of [0, 5]) {
    por(fr[k].x, t, lin(FALA_BAIXA_DB))
    seg.falaBaixa.push([t + fr[k].a, t + fr[k].b])
    seg.ataques.push({ em: t + fr[k].a, grupo: 'fala-baixa' })
    t += fr[k].x.length + amostras(0.9)
  }
  seg.ruido.push([t - amostras(0.6), t])
  t += amostras(0.3)

  const x = new Float32Array(t)
  // Ruído de fundo estacionário (rosa, Paul Kellet), baixo, em todo o sinal.
  {
    let b0 = 0
    let b1 = 0
    let b2 = 0
    for (let i = 0; i < t; i++) {
      const w = rand() * 2 - 1
      b0 = 0.99765 * b0 + w * 0.099046
      b1 = 0.963 * b1 + w * 0.2965164
      b2 = 0.57 * b2 + w * 1.0526913
      x[i] = b0 + b1 + b2 + w * 0.1848
    }
    const g = lin(NIVEL_RUIDO_DB) / rms(x)
    for (let i = 0; i < t; i++) x[i] *= g
  }
  for (const p of pedacos) for (let i = 0; i < p.x.length && p.em + i < t; i++) x[p.em + i] += p.x[i]
  const pk = pico(x)
  if (pk > 0.98) throw new Error(`o sinal de ensaio satura (pico ${db(pk).toFixed(1)} dBFS) — baixa NIVEL_FALA_DB`)

  // Só para o modo APM: fala e ruído ANTES do ensaio, para o ganho automático
  // do Chromium assentar antes da primeira frase que se mede.
  const aquecimento = new Float32Array(amostras(7))
  for (let i = 0; i < aquecimento.length; i++) aquecimento[i] = x[amostras(0.2) + (i % amostras(1.2))]
  let ta = amostras(0.4)
  for (const k of [1, 3, 5]) {
    for (let i = 0; i < fr[k].x.length && ta + i < aquecimento.length; i++) aquecimento[ta + i] += fr[k].x[i]
    ta += fr[k].x.length + amostras(0.5)
  }
  return { x, seg, aquecimento, fala: { real: fala.real, origem: fala.origem, n: fala.frases.length }, picoDb: db(pk) }
}

// ───────────────────────── a página ─────────────────────────

function portoLivre() {
  return new Promise((res, rej) => {
    const s = criarServidorTcp()
    s.on('error', rej)
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address()
      s.close(() => res(port))
    })
  })
}

/** Um Vite só para esta prova, com a configuração do produto (é ela que resolve os `?url`). */
async function arrancarVite() {
  if (APP) return { url: APP, fechar: async () => {} }
  process.env.NO_HTTPS = '1'
  const { createServer } = await import('vite')
  const porto = await portoLivre()
  const servidor = await createServer({
    root: WEB,
    configFile: join(WEB, 'vite.config.ts'),
    logLevel: VERBOSO ? 'info' : 'error',
    server: { host: '127.0.0.1', port: porto, strictPort: true, hmr: false, https: false, proxy: {} },
    // Pré-empacotar o pacote do RNNoise à partida: descoberto a meio, o Vite
    // recarrega a página — e a renderização em curso perde-se.
    optimizeDeps: { include: ['@sapphi-red/web-noise-suppressor'] },
  })
  await servidor.listen()
  return { url: `http://127.0.0.1:${porto}`, fechar: () => servidor.close() }
}

const paraB64 = (f) => Buffer.from(f.buffer, f.byteOffset, f.byteLength).toString('base64')
const deB64 = (s) => {
  const b = Buffer.from(s, 'base64')
  return new Float32Array(b.buffer, b.byteOffset, b.length / 4).slice()
}

/** Há um troço de zeros EXACTOS em [a, b)? É a assinatura de um RNNoise que ainda não carregou. */
function temSilencioDigital(x, a, b) {
  let seguidos = 0
  for (let i = a; i < b; i++) {
    seguidos = x[i] === 0 ? seguidos + 1 : 0
    if (seguidos >= 128) return true
  }
  return false
}

/**
 * Passa `x` pelas cadeias pedidas, no Chromium, e devolve o PCM de cada uma.
 *
 * O WASM do RNNoise carrega-se dentro do worklet e, até acabar, o nó devolve
 * ZEROS. Num contexto offline a renderização não espera por ele. Por isso:
 * dá-se tempo antes de arrancar, confere-se que a saída do RNNoise não tem
 * silêncio digital no primeiro segundo e, se tiver, repete-se com mais tempo.
 */
async function renderizar(browser, url, x, cadeias) {
  const pedidos = []
  const erros = []
  let ultimo = null
  for (const espera of [600, 2000, 6000]) {
    const pagina = await browser.newPage()
    pagina.on('response', (r) => {
      if (/rnnoise.*\.wasm/.test(r.url())) pedidos.push({ ficheiro: r.url().replace(/^.*\//, '').replace(/\?.*$/, ''), estado: r.status() })
    })
    pagina.on('pageerror', (e) => erros.push(String(e)))
    await pagina.goto(`${url}/e2e/voz-natural.html`, { waitUntil: 'load' })
    await pagina.waitForFunction(() => window.__voz?.pronto === true, null, { timeout: 60_000 })
    const r = await pagina.evaluate(([b64, c, e]) => window.__voz.render(b64, c, e), [paraB64(x), cadeias, espera])
    await pagina.close()
    const pcm = {}
    for (const nome of r.cadeias) pcm[nome] = deB64(r.pcm[nome])
    const tap = cadeias.find((c) => c.tipo === 'rnnoise')
    const mudo = tap ? temSilencioDigital(pcm[tap.nome], amostras(0.05), amostras(1.0)) : false
    ultimo = { pcm, gateDoProduto: r.gateDoProduto, wasm: r.wasm, pedidos, erros, esperaMs: espera, rnnoiseMudoNoArranque: mudo }
    if (!mudo) break
  }
  return ultimo
}

function escreverWav(caminho, x) {
  const b = Buffer.alloc(44 + x.length * 2)
  b.write('RIFF', 0)
  b.writeUInt32LE(36 + x.length * 2, 4)
  b.write('WAVEfmt ', 8)
  b.writeUInt32LE(16, 16)
  b.writeUInt16LE(1, 20)
  b.writeUInt16LE(1, 22)
  b.writeUInt32LE(TAXA, 24)
  b.writeUInt32LE(TAXA * 2, 28)
  b.writeUInt16LE(2, 32)
  b.writeUInt16LE(16, 34)
  b.write('data', 36)
  b.writeUInt32LE(x.length * 2, 40)
  for (let i = 0; i < x.length; i++) b.writeInt16LE(Math.round(Math.max(-1, Math.min(1, x[i])) * 32767), 44 + i * 2)
  writeFileSync(caminho, b)
}

// ───────────────────────── as medidas ─────────────────────────

const BANDA_VOZ = [300, 3400]
const BANDA_AGUDA = [4000, 8000]
const BANDA_CLIQUE = [8000, 16000]
const JANELA_ATAQUE = amostras(0.03)

/**
 * Mede um sinal `y` nos segmentos do ensaio. `pos(n)` diz onde está, em `y`,
 * a amostra `n` da linha do tempo do ensaio — é o que compensa o atraso de
 * cada cadeia (e, no modo APM, a deriva do relógio da captura).
 */
function medir(y, seg, pos) {
  const J = ([a, b]) => {
    const p = pos(a)
    return [p, p + (b - a)]
  }
  const rmsJ = (js) => {
    let s = 0
    let n = 0
    for (const j of js) {
      const [a, b] = J(j)
      const r = rms(y, a, b)
      s += r * r * (b - a)
      n += b - a
    }
    return Math.sqrt(s / Math.max(1, n))
  }
  const picoEm = (c, antes = 0.005, depois = 0.06) => pico(y, pos(c.em) - amostras(antes), pos(c.em) + c.n + amostras(depois))
  const m = {}
  m.ruidoDb = db(rmsJ(seg.ruido))
  // (a) rajadas brancas sozinhas
  m.brancoRmsDb = db(rmsJ([seg.teclado.janela]))
  m.brancoPicoDb = db(Math.max(...seg.teclado.cliques.map((c) => picoEm(c))))
  // (a) teclas com corpo, batidas e transitórios gravados
  m.duros = {}
  for (const c of seg.duros.estimulos) {
    const chave = c.tipo === 'tecla' || c.tipo === 'batida' ? c.tipo : `gravado:${c.tipo}`
    m.duros[chave] = Math.max(m.duros[chave] ?? -999, db(picoEm(c)))
  }
  m.teclaPorNivel = {}
  for (const c of seg.duros.estimulos.filter((e) => e.tipo === 'tecla')) m.teclaPorNivel[c.nivel] = Math.max(m.teclaPorNivel[c.nivel] ?? -999, db(picoEm(c)))
  // (a) cliques durante a fala (energia 8–16 kHz na janela de cada um) e logo a seguir a ela
  const dentro = seg.falaTeclado.flatMap((f) => f.dentro.map((c) => J([c.em - amostras(0.01), c.em + amostras(0.04)])))
  m.cliqueNaFalaDb = 10 * Math.log10(Math.max(1e-20, energiaPorBanda(y, dentro, [BANDA_CLIQUE])[0]))
  m.cliqueAposFalaPicoDb = db(Math.max(...seg.falaTeclado.flatMap((f) => f.depois.map((c) => picoEm(c)))))
  // (b) nível da fala
  m.falaDb = db(rmsJ(seg.fala))
  m.falaBaixaDb = db(rmsJ(seg.falaBaixa))
  // (c) inclinação espectral da fala: 4–8 kHz sobre 300–3400 Hz
  const [ev, ea] = energiaPorBanda(y, seg.fala.map(J), [BANDA_VOZ, BANDA_AGUDA])
  m.inclinacaoDb = 10 * Math.log10(ea / ev)
  // (d) cauda da própria fala e eco residual sozinho
  m.caudaDb = db(rmsJ(seg.cauda))
  m.ecoDb = db(rmsJ(seg.ecoSozinho))
  // (e) os primeiros 30 ms de cada frase
  m.ataques = seg.ataques.map((a) => db(rms(y, pos(a.em), pos(a.em) + JANELA_ATAQUE)))
  return m
}

const mediana = (v) => {
  const s = [...v].sort((a, b) => a - b)
  return s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2
}

// ───────────────────────── o relatório ─────────────────────────

let falhas = 0
const ok = (c, nome, detalhe) => {
  console.log(`  ${c ? '✓' : '✗'} ${nome}${detalhe ? `  — ${detalhe}` : ''}`)
  if (!c) falhas++
}
const nota = (texto) => console.log(`  · ${texto}`)
const f1 = (v) => (Number.isFinite(v) ? (v <= -150 ? '−∞' : v.toFixed(1)) : 'n/d')
const sinalado = (v) => (Number.isFinite(v) ? `${v >= 0 ? '+' : ''}${v.toFixed(1)}` : 'n/d')

function quadro(titulo, colunas, linhas) {
  console.log(`\n${titulo}`)
  const L = 46
  const C = 15
  console.log(`  ${''.padEnd(L)}${colunas.map((c) => c.padStart(C)).join('')}`)
  for (const [nome, valores] of linhas) {
    if (valores === null) {
      console.log(`  ${nome}`)
      continue
    }
    console.log(`  ${nome.padEnd(L)}${valores.map((v) => String(v).padStart(C)).join('')}`)
  }
}

/** As linhas do quadro: cada medida, por coluna, com a entrada como referência onde faz sentido. */
function linhasDoQuadro(ms, ref) {
  const abs = (k) => ms.map((m) => f1(m[k]))
  const rel = (k) => ms.map((m) => (m === ref ? '0.0' : sinalado(m[k] - ref[k])))
  const linhas = [
    ['(a) TECLADO — quanto menos, melhor', null],
    ['    rajadas brancas, sozinhas: pico (dBFS)', abs('brancoPicoDb')],
    ['    rajadas brancas, sozinhas: RMS (dBFS)', abs('brancoRmsDb')],
    ['    teclas com corpo, sozinhas: pico (dBFS)', ms.map((m) => f1(m.duros.tecla))],
    ...[-35, -25, -15, -10].map((n) => [`      tecla a ${n} dBFS: pico (dBFS)`, ms.map((m) => f1(m.teclaPorNivel[n]))]),
    ['    batidas na mesa: pico (dBFS)', ms.map((m) => f1(m.duros.batida))],
    ...Object.keys(ref.duros)
      .filter((k) => k.startsWith('gravado:'))
      .map((k) => [`    ${k}: pico (dBFS)`, ms.map((m) => f1(m.duros[k]))]),
    ['    cliques DURANTE a fala, 8–16 kHz (dB/entr.)', rel('cliqueNaFalaDb')],
    ['    cliques logo após a fala: pico (dBFS)', abs('cliqueAposFalaPicoDb')],
    ['(b) NÍVEL DA FALA (dB face à entrada)', null],
    ['    fala normal', rel('falaDb')],
    ['    fala baixa (−15 dB)', rel('falaBaixaDb')],
    ['(c) «AGUDO»: 4–8 kHz sobre 300–3400 Hz', null],
    ['    inclinação da fala (dB face à entrada)', rel('inclinacaoDb')],
    ['(d) ECO: ganho aplicado (dB face à entrada)', null],
    ['    eco residual sozinho (fala a −30 dB)', rel('ecoDb')],
    ['    cauda logo a seguir à fala', rel('caudaDb')],
    ['(e) ATAQUE: primeiros 30 ms (dB face à entr.)', null],
    ['    mediana das frases', ms.map((m) => (m === ref ? '0.0' : sinalado(mediana(m.ataques.map((v, i) => v - ref.ataques[i])))))],
    ['    a pior frase', ms.map((m) => (m === ref ? '0.0' : sinalado(Math.min(...m.ataques.map((v, i) => v - ref.ataques[i])))))],
    ['(f) LATÊNCIA acrescentada (ms)', ms.map((m) => f1(m.latenciaMs))],
    ['    ruído de fundo, sem fala (dBFS)', abs('ruidoDb')],
  ]
  return linhas
}

// ───────────────────────── modo APM: o processamento do próprio Chromium ─────────────────────────

/**
 * Lê a pista de um microfone FALSO (um WAV) tal como o Chromium a entrega
 * depois do seu processamento (cancelamento de eco, supressão de ruído,
 * ganho automático). `MediaStreamTrackProcessor` dá as amostras exactas, sem
 * um segundo relógio pelo meio — com um `AudioContext` em tempo real a captura
 * perdia dezenas de milissegundos ao acaso (medido).
 */
async function capturarDoChromium(url, wav, audio, segundos) {
  const browser = await chromium.launch({
    args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', `--use-file-for-fake-audio-capture=${wav}`],
  })
  try {
    const pagina = await browser.newPage()
    await pagina.goto(`${url}/e2e/voz-natural.html`, { waitUntil: 'load' })
    const r = await pagina.evaluate(
      async ([audio, segundos]) => {
        if (typeof MediaStreamTrackProcessor === 'undefined') return { erro: 'sem MediaStreamTrackProcessor' }
        const s = await navigator.mediaDevices.getUserMedia({ audio })
        const t = s.getAudioTracks()[0]
        const ler = () => {
          const st = t.getSettings()
          return { echoCancellation: st.echoCancellation, noiseSuppression: st.noiseSuppression, autoGainControl: st.autoGainControl }
        }
        const pedido = ler()
        // Dá para mudar a supressão nativa numa pista VIVA? É disto que depende
        // o recuo do produto quando o RNNoise não carrega.
        let aplicar = 'resolveu'
        try {
          await t.applyConstraints({ ...audio, noiseSuppression: !pedido.noiseSuppression })
        } catch (e) {
          aplicar = `rejeitou (${e.name})`
        }
        const depois = ler()
        try {
          await t.applyConstraints(audio)
        } catch {
          /* fica como estava */
        }
        const leitor = new MediaStreamTrackProcessor({ track: t }).readable.getReader()
        const blocos = []
        let n = 0
        let taxa = 0
        let fimEsperado = null
        let buracos = 0
        const ate = performance.now() + segundos * 1000
        while (performance.now() < ate) {
          const { value: f, done } = await leitor.read()
          if (done) break
          taxa = f.sampleRate
          if (fimEsperado !== null && Math.abs(f.timestamp - fimEsperado) > 2000) buracos++
          fimEsperado = f.timestamp + (f.numberOfFrames / f.sampleRate) * 1e6
          const x = new Float32Array(f.numberOfFrames)
          f.copyTo(x, { planeIndex: 0, format: 'f32-planar' })
          blocos.push(x)
          n += x.length
          f.close()
        }
        t.stop()
        const out = new Float32Array(n)
        let o = 0
        for (const x of blocos) {
          out.set(x, o)
          o += x.length
        }
        const bytes = new Uint8Array(out.buffer)
        let str = ''
        for (let i = 0; i < bytes.length; i += 0x8000) str += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
        return { pedido, aplicar, depois, taxa, buracos, b64: btoa(str) }
      },
      [audio, segundos],
    )
    if (r.erro) return r
    return { ...r, versao: browser.version(), pcm: deB64(r.b64), b64: undefined }
  } finally {
    await browser.close()
  }
}

/**
 * Onde está, na captura, cada amostra do ensaio. A captura começa algures
 * perto do princípio do ficheiro e o relógio do microfone falso deriva alguns
 * milissegundos por minuto face ao do ficheiro, e não a direito. Três âncoras
 * de fala (cedo, a meio, tarde) dão uma linha quebrada; uma quarta, que não
 * entrou na conta, diz quanto ela erra.
 */
function alinharCaptura(x, cap, seg, aquecimento) {
  const lagDe = ([a, b], centro, folga) => atraso(x, cap, a, b, centro - folga, centro + folga)
  const cedo = seg.fala[1]
  const meio = seg.falaTeclado[0].fala
  const tarde = seg.falaBaixa[1]
  const l0 = lagDe(cedo, aquecimento, amostras(0.5))
  const l1 = lagDe(meio, l0, amostras(0.05))
  const l2 = lagDe(tarde, l1, amostras(0.05))
  const pos = (n) => {
    const [na, la, nb, lb] = n < meio[0] ? [cedo[0], l0, meio[0], l1] : [meio[0], l1, tarde[0], l2]
    return n + Math.round(la + ((lb - la) * (n - na)) / (nb - na))
  }
  const prova = seg.fala[3]
  const lProva = lagDe(prova, pos(prova[0]) - prova[0], amostras(0.02))
  return { pos, derivaMs: ((l2 - l0) / TAXA) * 1000, erroMs: ((lProva - (pos(prova[0]) - prova[0])) / TAXA) * 1000 }
}

// ───────────────────────── correr ─────────────────────────

const { x, seg, fala, picoDb, aquecimento } = construirSinal()
console.log(`voz-natural — sinal de ensaio de ${(x.length / TAXA).toFixed(1)} s a ${TAXA} Hz, pico ${picoDb.toFixed(1)} dBFS`)
console.log(`  fala: ${fala.origem} — ${fala.n} frases${fala.real ? '' : '\n  ⚠ NÃO É FALA REAL: as medidas da voz ficam por validar e os limiares que dependem dela não se aplicam'}`)
const gravados = seg.duros.estimulos.filter((e) => e.tipo !== 'tecla' && e.tipo !== 'batida').map((e) => e.tipo)
console.log(`  transitórios gravados do sistema: ${gravados.length ? gravados.join(', ') : 'nenhum encontrado (ou sem ffmpeg)'}`)

const CADEIAS = [
  { nome: 'entrada', tipo: 'entrada' },
  { nome: 'antiga', tipo: 'antiga' },
  { nome: 'nova', tipo: 'nova' },
  { nome: 'rnnoise', tipo: 'rnnoise' },
  // Controlo negativo 2: a nova, com a rejeição de transitórios desligada.
  { nome: 'nova-sem-transitorios', tipo: 'nova', gate: { sustainMs: 0, transientDropDb: 0 } },
]

const vite = await arrancarVite()
const browser = await chromium.launch()
let r
try {
  r = await renderizar(browser, vite.url, x, CADEIAS)
} finally {
  await browser.close()
}
if (r.erros.length) console.log('  erros da página:', r.erros)

const [fa, fb] = [seg.fala[0][0], seg.fala[seg.fala.length - 1][1]]
const lags = {}
const medidas = {}
for (const c of CADEIAS) {
  lags[c.nome] = c.tipo === 'entrada' ? 0 : atraso(x, r.pcm[c.nome], fa, fb, 0, amostras(0.12))
  medidas[c.nome] = medir(r.pcm[c.nome], seg, (n) => n + lags[c.nome])
  medidas[c.nome].latenciaMs = (lags[c.nome] / TAXA) * 1000
}
const E = medidas.entrada
const A = medidas.antiga
const N = medidas.nova
const R = medidas.rnnoise
const S = medidas['nova-sem-transitorios']
const g = r.gateDoProduto

console.log(
  `  gate da cadeia nova (src/voiceChain.ts): limiar ${g.thresholdDb} dBFS, chão ${g.floorDb} dB, histerese ${g.hysteresisDb} dB, ` +
    `sustentação ${g.sustainMs} ms, queda ${g.transientDropDb} dB, antecipação ${g.lookaheadMs} ms, ataque ${g.attackMs} ms, hold ${g.holdMs} ms, release ${g.releaseMs} ms`,
)

quadro(
  'QUADRO 1 — o mesmo sinal por cada cadeia (OfflineAudioContext, worklets reais)',
  ['entrada', 'ANTIGA', 'NOVA', 'só RNNoise', 'nova s/trans.'],
  linhasDoQuadro([E, A, N, R, S], E),
)
console.log('\n  ataque frase a frase (dB face à entrada; A=fala, T=fala+teclado, B=fala baixa):')
for (const [nome, m] of [['antiga', A], ['nova', N], ['só RNNoise', R]]) {
  console.log(`    ${nome.padEnd(11)} ${m.ataques.map((v, i) => `${seg.ataques[i].grupo === 'fala' ? 'A' : seg.ataques[i].grupo === 'fala-baixa' ? 'B' : 'T'}${sinalado(v - E.ataques[i])}`).join('  ')}`)
}

// ── Controlos negativos: sem eles, o quadro acima não prova nada. ──
console.log('\nCONTROLOS NEGATIVOS')
const wasmOk = r.pedidos.length > 0 && r.pedidos.every((p) => p.estado === 200) && r.wasm.valido && r.wasm.bytes > 50_000
ok(wasmOk, 'C3 o WASM do RNNoise chegou à página e é um módulo válido', `${r.wasm.bytes} bytes; ${[...new Set(r.pedidos.map((p) => `${p.ficheiro}→${p.estado}`))].join(', ')}`)
ok(!r.rnnoiseMudoNoArranque, 'C3 o RNNoise estava a processar desde o princípio (sem silêncio digital no 1.º segundo)', `espera antes de renderizar: ${r.esperaMs} ms`)
ok(R.ruidoDb <= E.ruidoDb - 15, 'C3 o RNNoise TIRA ruído (um que não carregou deixava-o igual, ou tudo mudo)', `ruído de fundo ${f1(E.ruidoDb)} → ${f1(R.ruidoDb)} dBFS`)
ok(R.latenciaMs > 5 && Math.abs(R.falaDb - E.falaDb) < 1.5, 'C3 …e deixa passar a fala, com o atraso próprio dele', `fala ${sinalado(R.falaDb - E.falaDb)} dB, atraso ${f1(R.latenciaMs)} ms`)
{
  let dif = 0
  for (let i = 0; i < x.length; i++) dif = Math.max(dif, Math.abs(x[i] - r.pcm.entrada[i]))
  ok(dif < 1e-6, 'C0 a régua: o canal «entrada» sai da página igual ao que entrou', `diferença máxima ${dif.toExponential(1)}`)
}
if (fala.real) {
  ok(A.ecoDb - E.ecoDb >= 3, 'C1 a ANTIGA amplifica o eco residual (a hipótese do ganho de compensação)', `${sinalado(A.ecoDb - E.ecoDb)} dB`)
  ok(A.inclinacaoDb - E.inclinacaoDb >= 1.5, 'C1 a ANTIGA sobe os agudos da fala', `${sinalado(A.inclinacaoDb - E.inclinacaoDb)} dB`)
}
ok(A.duros.tecla >= N.duros.tecla + 10, 'C1 a ANTIGA deixa passar teclas com corpo que a NOVA segura', `pico ${f1(A.duros.tecla)} contra ${f1(N.duros.tecla)} dBFS`)
ok(S.duros.tecla >= N.duros.tecla + 10, 'C2 sem a rejeição de transitórios, a NOVA deixa passar as teclas', `pico ${f1(S.duros.tecla)} contra ${f1(N.duros.tecla)} dBFS`)

// ── Limiares: o que faz esta prova falhar se a cadeia regredir. ──
console.log('\nLIMIARES DA CADEIA NOVA')
const ataquesN = N.ataques.map((v, i) => v - E.ataques[i])
if (fala.real) {
  ok(Math.abs(N.falaDb - E.falaDb) <= 1.5, '(b) fala a ±1,5 dB da entrada', `${sinalado(N.falaDb - E.falaDb)} dB`)
  ok(Math.abs(N.falaBaixaDb - E.falaBaixaDb) <= 1.5, '(b) fala baixa a ±1,5 dB da entrada', `${sinalado(N.falaBaixaDb - E.falaBaixaDb)} dB`)
  ok(Math.abs(N.inclinacaoDb - E.inclinacaoDb) <= 1, '(c) inclinação espectral a ±1 dB da entrada', `${sinalado(N.inclinacaoDb - E.inclinacaoDb)} dB`)
  ok(N.ecoDb - E.ecoDb <= 0, '(d) eco residual sozinho: ganho ≤ 0 dB', `${sinalado(N.ecoDb - E.ecoDb)} dB`)
  ok(N.caudaDb - E.caudaDb <= 0, '(d) cauda a seguir à fala: ganho ≤ 0 dB', `${sinalado(N.caudaDb - E.caudaDb)} dB`)
  // O ataque mede-se face à ENTRADA (é o que o quadro mostra), mas o limiar é
  // sobre o que o GATE tira a mais do que o RNNoise já tirou: o RNNoise come
  // inícios brandos por conta própria (ver o cabeçalho), e isso não é desta
  // cadeia que depende — já era assim na anterior.
  const doGate = N.ataques.map((v, i) => v - R.ataques[i])
  const doRnnoise = R.ataques.map((v, i) => v - E.ataques[i])
  ok(mediana(doGate) >= -1.5, '(e) ataque: na mediana das frases o gate tira menos de 1,5 dB ao que o RNNoise deixou', `${sinalado(mediana(doGate))} dB`)
  const comidos = doGate.filter((v) => v < -3).length
  ok(comidos <= 1, `(e) ataque: o gate come mais de 3 dB ao início de, no máximo, 1 frase em ${doGate.length}`, `${comidos}; a pior ${sinalado(Math.min(...doGate))} dB`)
  nota(`(e) face à ENTRADA: mediana ${sinalado(mediana(ataquesN))} dB, ${ataquesN.filter((v) => v < -3).length} frases em ${ataquesN.length} perdem mais de 3 dB — das quais ${doRnnoise.filter((v) => v < -3).length} já no RNNoise sozinho`)
} else {
  nota('(b) (c) (d) (e) NÃO AVALIADOS: precisam de fala real')
}
ok(N.duros.tecla <= E.duros.tecla - 20, '(a) teclas com corpo: pelo menos 20 dB abaixo da entrada', `${f1(E.duros.tecla)} → ${f1(N.duros.tecla)} dBFS`)
ok(N.brancoPicoDb <= -50, '(a) rajadas brancas: pico abaixo de −50 dBFS', `${f1(N.brancoPicoDb)} dBFS`)
ok(N.latenciaMs <= 53, '(f) latência acrescentada ≤ 53 ms (RNNoise + antecipação do gate)', `${f1(N.latenciaMs)} ms (a antiga: ${f1(A.latenciaMs)} ms)`)

console.log('\nO QUE O QUADRO DIZ CONTRA O DESENHO')
nota(`latência: a nova acrescenta ${f1(N.latenciaMs)} ms e a antiga ${f1(A.latenciaMs)} ms — tirar os compressores poupou ${f1(A.latenciaMs - R.latenciaMs)} ms, a antecipação do gate custa ${g.lookaheadMs} ms`)
nota(`rajadas brancas sozinhas: a ANTIGA já as calava (${f1(A.brancoPicoDb)} dBFS) — o RNNoise deixa-as abaixo do limiar; a NOVA deixa ${f1(N.brancoPicoDb)} dBFS porque o chão não é silêncio`)
nota(`batidas (${f1(N.duros.batida)} dBFS) e sons de dezenas de milissegundos passam: duram tanto como uma sílaba`)

if (SAIDA) {
  mkdirSync(SAIDA, { recursive: true })
  for (const c of CADEIAS) escreverWav(join(SAIDA, `${c.nome}.wav`), r.pcm[c.nome])
  writeFileSync(join(SAIDA, 'medidas.json'), JSON.stringify({ fala, gate: g, lags, medidas, segmentos: seg }, null, 1))
  if (process.env.CRU) for (const c of CADEIAS) writeFileSync(join(SAIDA, `${c.nome}.f32`), Buffer.from(r.pcm[c.nome].buffer))
  console.log(`\n  ficheiros para OUVIR em ${SAIDA}: entrada.wav, antiga.wav, nova.wav`)
}

// ── Modo APM (opcional, em tempo real): o Chromium a tratar o microfone. ──
if (APM) {
  console.log('\nMODO APM — o mesmo sinal como microfone FALSO do Chromium, com o processamento dele')
  const wav = join(tmpdir(), `dx-voz-natural-${process.pid}.wav`)
  const comAquecimento = new Float32Array(aquecimento.length + x.length)
  comAquecimento.set(aquecimento, 0)
  comAquecimento.set(x, aquecimento.length)
  escreverWav(wav, comAquecimento)
  const segundos = comAquecimento.length / TAXA + 1.5
  const HOJE = { echoCancellation: true, noiseSuppression: true, autoGainControl: true }
  const NOVO = { echoCancellation: true, noiseSuppression: false, autoGainControl: true }
  try {
    const capHoje = await capturarDoChromium(vite.url, wav, HOJE, segundos)
    const capNovo = capHoje.erro ? capHoje : await capturarDoChromium(vite.url, wav, NOVO, segundos)
    if (capHoje.erro || capNovo.erro) {
      nota(`não correu: ${capHoje.erro ?? capNovo.erro}`)
    } else {
      console.log(`  Chromium ${capHoje.versao}`)
      for (const [nome, c] of [['pedido de hoje', capHoje], ['pedido novo ', capNovo]]) {
        nota(`${nome}: getSettings = ${JSON.stringify(c.pedido)}; applyConstraints a inverter a supressão: ${c.aplicar}, e ficou noiseSuppression=${c.depois.noiseSuppression}; ${c.buracos} buracos na captura`)
      }
      ok(capNovo.pedido.noiseSuppression === false && capNovo.pedido.echoCancellation === true && capNovo.pedido.autoGainControl === true, 'o Chromium aceita supressão desligada com eco e ganho ligados')
      const browser2 = await chromium.launch()
      let rh
      let rn
      try {
        rh = await renderizar(browser2, vite.url, capHoje.pcm, [{ nome: 'rnnoise', tipo: 'rnnoise' }, { nome: 'antiga', tipo: 'antiga' }])
        rn = await renderizar(browser2, vite.url, capNovo.pcm, [{ nome: 'rnnoise', tipo: 'rnnoise' }, { nome: 'nova', tipo: 'nova' }])
      } finally {
        await browser2.close()
      }
      const ah = alinharCaptura(x, capHoje.pcm, seg, aquecimento.length)
      const an = alinharCaptura(x, capNovo.pcm, seg, aquecimento.length)
      nota(`alinhamento: deriva ${f1(ah.derivaMs)} / ${f1(an.derivaMs)} ms ao longo do ensaio, erro numa frase que não entrou na conta ${f1(ah.erroMs)} / ${f1(an.erroMs)} ms`)
      const col = [
        E,
        medir(capHoje.pcm, seg, ah.pos),
        medir(rh.pcm.antiga, seg, (n) => ah.pos(n) + lags.antiga),
        medir(capNovo.pcm, seg, an.pos),
        medir(rn.pcm.nova, seg, (n) => an.pos(n) + lags.nova),
      ]
      col.forEach((m, i) => {
        if (i) m.latenciaMs = NaN
      })
      quadro(
        'QUADRO 2 — ponta a ponta com o Chromium: HOJE (NS+AGC nativos → antiga) contra NOVO (AGC nativo → nova)',
        ['ficheiro', 'Chr. NS+AGC', '→ ANTIGA', 'Chr. só AGC', '→ NOVA'],
        linhasDoQuadro(col, E).filter(([nome]) => !nome.startsWith('(f)')),
      )
      const tapH = medir(rh.pcm.rnnoise, seg, (n) => ah.pos(n) + lags.rnnoise)
      const tapN = medir(rn.pcm.rnnoise, seg, (n) => an.pos(n) + lags.rnnoise)
      nota(`só o RNNoise sobre cada captura — rajadas brancas: pico ${f1(tapH.brancoPicoDb)} / ${f1(tapN.brancoPicoDb)} dBFS; teclas: ${f1(tapH.duros.tecla)} / ${f1(tapN.duros.tecla)} dBFS; inclinação ${sinalado(tapH.inclinacaoDb - E.inclinacaoDb)} / ${sinalado(tapN.inclinacaoDb - E.inclinacaoDb)} dB`)
      nota('este quadro MEDE, não afirma: é tempo real, e o ganho automático do Chromium ainda está a assentar nas primeiras frases')
      if (SAIDA) {
        escreverWav(join(SAIDA, 'apm-hoje-chromium.wav'), capHoje.pcm)
        escreverWav(join(SAIDA, 'apm-hoje-antiga.wav'), rh.pcm.antiga)
        escreverWav(join(SAIDA, 'apm-novo-chromium.wav'), capNovo.pcm)
        escreverWav(join(SAIDA, 'apm-novo-nova.wav'), rn.pcm.nova)
        writeFileSync(join(SAIDA, 'medidas-apm.json'), JSON.stringify({ colunas: ['ficheiro', 'chromium-hoje', 'hoje-antiga', 'chromium-novo', 'novo-nova'], medidas: col }, null, 1))
      }
    }
  } finally {
    rmSync(wav, { force: true })
  }
}

await vite.fechar()
console.log(
  falhas
    ? `\n✗ voz-natural: ${falhas} falha(s)`
    : `\n✓ voz-natural: a cadeia nova cumpre os limiares e os três controlos negativos distinguem${fala.real ? '' : ' — SEM FALA REAL: (b) (c) (d) (e) por validar'}`,
)
process.exit(falhas ? 1 : 0)
