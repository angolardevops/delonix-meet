#!/usr/bin/env node
// Teste por MUTAÇÃO: quantas asserções guardam mesmo alguma coisa?
//
// PORQUE EXISTE. Quatro correcções seguidas foram entregues com testes que
// davam VERDE com o produto partido (R69, R71, R90, R91, R92). Não é distracção
// pontual — é o modo de falha dominante deste trabalho, e uma suite verde
// deixou de ser prova. Isto mede-a.
//
// COMO FUNCIONA. Aplica uma mutação pequena e semanticamente real ao CÓDIGO
// (não ao teste), corre a bateria, e vê se alguém dá por ela:
//   · morto      — algum teste falhou. A asserção guarda mesmo.
//   · SOBREVIVEU — bateria verde com o código alterado. Ninguém guarda isto.
//
// Um sobrevivente NÃO é necessariamente um defeito: pode ser código
// equivalente (`x > 0` e `x >= 1` em inteiros), ou uma linha que ninguém
// promete. O valor está em OLHAR para cada um e decidir — e é por isso que a
// saída dá o ficheiro, a linha e a mutação, não só um número.
//
//   node scripts/mutantes.mjs                  # tudo
//   node scripts/mutantes.mjs callQuality      # só um alvo
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const RAIZ = join(dirname(fileURLToPath(import.meta.url)), '..')
const WEB = join(RAIZ, 'web')

// Os alvos são os módulos de DECISÃO PURA — os que decidem qualidade, camada,
// recuperação e corte. São os que mais custam quando estão errados e os mais
// baratos de mutar: sem rede, sem DOM, sem relógio.
const ALVOS = [
  'src/callQuality.ts',
  'src/callRecovery.ts',
  'src/layerPolicy.ts',
  'src/callTimings.ts',
  'src/sfuLifecycle.ts',
  'src/studio/analise.ts',
]

// Operadores escolhidos por serem os erros que se cometem A SÉRIO: fronteiras
// trocadas, condições invertidas, limiares deslocados. Nada de mutações
// exóticas que ninguém escreveria — essas enchem o relatório de ruído.
const OPS = [
  { de: '>=', para: '>', nome: '>= vira >' },
  { de: '<=', para: '<', nome: '<= vira <' },
  { de: '&&', para: '||', nome: '&& vira ||' },
  { de: '||', para: '&&', nome: '|| vira &&' },
  { de: '===', para: '!==', nome: '=== vira !==' },
]

/** Todos os índices de `agulha` em `palheiro`, sem regex — os operadores têm
 *  caracteres especiais e escapá-los à mão é uma fonte de erro que não vale a
 *  pena correr. */
function ocorrencias(palheiro, agulha) {
  const out = []
  let i = palheiro.indexOf(agulha)
  while (i !== -1) {
    out.push(i)
    i = palheiro.indexOf(agulha, i + agulha.length)
  }
  return out
}

const linhaDe = (txt, i) => txt.slice(0, i).split('\n').length

function baterias() {
  execFileSync(join(WEB, 'node_modules/.bin/vitest'), ['run'], {
    cwd: WEB, stdio: 'pipe', timeout: 180000,
  })
}

const filtro = process.argv[2]
const alvos = filtro ? ALVOS.filter((a) => a.includes(filtro)) : ALVOS
if (!alvos.length) {
  console.error(`sem alvo a condizer com "${filtro}"`)
  process.exit(2)
}

// A bateria TEM de estar verde antes de começar: mutar uma árvore já vermelha
// dá «morto» a toda a gente e o relatório fica inútil.
try {
  baterias()
} catch {
  console.error('✗ a bateria já está vermelha ANTES de mutar — corrige isso primeiro')
  process.exit(2)
}

// Sobreviventes com razão escrita — mutantes equivalentes. Ver o cabeçalho do
// ficheiro para o que isso significa.
const LEDGER = join(RAIZ, 'scripts/mutantes-equivalentes.txt')
const conhecidos = existsSync(LEDGER)
  ? readFileSync(LEDGER, 'utf8')
      .split('\n')
      .filter((l) => l.trim() && !l.startsWith('#'))
      .map((l) => l.split('·')[0].trim())
  : []

const sobreviventes = []
let total = 0

for (const rel of alvos) {
  const caminho = join(WEB, rel)
  const original = readFileSync(caminho, 'utf8')
  const linhas = original.split('\n')
  for (const op of OPS) {
    // Uma ocorrência de cada vez: mutar todas ao mesmo tempo esconderia quais
    // são apanhadas.
    const sitios = []
    for (const i of ocorrencias(original, op.de)) {
      const ln = linhaDe(original, i)
      const texto = linhas[ln - 1].trim()
      // Comentários não são código. Nem `>=` dentro de `>>=` ou de uma string,
      // mas esses são raros o suficiente para aparecerem no relatório como
      // sobreviventes óbvios em vez de valer uma análise sintáctica.
      if (!texto.startsWith('//') && !texto.startsWith('*')) sitios.push({ i, ln, texto })
    }
    for (const s of sitios) {
      total++
      const mutado = original.slice(0, s.i) + op.para + original.slice(s.i + op.de.length)
      writeFileSync(caminho, mutado)
      let morto = false
      try {
        baterias()
      } catch {
        morto = true
      }
      writeFileSync(caminho, original)
      if (!morto) {
        const chave = `${rel}:${s.ln}`
        const explicado = conhecidos.includes(chave)
        sobreviventes.push({ rel, ln: s.ln, op: op.nome, texto: s.texto.slice(0, 92), explicado })
        process.stdout.write(explicado ? '=' : 'S')
      } else {
        process.stdout.write('.')
      }
    }
  }
}
const porExplicar = sobreviventes.filter((s) => !s.explicado)
const explicados = sobreviventes.length - porExplicar.length
console.log(
  `\n\n${total} mutações · ${total - sobreviventes.length} mortas · ` +
    `${porExplicar.length} POR EXPLICAR · ${explicados} equivalentes com razão escrita`,
)
if (porExplicar.length) {
  console.log('\nSobreviventes sem razão — a bateria fica VERDE com o código assim:\n')
  for (const s of porExplicar) {
    console.log(`  ${s.rel}:${s.ln}  [${s.op}]`)
    console.log(`      ${s.texto}`)
  }
  console.log('\nCada um é uma pergunta, não uma acusação: ou falta lá uma asserção,')
  console.log('ou a linha é equivalente. Se for equivalente, a razão vai para')
  console.log('scripts/mutantes-equivalentes.txt — senão volta a investigar-se daqui a três meses.')
}
process.exit(porExplicar.length ? 1 : 0)
