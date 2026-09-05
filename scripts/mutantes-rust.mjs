#!/usr/bin/env node
// Teste por mutação das GUARDAS DE AUTORIZAÇÃO do servidor.
//
// O par do `mutantes.mjs`, com um alvo diferente e mais estreito. Ali mutam-se
// operadores em lógica pura; aqui **remove-se uma autorização de cada vez** e
// pergunta-se se algum teste dá por isso.
//
// PORQUE ESTE ALVO. A invariante 8 do AGENTS.md diz que os controlos do
// anfitrião são validados no servidor e nunca confiados ao cliente. Isso é uma
// afirmação sobre TREZE `if` espalhados por 2 800 linhas, e a única forma de a
// verificar é desligá-los um a um. Foi assim que se descobriu, no R92, que o
// teste da transferência de anfitrião passava com a guarda removida.
//
// PORQUE NÃO SE MUTAM OPERADORES AQUI. Em Rust cada mutação custa uma
// recompilação. Mutar tudo seria horas para um relatório cheio de mutantes
// equivalentes; mutar as guardas dá treze perguntas, todas com significado de
// segurança, em minutos.
//
//   node scripts/mutantes-rust.mjs
import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const RAIZ = join(dirname(fileURLToPath(import.meta.url)), '..')
const ALVO = join(RAIZ, 'server/src/signaling.rs')

// Cada padrão é «a guarda deixou de existir». O texto tem de ser exacto — se
// deixar de casar, é porque o código mudou, e o arnês avisa em vez de reportar
// zero sobreviventes por não ter mutado nada.
const GUARDAS = [
  { de: 'if !self.is_host(room_id, peer_id) {', para: 'if false {' },
  { de: 'if self.is_host(room_id, peer_id) {', para: 'if true {' },
  { de: 'if is_presenter || self.is_host(room_id, peer_id) {', para: 'if true {' },
  { de: '&& !self.is_host(room_id, peer_id)', para: '&& false' },
  { de: 'if !self.is_host(room_id, peer_id) || to == peer_id {', para: 'if false {' },
  {
    de: 'if !crate::apikeys::ct_eq(p.reconnect_secret.expose().as_bytes(), segredo.as_bytes()) {',
    para: 'if false {',
  },
]

function bateria() {
  execFileSync('cargo', ['test', '--release', '--manifest-path', join(RAIZ, 'server/Cargo.toml')], {
    stdio: 'pipe',
    timeout: 900000,
    env: { ...process.env, E2E_TIMEOUT_FACTOR: '4' },
  })
}

const original = readFileSync(ALVO, 'utf8')
const linhas = original.split('\n')

// Todos os sítios, com a linha, para o relatório apontar o local exacto.
const sitios = []
for (const g of GUARDAS) {
  let i = original.indexOf(g.de)
  while (i !== -1) {
    sitios.push({ i, ...g, ln: original.slice(0, i).split('\n').length })
    i = original.indexOf(g.de, i + g.de.length)
  }
}
if (!sitios.length) {
  console.error('✗ nenhum padrão de guarda casou — o código mudou e este arnês está cego')
  process.exit(2)
}
// As guardas mais específicas contêm as genéricas (`if !self.is_host(...) {` é
// prefixo de `if !self.is_host(...) || to == peer_id {`). Fica a mais LONGA em
// cada posição, senão mutava-se meia expressão e o resultado nem compilava.
const porPos = new Map()
for (const s of sitios) {
  const anterior = porPos.get(s.i)
  if (!anterior || s.de.length > anterior.de.length) porPos.set(s.i, s)
}
const finais = [...porPos.values()].sort((a, b) => a.ln - b.ln)

console.log(`${finais.length} guardas de autorização a desligar, uma a uma.\n`)
try {
  bateria()
} catch {
  console.error('✗ a bateria já está vermelha ANTES de mutar')
  process.exit(2)
}

const sobreviventes = []
for (const s of finais) {
  const mutado = original.slice(0, s.i) + s.para + original.slice(s.i + s.de.length)
  writeFileSync(ALVO, mutado)
  let morto = false
  try {
    bateria()
  } catch {
    morto = true
  }
  writeFileSync(ALVO, original)
  const ctx = linhas[s.ln - 1].trim().slice(0, 76)
  console.log(`  ${morto ? '✓ guardada ' : '✗ SEM TESTE'}  linha ${String(s.ln).padStart(4)}  ${ctx}`)
  if (!morto) sobreviventes.push({ ln: s.ln, ctx })
}

console.log(
  `\n${finais.length} guardas · ${finais.length - sobreviventes.length} defendidas · ` +
    `${sobreviventes.length} SEM TESTE`,
)
if (sobreviventes.length) {
  console.log('\nEstas autorizações podem ser removidas e a bateria fica verde:\n')
  for (const s of sobreviventes) console.log(`  signaling.rs:${s.ln}  ${s.ctx}`)
  console.log('\nCada uma é um controlo que se diz validado no servidor e que ninguém prova.')
}
process.exit(sobreviventes.length ? 1 : 0)
