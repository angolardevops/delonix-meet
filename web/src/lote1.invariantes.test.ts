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
  const pesadas = ['Room', 'Calendar', 'Analytics', 'Recordings', 'Directory', 'Whiteboards']

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

  it('o Shell não é arrastado pelo Login nem pela Landing', () => {
    // LanguageToggle/ThemePicker viviam no Shell; importá-los de lá trazia a
    // consola inteira (CommandPalette, NotificationCenter, OnboardingTour…).
    for (const p of ['web/src/pages/Login.tsx', 'web/src/pages/Landing.tsx', 'web/src/pages/Room.tsx']) {
      expect(read(p)).not.toContain("from '../components/Shell'")
    }
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
  const css = read('web/src/styles.scss')

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
    for (const c of ['.join-box', '.people-search', '.cmd-search', '.app-bar-join']) {
      expect(css).toContain(`${c}:focus-within`)
    }
  })
})
