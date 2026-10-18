import { describe, expect, it } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'

const raiz = join(__dirname, '..')
const ler = (p: string) => readFileSync(join(raiz, p), 'utf8')

const TELEFONE = ler('pages/EmparelharTelefone.tsx')
const OPERADOR = ler('pages/studio/tv/Emparelhar.tsx')

/** Todo o CSS do `web/src/ui`, junto: uma classe existe se existir em algum. */
const CSS = readdirSync(join(raiz, 'ui'))
  .filter((f) => f.endsWith('.css'))
  .map((f) => ler(join('ui', f)))
  .join('\n')

/**
 * As classes estáticas que um ficheiro usa. Só literais — `className="a b"` e
 * `cx('a', 'b')`; o que vem de variável não se pode verificar daqui, e um
 * teste que finge verificar é pior do que nenhum.
 */
function classesDe(fonte: string): string[] {
  const vistas = new Set<string>()
  for (const m of fonte.matchAll(/className="([^"{}]+)"/g)) for (const c of m[1].split(/\s+/)) if (c) vistas.add(c)
  for (const m of fonte.matchAll(/cx\(([^)]*)\)/g)) for (const s of m[1].matchAll(/'([^']+)'/g)) for (const c of s[1].split(/\s+/)) if (c) vistas.add(c)
  return [...vistas]
}

describe('os ecrãs de emparelhamento leem-se', () => {
  it('nenhuma classe usada nestes ecrãs está sem CSS', () => {
    // O #356 entrou com SETE classes que não existiam em lugar algum:
    // `tv-emparelhar`, `__qr`, `__lado`, `__codigo`, `tv-lista`, `dx-centro`
    // e `dx-h2`. O painel do QR — o momento em que alguém tem de ler o
    // código antes de expirar — aparecia sem grelha, com o SVG ao tamanho
    // natural e o código sem destaque. Nenhum portão apanhava isso.
    for (const [nome, fonte] of [
      ['EmparelharTelefone.tsx', TELEFONE],
      ['studio/tv/Emparelhar.tsx', OPERADOR],
    ] as const) {
      const semCss = classesDe(fonte).filter((c) => !new RegExp(`\\.${c.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(?![\\w-])`).test(CSS))
      expect(semCss, `${nome} usa classes que não existem em nenhum .css`).toEqual([])
    }
  })

  it('as classes do painel do QR estão lá, e o QR tem tamanho', () => {
    for (const c of ['tv-emparelhar', 'tv-emparelhar__qr', 'tv-emparelhar__lado', 'tv-emparelhar__codigo', 'tv-lista', 'emp-tel']) {
      expect(CSS, `falta .${c}`).toMatch(new RegExp(`\\.${c}(?![\\w-])`))
    }
    // O SVG do gerador vem SEM dimensões: sem isto saía ao tamanho natural e
    // empurrava o painel para fora do ecrã.
    const qr = CSS.slice(CSS.indexOf('.tv-emparelhar__qr'))
    expect(qr.slice(0, 400)).toMatch(/width:\s*\d/)
    // E um QR escuro em tema escuro não se lê: o fundo do QR é claro fixo.
    expect(qr.slice(0, 400)).toMatch(/background:\s*#fff/)
  })

  it('o ecrã estreito empilha o QR e o texto — é num telefone que isto se usa', () => {
    expect(CSS).toMatch(/@media\s*\(max-width:\s*4\d\dpx\)[\s\S]{0,400}\.tv-emparelhar\s*\{[\s\S]{0,200}flex-direction:\s*column/)
  })

  it('NENHUM código técnico chega ao ecrã — nem no telefone nem no operador', () => {
    // A regressão: `apiErrorCode(e) ?? t('…falhou')` mostra o CÓDIGO quando
    // ele existe. `studio.invalid_device` num quadro vermelho, em inglês
    // qualquer que fosse a língua, sem dizer o que fazer.
    for (const [nome, fonte] of [
      ['EmparelharTelefone.tsx', TELEFONE],
      ['studio/tv/Emparelhar.tsx', OPERADOR],
    ] as const) {
      expect(fonte, `${nome}: o código cru volta ao ecrã`).not.toMatch(/setErro\(\s*apiErrorCode\(/)
      expect(fonte, `${nome}: o código cru volta ao ecrã pelo ?? final`).not.toMatch(/\(\s*c\s*\?\?\s*t\(/)
    }
    // E o código não se perde: vai para a consola, que é onde serve.
    expect(TELEFONE).toMatch(/console\.warn\(/)
    expect(OPERADOR).toMatch(/console\.warn\(/)
  })

  it('cada código que o servidor emite neste fluxo TEM frase, e as quatro línguas têm-na', async () => {
    // Medido contra o servidor: estes são os códigos documentados nas rotas
    // do estúdio (`grep '`studio\.[a-z_]+`' server/src/studio.rs`).
    const CODIGOS = ['studio.source_number_taken', 'studio.full', 'studio.not_manager', 'studio.not_operator', 'studio.invalid_name', 'studio.pairing_invalid']
    for (const c of CODIGOS) {
      expect(OPERADOR, `o operador não tem frase para ${c}`).toContain(`case '${c}'`)
    }
    const CHAVES = ['numeroOcupado', 'cheio', 'semPermissao', 'nomeInvalido', 'expirado', 'falhou']
    for (const loc of ['pt', 'en', 'fr', 'zh']) {
      const tv = ler(join('locales', loc, 'tv.ts'))
      const bloco = tv.slice(tv.indexOf('emparelhar:'), tv.indexOf('emparelhar:') + 2500)
      for (const k of CHAVES) expect(bloco, `${loc}/tv.ts: falta emparelhar.${k}`).toMatch(new RegExp(`${k}\\s*:`))
    }
    // E no telefone: o 429 do limite por IP tem frase própria nas quatro.
    expect(TELEFONE).toMatch(/status === 429/)
    for (const loc of ['pt', 'en', 'fr', 'zh']) {
      expect(ler(join('locales', loc, 'telemovel.ts')), `${loc}: falta emparelhar.demasiados`).toMatch(/demasiados\s*:/)
    }
  })
})
