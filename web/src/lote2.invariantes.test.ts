/**
 * Fitness functions do lote 2 (docs/ux-perf-review.md) e das práticas de estado
 * trazidas do `delonix-portal`.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const root = join(__dirname, '..', '..')
const read = (p: string) => readFileSync(join(root, p), 'utf8')
const css = () => read('web/src/styles.scss')

describe('3.1.1 · a navegação tem comportamento em ecrã estreito', () => {
  it('o rail sai do fluxo abaixo de 900px', () => {
    const mq = css().match(/@media \(max-width: 900px\) \{[\s\S]*?\n\}/g)?.join('\n') ?? ''
    expect(mq).toMatch(/\.shell-nav\s*\{[^}]*position:\s*fixed/)
    expect(mq).toMatch(/\.shell-nav\s*\{[^}]*transform:\s*translateX\(-100%\)/)
    expect(mq).toContain('.shell.nav-open .shell-nav')
  })

  it('a regra de abrir vence a de fechar por especificidade', () => {
    // `.shell.nav-open .shell-nav` (0,3,0) > `.shell-nav` (0,1,0). Se alguém
    // trocar por `.nav-open .shell-nav` a gaveta deixa de abrir em silêncio.
    expect(css()).toContain('.shell.nav-open .shell-nav { transform: none; }')
  })

  it('o Shell tem estado, backdrop, Esc e ARIA', () => {
    const s = read('web/src/components/Shell.tsx')
    expect(s).toContain('const [navOpen, setNavOpen] = useState(false)')
    expect(s).toContain('shell-nav-backdrop')
    expect(s).toContain("e.key === 'Escape'")
    expect(s).toContain('aria-expanded={navOpen}')
    expect(s).toContain('aria-controls="shell-nav"')
  })

  it('escolher um destino fecha a gaveta', () => {
    // Uma gaveta sobreposta que fica aberta depois de navegar tapa o que a
    // pessoa acabou de pedir.
    const s = read('web/src/components/Shell.tsx')
    expect(s).toMatch(/function go\(k: NavKey\) \{\s*setNavOpen\(false\)\s*onNavigate\(k\)/)
    expect(s).toContain('onClick={() => go(n.key)}')
  })

  it('a gaveta não persiste — só o colapso de desktop é preferência', () => {
    const s = read('web/src/components/Shell.tsx')
    expect(s).not.toMatch(/localStorage[^\n]*nav_open/i)
  })
})

describe('3.1.3 · alturas de viewport em dvh', () => {
  it('todo o 100vh tem um 100dvh a seguir', () => {
    const orfaos = css()
      .split('\n')
      .map((l, i) => ({ l, n: i + 1 }))
      .filter(({ l }) => /100vh/.test(l) && !/100dvh/.test(l))
    expect(orfaos.map((o) => `${o.n}: ${o.l.trim()}`)).toEqual([])
  })
})

describe('3.1.4 · as ações não desaparecem no telemóvel', () => {
  it('entrar por código muda-se para a gaveta em vez de ser escondido', () => {
    const s = css()
    expect(s).not.toMatch(/\.app-bar-date,\s*\.app-bar-join \{ display: none; \}/)
    expect(s).toContain('.qa-drawer { display: flex; }')
  })

  it('as ações rápidas são um componente só, usado nos dois sítios', () => {
    const s = read('web/src/components/Shell.tsx')
    expect(s).toContain('function QuickActions(')
    expect(s).toContain('<QuickActions variant="bar"')
    expect(s).toContain('variant="drawer"')
  })
})

describe('3.2.5 · nada de emoji como controlo na consola', () => {
  // `⌘` e `⌥` são NOMES DE TECLAS dentro de <kbd> — conteúdo, não controlo.
  const TECLAS = /[\u2318\u2325\u21E7\u23CE]/u
  const EMOJI = /[\u{1F300}-\u{1FAFF}\u{2190}-\u{21FF}\u{2300}-\u{27BF}\u{2B00}-\u{2BFF}]/u
  // A SALA entrou nesta lista em 2026-09-03 (R88). Eram 40 glifos usados como
  // ícone — ⏳ para o temporizador, 📊 para as sondagens, ❓ para o Q&A, 🛡 para
  // o código de segurança — a par do conjunto SVG que já existia ao lado, na
  // MESMA barra. A regra não era nova; faltava-lhe cobertura.
  //
  // O que NÃO entra nesta lista, e é deliberado: `Landing.tsx`, `Analytics.tsx`
  // e `Studio.tsx` ainda têm glifos em ARRAYS DE DADOS (listas de
  // funcionalidades, rótulos de canto `↖↗↙↘`). Convertê-los é outro trabalho —
  // mexe na forma dos dados, não na marcação — e está escrito em
  // `docs/reference/regressions.md` (R88) para não passar por esquecimento.
  const CHROME = [
    'web/src/components/Shell.tsx',
    'web/src/components/CommandPalette.tsx',
    'web/src/pages/Home.tsx',
    'web/src/pages/Recordings.tsx',
    'web/src/pages/Directory.tsx',
    'web/src/pages/Room.tsx',
    'web/src/room/RemoteTile.tsx',
    'web/src/pages/Lobby.tsx',
    'web/src/components/MfaPanel.tsx',
    'web/src/components/OnboardingTour.tsx',
    'web/src/App.tsx',
  ]
  for (const f of CHROME) {
    it(`${f.split('/').pop()} não tem glifos dentro de elementos`, () => {
      // O `{1,4}` que aqui estava deixava passar o caso MAIS comum: um glifo
      // seguido do rótulo. `>📊 Sondagens<` tem mais de 4 caracteres e escapava
      // — o portão dava verde com o defeito à frente. Apanhado a 2026-09-03 ao
      // tentar vê-lo falhar de propósito, que é a única forma de saber que um
      // portão guarda alguma coisa (R71).
      const dentroDeElemento = [...read(f).matchAll(/>\s*([^<>{}\n]{1,120})\s*</g)]
        .map((m) => m[1])
        .filter((txt) => EMOJI.test(txt) && !TECLAS.test(txt))
      expect(dentroDeElemento).toEqual([])
    })
  }
})

describe('3.2.6 · a identidade é nossa, não emprestada', () => {
  // A folha trazia `#ea4335` com o comentário «vermelho Meet exato» — a cor de
  // marca da Google, copiada e usada no botão de desligar, no microfone
  // silenciado e no ponto de gravação. Contraria o §37 do mandato e não trazia
  // nada: o `--danger` da casa já existia e é o que vencia na cascata.
  //
  // Guarda-se a PALETA DE MARCA dos concorrentes, não «cores literais» em
  // geral — a folha tem centenas delas e proibi-las todas de uma vez seria um
  // portão que ninguém consegue pôr verde.
  const ALHEIAS: Record<string, string> = {
    '#ea4335': 'vermelho Google', '#4285f4': 'azul Google',
    '#34a853': 'verde Google', '#fbbc05': 'amarelo Google',
    '#6264a7': 'roxo Teams', '#2d8cff': 'azul Zoom',
  }
  it('a folha de estilos não usa cores de marca alheias', () => {
    const css = read('web/src/styles.scss')
      .split('\n')
      .filter((l) => !l.trimStart().startsWith('/*') && !l.trimStart().startsWith('*'))
      .join('\n')
      .toLowerCase()
    const achadas = Object.keys(ALHEIAS).filter((c) => css.includes(c))
    expect(achadas.map((c) => `${c} (${ALHEIAS[c]})`)).toEqual([])
  })
})

describe('práticas de estado do delonix-portal', () => {
  it('a camada de API expõe as três guardas', () => {
    const s = read('web/src/api.ts')
    for (const g of ['export class ApiError', 'export function isAbort', 'export function isAuthFailure', 'export function apiErrorMessage']) {
      expect(s).toContain(g)
    }
  })

  it('o hook de carregamento usa AbortController e guarda o aborto', () => {
    const s = read('web/src/components/AsyncSection.tsx')
    expect(s).toContain('new AbortController()')
    expect(s).toContain('return () => ctrl.abort()')
    expect(s).toContain('if (isAbort(e)) return')
  })

  it('nenhum catch de um pedido com sinal engole o erro em silêncio', () => {
    // O `.catch(() => {})` do Shell escondia até a resposta que dizia que a
    // pessoa É admin. Agora ou trata, ou deixa rasto.
    const s = read('web/src/components/Shell.tsx')
    expect(s).toContain('myOrgs(ctrl.signal)')
    expect(s).not.toMatch(/myOrgs\(\)[\s\S]{0,120}catch\(\(\) => \{\}\)/)
  })

  it('o estado de servidor é uma máquina de três estados, não um booleano', () => {
    const s = read('web/src/components/AsyncSection.tsx')
    expect(s).toMatch(/\{ s: 'loading' \}/)
    expect(s).toMatch(/\{ s: 'ready'; d: T \}/)
    expect(s).toMatch(/\{ s: 'error'; msg: string \}/)
  })
})

describe('3.2.8 · uma marca só, e que respeita quem renomeia', () => {
  // Havia duas marcas: o globo de `/logo.svg` em cinco ecrãs, e um quadrado
  // com a inicial no rail da consola. Com o nome de origem isso é incoerência.
  //
  // O defeito a SÉRIO aparece ao renomear a aplicação: o quadrado adapta-se, os
  // cinco ecrãs continuavam a mostrar o globo Delonix. A marca-branca estava
  // feita a meio, e quem a usasse via o logótipo de OUTRA empresa em metade do
  // produto (R100).
  it('ninguém desenha /logo.svg à mão — passa tudo pelo BrandMark', () => {
    const fixos: string[] = []
    for (const f of [
      'web/src/pages/Status.tsx', 'web/src/pages/Legal.tsx', 'web/src/pages/Lobby.tsx',
      'web/src/pages/Landing.tsx', 'web/src/pages/ApiDocs.tsx', 'web/src/components/Shell.tsx',
    ]) {
      if (read(f).includes('/logo.svg')) fixos.push(f)
    }
    expect(fixos).toEqual([])
  })

  it('o BrandMark decide pelo NOME, não por uma constante', () => {
    const src = read('web/src/components/BrandMark.tsx')
    // Sem esta ligação, o componente seria só um invólucro do logótipo e o
    // defeito da marca-branca continuaria de pé, agora escondido atrás de um
    // nome tranquilizador.
    expect(src).toContain('isMarcaDeOrigem')
    expect(src).toContain('/logo.svg')
    expect(src).toContain('brand-square')
  })
})
