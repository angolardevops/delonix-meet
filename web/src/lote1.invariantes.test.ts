/**
 * Fitness functions do lote 1 (docs/ux-perf-review.md).
 *
 * Cada um destes invariantes custou uma medição a descobrir e custa um `git
 * revert` a perder em silêncio: um `import` estático reposto no App.tsx volta a
 * fundir a sala com o dashboard sem que nada fique vermelho. É por isso que
 * são testes e não uma nota no fim do relatório.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const root = join(__dirname, '..', '..')
const read = (p: string) => readFileSync(join(root, p), 'utf8')

describe('1.1 · a app é servida comprimida', () => {
  for (const conf of ['deploy/k8s/nginx.conf', 'deploy/nginx-delonix.conf']) {
    it(`${conf} liga o gzip e o Vary`, () => {
      const s = read(conf)
      expect(s).toMatch(/^\s*gzip\s+on;/m)
      expect(s).toMatch(/^\s*gzip_vary\s+on;/m)
      // Sem estes tipos, o JS e o CSS — o grosso do peso — saíam crus na mesma.
      expect(s).toMatch(/gzip_types[\s\S]*?text\/css/)
      expect(s).toMatch(/gzip_types[\s\S]*?application\/javascript/)
    })
  }
})

describe('1.2 · as páginas pesadas não entram no chunk de arranque', () => {
  const app = read('web/src/App.tsx')
  // Room arrasta webrtc/media/e2ee/signaling atrás de si; as outras são as
  // maiores da consola. Nenhuma pode voltar a ser importada estaticamente.
  const pesadas = ['Room', 'Lobby', 'Calendar', 'Analytics', 'Recordings', 'Directory', 'Whiteboards', 'Studio', 'Integrations', 'Admin']

  for (const p of pesadas) {
    it(`${p} é lazy`, () => {
      expect(app).not.toMatch(new RegExp(`^import ${p} from './pages/${p}'`, 'm'))
      expect(app).toContain(`const ${p} = lazy(() => import('./pages/${p}'))`)
    })
  }

  it('cada lazy tem uma fronteira de Suspense', () => {
    expect(app).toContain('<Suspense')
  })

  it('a fronteira fica DENTRO do Shell — mudar de página não desmonta o rail', () => {
    const shellAbre = app.indexOf('<Shell')
    const fallbackDentro = app.indexOf('<RouteFallback>', shellAbre)
    const shellFecha = app.indexOf('</Shell>')
    expect(shellAbre).toBeGreaterThan(-1)
    expect(fallbackDentro).toBeGreaterThan(shellAbre)
    expect(fallbackDentro).toBeLessThan(shellFecha)
  })

  it('o Shell não é arrastado pelos ecrãs que vivem fora da consola', () => {
    // Entrar, a sala e as páginas públicas não têm rail: importar o Shell
    // trazia a consola inteira (paleta, definições, MFA) para esses chunks.
    for (const p of ['Login', 'Room', 'SharePage', 'Status', 'Legal', 'ApiDocs']) {
      expect(read(`web/src/pages/${p}.tsx`)).not.toMatch(/from '\.\.\/components\/(Shell|PageBar|shellContext)'/)
    }
  })

  it('a moderação vive DENTRO da consola (lote 2, template DelonixModeration com o rail)', () => {
    // Saiu da lista acima de propósito: o template desenha-a com o rail, e a
    // `PageBar` que importa só é legítima se a página for mesmo filha do Shell.
    const app = read('web/src/App.tsx')
    const shellAbre = app.indexOf('<Shell')
    const shellFecha = app.indexOf('</Shell>')
    const lobby = app.indexOf('<Lobby code={route.code} />')
    expect(lobby).toBeGreaterThan(shellAbre)
    expect(lobby).toBeLessThan(shellFecha)
  })
})

describe('1.3 · só o idioma em uso viaja', () => {
  const i18n = read('web/src/i18n.ts')

  it('PT é o único dicionário estático (é o fallback)', () => {
    expect(i18n).toContain("import pt from './locales/pt'")
  })

  it('EN e FR só chegam por import() dinâmico', () => {
    for (const l of ['en', 'fr']) {
      expect(i18n).not.toMatch(new RegExp(`^import ${l} from './locales/${l}'`, 'm'))
      expect(i18n).toContain(`import('./locales/${l}')`)
    }
  })

  it('o ficheiro deixou de carregar os três dicionários inline', () => {
    // Eram 1980 linhas com pt/en/fr embutidos — 98,9 KB no chunk de arranque.
    expect(i18n.split('\n').length).toBeLessThan(150)
  })
})

describe('4.3 · nenhum foco fica invisível', () => {
  const css = read('web/src/ui/base.css')

  it('existe uma rede de segurança em :focus-visible', () => {
    expect(css).toMatch(/:where\([^)]*button[^)]*\):focus-visible\s*\{[^}]*outline:\s*2px solid/)
  })

  it('a rede usa outline, não box-shadow', () => {
    // box-shadow é disputado por dezenas de regras de classe deste ficheiro e
    // é interpolado pelo `transition: all` global (achado 2.5): o anel chegava
    // a não aparecer. outline não é disputado por ninguém.
    const rede = css.match(/:where\([^)]*button[^)]*\):focus-visible\s*\{[^}]*\}/)?.[0] ?? ''
    expect(rede).not.toContain('box-shadow')
  })

  it('os inputs sem borda têm o anel no contentor', () => {
    // Um input com `border: 0` dentro de uma caixa desenhada não tem onde
    // mostrar o foco: o anel tem de ir para a caixa.
    const shell = read('web/src/ui/shell.css')
    expect(shell).toContain('.palette__search:focus-within')
  })
})
