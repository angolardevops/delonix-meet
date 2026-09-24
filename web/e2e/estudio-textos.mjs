// MÓDULO DE APOIO — não é um teste; é importado por estudio.mjs, directo.mjs e offline.mjs.
/**
 * Textos do Estúdio lidos das CHAVES de tradução, não escritos à mão no teste.
 *
 * Um teste que procura «Gravar|Record» parte quando alguém melhora a frase, e
 * passa com uma frase errada desde que contenha a palavra. Lendo o valor da
 * chave nas três línguas, o teste acompanha o dicionário e continua a exigir
 * que o ecrã mostre o que o dicionário diz.
 *
 * Lê os ficheiros como TEXTO (uma chave por linha, que é o formato que o
 * portão de paridade `lote2` 3.2.7 já impõe), para não depender de o Node
 * saber importar TypeScript.
 */
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const raiz = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'locales')

function mapa(lingua) {
  const out = {}
  const pilha = []
  for (const linha of readFileSync(join(raiz, lingua, 'studio.ts'), 'utf8').split('\n')) {
    const t = linha.trim()
    if (t.startsWith('export default {')) continue
    const abre = t.match(/^([A-Za-z0-9_]+):\s*\{$/)
    if (abre) {
      pilha.push(abre[1])
      continue
    }
    if (t.startsWith('}')) {
      pilha.pop()
      continue
    }
    const par = t.match(/^([A-Za-z0-9_]+):\s*(['"])((?:\\.|(?!\2).)*)\2\s*,?$/)
    if (par) out[[...pilha, par[1]].join('.')] = par[3]
  }
  return out
}

const LINGUAS = ['pt', 'en', 'fr'].map(mapa)

/** Valor da chave `studio.<chave>` em cada língua (sem o prefixo `studio.`). */
export function valores(chave) {
  const vs = LINGUAS.map((m) => m[chave]).filter(Boolean)
  if (!vs.length) throw new Error(`chave de tradução inexistente: studio.${chave}`)
  return vs
}

/**
 * Expressão que casa o texto da chave em qualquer das três línguas. As
 * interpolações (`{{n}}`) casam com qualquer coisa.
 */
export function texto(chave) {
  const partes = valores(chave).map((v) =>
    v
      .split(/\{\{\w+\}\}/)
      .map((p) => p.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
      .join('.*'),
  )
  return new RegExp(partes.join('|'))
}
