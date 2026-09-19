import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * O CI declara `E2E_TIMEOUT_FACTOR=4` porque sabe que o runner é lento. Um teste
 * de media que o ignore falha ao acaso — e um portão que falha ao acaso perde a
 * credibilidade toda, que é a frase que já estava escrita no próprio CI.
 *
 * Aconteceu ao `tempos.mjs`: era o único do trabalho a ignorá-lo, e deu três
 * falhas seguidas com a media a chegar em 185 ms e a recolha de ICE a levar
 * 74 840 ms (R118).
 */
const raiz = join(__dirname, '..', 'e2e')
// Os COMENTÁRIOS saem antes de procurar. A primeira versão deste portão
// sobreviveu a tirar o factor do `tempos.mjs`, porque o comentário logo acima
// continuava a falar dele — o portão media a presença de uma palavra, não o
// comportamento. É a mesma falha que este ficheiro existe para impedir.
const semComentarios = (f: string) =>
  readFileSync(join(raiz, f), 'utf8')
    .replace(/\/\/[^\n]*/g, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
// E exige-se a LEITURA do ambiente, não a menção: `process.env.E2E_TIMEOUT_FACTOR`.
const LE_O_FACTOR = /process\.env\.E2E_TIMEOUT_FACTOR/

describe('R118 · quem espera por media honra o E2E_TIMEOUT_FACTOR', () => {
  // Só os que ESPERAM por uma ligação de media. Os que medem layout ou falam
  // com a API não têm ICE pelo meio e não precisam do factor.
  const COM_MEDIA = ['tempos.mjs', 'estudio.mjs']
  for (const f of COM_MEDIA) {
    it(`${f} lê o factor do ambiente`, () => {
      expect(semComentarios(f), `${f} espera por media e ignora o factor`).toMatch(LE_O_FACTOR)
    })
  }

  it('e a lista acima não esqueceu nenhum ficheiro novo com espera longa', () => {
    // A armadilha desta família: a lista é escrita à mão e um ficheiro novo
    // entra sem portão. Aqui procura-se a FORMA do problema — uma espera de
    // dezenas de segundos por um marco de media — em todos os `.mjs`.
    const suspeitos: string[] = []
    for (const f of readdirSync(raiz)) {
      if (!f.endsWith('.mjs') || COM_MEDIA.includes(f)) continue
      const src = semComentarios(f)
      const esperaLonga = /\b(\d{5,})\s*(?:\*|,|\))/.test(src)
      const temMedia = /join_ms|ice_gathering|getStats|RTCPeerConnection|__dlx\.tempos/.test(src)
      if (esperaLonga && temMedia && !LE_O_FACTOR.test(src)) suspeitos.push(f)
    }
    expect(suspeitos).toEqual([])
  })
})
