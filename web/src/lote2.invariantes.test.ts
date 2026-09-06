/**
 * Fitness functions do lote 2 (docs/ux-perf-review.md) e das práticas de estado
 * trazidas do `delonix-portal`.
 */
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import ts from 'typescript'
import { describe, expect, it } from 'vitest'

const root = join(__dirname, '..', '..')
const read = (p: string) => readFileSync(join(root, p), 'utf8')
const css = () => read('web/src/styles.scss')

/** Todos os `.tsx` sob um directório, recursivamente. A lista é DERIVADA da
 *  árvore e não escrita à mão: uma lista à mão fica desactualizada no dia em
 *  que alguém acrescenta um ficheiro, e o portão passa a proteger menos do que
 *  diz. */
function listarTsx(dir: string): string[] {
  const out: string[] = []
  for (const e of readdirSync(join(root, dir), { withFileTypes: true })) {
    const p = `${dir}/${e.name}`
    if (e.isDirectory()) out.push(...listarTsx(p))
    else if (e.name.endsWith('.tsx')) out.push(p)
  }
  return out
}

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

  // A escolha original foi entre BARRA e GAVETA — o corpo da página nunca
  // esteve em cima da mesa. No telemóvel isso deixava a acção principal do
  // produto atrás de um toque no menu, num ecrã com metade da altura vazia
  // (R103). O mesmo componente passou a viver também na Home.
  it('a Home mostra as ações onde a barra não as tem', () => {
    const s = css()
    // Em ecrã largo a barra já as tem: mostrá-las na Home seria a duplicação
    // que a decisão original evitou.
    expect(s).toContain('.qa-home { display: none; }')
    // E abaixo dos 900px — o MESMO limiar em que a barra as passa à gaveta —
    // aparecem no corpo. Sem esta metade, ficariam escondidas nas duas larguras.
    const mobile = s.slice(s.indexOf('@media (max-width: 900px)'))
    expect(mobile).toMatch(/\.qa-home \{\s*display: flex;/)
    expect(read('web/src/pages/Home.tsx')).toContain('<QuickActions variant="home"')
  })

  it('as ações rápidas são um componente só, usado nos dois sítios', () => {
    const s = read('web/src/components/Shell.tsx')
    expect(s).toContain('function QuickActions(')
    expect(s).toContain('<QuickActions variant="bar"')
    expect(s).toContain('variant="drawer"')
  })
})

describe('3.2.5 · nada de emoji como controlo na consola', () => {
  // A REGRA VEM DO PORQUÊ. Um emoji renderiza DIFERENTE por sistema operativo,
  // é colorido, e NÃO herda `currentColor` — fica com a sua cor sobre um botão
  // que muda de cor. Isso é verdade dos PICTOGRAMAS (🔒 📞 🤖 💾 🏆). Não é
  // verdade das setas (← → ↑ ↓), do ✓, do ● nem do ⧉: são tipografia, herdam a
  // cor, e renderizam igual em todo o lado. Uma versão anterior metia-os no
  // mesmo saco e acusava 56 sítios, a maioria setas em prosa — um portão que
  // grita por tudo é ignorado tal como um portão cego.
  const EMOJI = /[\u{1F300}-\u{1FAFF}\u{FE0F}]/u
  // `⌘` e `⌥` são NOMES DE TECLAS dentro de <kbd> — conteúdo, não controlo.
  const TECLAS = /[\u2318\u2325\u21E7\u23CE]/u

  // ── QUINTA versão, e a primeira com parser (R113) ────────────────────────
  //
  // As quatro anteriores foram linha-a-linha, e cada uma comprou um problema:
  //
  //   1ª: `>\s*(.{1,4})\s*<` — só via glifos SOZINHOS entre tags (R88).
  //   2ª: `{1,120}` com `[^<>{}\n]` — a classe exclui `{`, e `⧉ {t('…')}`
  //       ficava invisível. Vinte e um escaparam assim.
  //   3ª/4ª: a isenção dos emoji de reacção começava numa linha que os
  //       mencionasse e acabava no `]` seguinte. Só que
  //       `{REACTION_EMOJIS.map((e) => (` também a activa, e o `]` que a fechava
  //       aparecia 223 linhas abaixo: a barra de controlo inteira ficou fora do
  //       portão, com dois emoji lá dentro (R105).
  //
  // Havia ainda uma lista de 16 ficheiros escrita à mão — uma página nova
  // nascia sem portão nenhum — e uma exigência de a linha ter `<`, `>` ou `{`,
  // que deixava passar um nó de texto sozinho na sua linha.
  //
  // Com o parser nada disto é preciso. Um `JsxText` é texto que aparece no
  // ecrã; um emoji ESCOLHIDO por quem usa a app chega como `JsxExpression`
  // (`{e}` dentro do `.map`) e nunca como `JsxText` — a isenção deixa de ser
  // uma heurística e passa a ser uma consequência da forma do programa.
  const VISIVEIS =
    /^(title|placeholder|alt|label|desc|caption|subtitle|summary|tooltip|hint|message|data-tip|aria-.*)$/
  it('nenhum JSX usa emoji como iconografia', () => {
    const soltos: string[] = []
    for (const f of listarTsx('web/src')) {
      if (f.includes('/locales/')) continue
      const sf = ts.createSourceFile(f, read(f), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
      const visitar = (n: ts.Node) => {
        if (ts.isJsxText(n)) {
          const v = n.text.replace(/\s+/g, ' ').trim()
          if (EMOJI.test(v) && !TECLAS.test(v)) soltos.push(`${f} [texto]: ${v.slice(0, 60)}`)
        }
        if (ts.isJsxAttribute(n) && n.initializer && VISIVEIS.test(n.name.getText())) {
          const ini = n.initializer
          const lit = ts.isStringLiteral(ini)
            ? ini
            : ts.isJsxExpression(ini) && ini.expression && ts.isStringLiteral(ini.expression)
              ? ini.expression
              : null
          if (lit && EMOJI.test(lit.text) && !TECLAS.test(lit.text)) {
            soltos.push(`${f} [${n.name.getText()}]: ${lit.text.slice(0, 60)}`)
          }
        }
        ts.forEachChild(n, visitar)
      }
      visitar(sf)
    }
    expect(soltos).toEqual([])
  })

  it('e os locales também não — um emoji não se resolve mudando-o de ficheiro', () => {
    // Aconteceu no R112: ao mover frases para os locales, um `🔌` foi com elas,
    // e o portão só olhava para `.tsx`. Passar um problema para onde o portão
    // não olha não é resolvê-lo.
    //
    // A FRONTEIRA aqui é entre ICONOGRAFIA e PROSA, e é o porquê que a traça:
    // o problema do emoji é ficar no LUGAR DE UM ÍCONE — no início de um
    // rótulo, num botão, com a sua cor própria sobre um fundo que muda. Um
    // emoji DENTRO de uma frase é outra coisa: ou é tom («Tudo pronto! 🎉»), ou
    // aponta para um glifo que o próprio browser desenha e que a pessoa tem de
    // encontrar («clica no cadeado 🔒 na barra de endereço») — e aí trocá-lo por
    // um ícone nosso tornaria a frase MENOS útil, não mais.
    //
    // Por isso não há regra de posição a adivinhar: há uma lista, com a razão
    // ao lado, e ela é curta de propósito. Se crescer, é sinal de que voltou a
    // entrar iconografia por aqui.
    const PROSA = [
      'cadeado 🔒',       // aponta para o cadeado que o BROWSER desenha
      'Meet 👋',          // tom, na primeira frase do tour
      'pronto! 🎉',       // tom, no fim do tour
      'período. 🎉',      // tom, num estado vazio que é boa notícia
      "all set! 🎉", 'padlock 🔒', 'period. 🎉',
      'cadenas 🔒', 'prêt ! 🎉', 'période. 🎉',
    ]
    const soltos: string[] = []
    for (const loc of ['pt', 'en', 'fr']) {
      for (const l of read(`web/src/locales/${loc}.ts`).split('\n')) {
        if (l.trim().startsWith('//')) continue
        if (!EMOJI.test(l)) continue
        if (PROSA.some((p) => l.includes(p))) continue
        soltos.push(`${loc}: ${l.trim().slice(0, 70)}`)
      }
    }
    expect(soltos).toEqual([])
  })
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

describe('3.2.7 · a sala fala os três idiomas', () => {
  // O `Room.tsx` — 4 300 linhas, o ecrã principal do produto — estava
  // INTEIRAMENTE fora do i18n: zero chamadas a `t()`. Não era uma tradução
  // incompleta; era uma sala que só existia em português (R99).
  //
  // Este portão guarda duas coisas, e as duas já falharam:
  //   · não voltam a entrar literais visíveis sem passar pelo `t()`;
  //   · os três locales têm as MESMAS chaves — uma chave só em `pt` mostra-se
  //     ao utilizador inglês como o identificador cru, que é pior do que a
  //     frase em português.

  // Nomes próprios: não se traduzem, e ficam de fora UM A UM com razão escrita.
  // Nunca por a regra ser afrouxada — uma regra afrouxada deixa passar a frase
  // seguinte, que já não é um nome.
  const NOMES = [
    'Microsoft Teams',                 // marca, na matriz competitiva
    'Google Meet',                     // idem
    'API / Signaling',                 // nomes dos serviços na página de estado
    'Delonix Meet',                    // o nome do produto
    'TrueNAS / NFS',                   // nomes de tecnologia, na escolha de armazenamento
    'Nextcloud / WebDAV',              // idem
    'Delonix Call Quality Score',      // nome da métrica, como o «MOS» de que descende
    'Delonix Call Quality Score: /100',  // o mesmo nome, com o valor interpolado
    'X-Delonix-Signature: sha256=…',   // um header HTTP não tem tradução
  ]
  /** Uma FRASE lê-se; um identificador não. É isto que distingue as duas. */
  const eFrase = (v: string) => /\s/.test(v) && /[a-zà-ú]{3}/.test(v) && !NOMES.includes(v)

  it('nenhum TEMPLATE LITERAL leva uma frase escrita à mão', () => {
    // A busca por literais olhava para `'…'`. Uma frase com um valor lá dentro
    // escreve-se com crases — `` `Quadro branco partilhado por ${m.by}` `` — e
    // essas nunca foram vistas. Doze assim, incluindo o aviso de reconexão que
    // uma pessoa lê justamente quando a rede está má.
    //
    // A interpolação é retirada antes de julgar: o que interessa é a prosa à
    // volta dela, e é ela que um utilizador francês não lê.
    const soltos: string[] = []
    for (const f of listarTsx('web/src')) {
      for (const l of read(f).split('\n')) {
        const semComentario = l.replace(/\/\/.*$/, '')
        for (const m of semComentario.matchAll(/`([^`]{4,300})`/g)) {
          const prosa = m[1].replace(/\$\{[^{}]*\}/g, '').replace(/\s+/g, ' ').trim()
          if (!/^[A-ZÀ-Ú]/.test(prosa)) continue
          if (eFrase(prosa)) soltos.push(`${f}: ${prosa}`)
        }
      }
    }
    expect(soltos).toEqual([])
  })

  it('nenhum ficheiro tem frases visíveis dentro de expressões', () => {
    // O portão de cima olha para NÓS DE TEXTO (`>frase<`) e três atributos. Uma
    // frase dentro de uma expressão — `{cond ? 'Ligar câmara' : 'Desligar'}`,
    // `title={x ? 'A' : 'B'}`, `setStatus('Foste removido da reunião')` — nunca
    // passou por ele. Medido quando se descobriu: 64 frases só no `Room.tsx` e
    // mais 21 no resto da árvore, entre elas avisos de leitor de ecrã, títulos
    // de todos os botões da barra e mensagens de erro. O «zero texto fora do
    // t()» do R102 era verdade só para nós de texto.
    //
    // A regra distingue FRASE de identificador pelo que uma pessoa lê: começa
    // por maiúscula, tem pelo menos um espaço, e tem uma palavra de três letras
    // minúsculas. Isso deixa de fora `'grid'`, `'room-topo'`, `'POST'` e os
    // nomes de eventos, sem precisar de saber onde cada literal é usado.
    //
    // NOMES PRÓPRIOS ficam de fora com razão escrita, um a um — nunca por a
    // regra ser afrouxada. Um nome de produto não se traduz; uma frase sim.
    const soltos: string[] = []
    for (const f of listarTsx('web/src')) {
      const src = read(f)
        // as próprias chamadas ao t() contêm literais — e são a solução, não o
        // problema; saem antes de procurar.
        .replace(/\bt\(\s*'[^']*'(\s*,\s*'[^']*')?\s*\)/g, 'T()')
        .replace(/\/\/[^\n]*/g, '')
        .replace(/\/\*[\s\S]*?\*\//g, '')
      for (const m of src.matchAll(/'([A-ZÀ-Ú][^'\\\n]{3,300})'/g)) {
        const v = m[1]
        if (!eFrase(v)) continue
        soltos.push(`${f}: ${v}`)
      }
    }
    expect(soltos).toEqual([])
  })

  // A sala foi a primeira, mas o resto do produto tinha os mesmos 47 (R102).
  // O portão passou a cobrir `web/src` INTEIRO — a alternativa era voltar a
  // acrescentar ficheiros à lista um a um, e é assim que uma lista fica
  // desactualizada sem ninguém dar por ela.
  // ── A SÉTIMA versão, e a primeira que não é uma expressão regular ──────
  //
  // Seis gerações de regex, seis famílias de fuga: o glifo sozinho (R88), a
  // sala inteira sem `t()` (R99), o tecto de 80 caracteres e os atributos
  // nomeados um a um (R105/R107), o nó de texto misturado com uma expressão, e
  // as crases (R110). De cada vez a regra nova apanhava a forma anterior e
  // falhava na seguinte.
  //
  // A causa é sempre a mesma: uma expressão regular não sabe o que é JSX. Sabe
  // o que é `>` e `<`, e por isso confunde um genérico `useState<Foo>` com uma
  // tag, e não distingue `className` de `aria-label`. Medido: sem a exigência
  // de maiúscula inicial — que só existia para calar esse ruído — a regex
  // acusava 400 sítios, quase todos código.
  //
  // Isto usa o PARSER do TypeScript. Um `JsxText` é texto que aparece no ecrã,
  // por definição; um `JsxAttribute` tem um nome que se pode ler. Não há
  // heurística nenhuma sobre a forma da linha, e por isso não há forma seguinte
  // por onde fugir.
  //
  // O que a lista de atributos faz é o contrário do que fazia: nomeia OS QUE
  // CHEGAM a uma pessoa, em vez dos que não chegam. Um atributo novo —
  // `data-tip`, `desc`, `hint` — entrava em silêncio na versão antiga e nunca
  // entra nesta.
  it('nenhum JSX tem texto de interface fora do t() (parser, não regex)', () => {
    // Nomes próprios e fragmentos técnicos, um a um e com razão ao lado.
    const LEDGER = new Set([
      'TrueNAS / NFS', 'Nextcloud / WebDAV',       // nomes de tecnologia
      'kubectl apply',                              // um comando não se traduz
      'Delonix Call Quality Score', 'Delonix Meet', // nomes do produto
      'X-Delonix-Signature: sha256=…',              // um header HTTP é o que é
      'min ·', 'kbps · perda',                      // unidades e separadores
      '1 min', '2 min', '5 min',                    // idem
      '🇵🇹 Português', '🇬🇧 English', '🇪🇸 Español',   // o idioma escreve-se NO idioma
      '🇫🇷 Français', '🇩🇪 Deutsch', '🇮🇹 Italiano',
    ])
    const VISIVEIS =
      /^(title|placeholder|alt|label|desc|caption|subtitle|summary|tooltip|hint|message|data-tip|aria-.*)$/
    const eFrase = (v: string) => /\s/.test(v) && /[a-zà-ú]{3}/.test(v) && !LEDGER.has(v)
    const soltos: string[] = []
    for (const f of listarTsx('web/src')) {
      if (f.includes('/locales/')) continue
      const sf = ts.createSourceFile(f, read(f), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
      const visitar = (n: ts.Node) => {
        if (ts.isJsxText(n)) {
          const v = n.text.replace(/\s+/g, ' ').trim()
          if (v.length >= 4 && eFrase(v)) soltos.push(`${f} [texto]: ${v.slice(0, 70)}`)
        }
        if (ts.isJsxAttribute(n) && n.initializer && VISIVEIS.test(n.name.getText())) {
          const ini = n.initializer
          const lit = ts.isStringLiteral(ini)
            ? ini
            : ts.isJsxExpression(ini) && ini.expression && ts.isStringLiteral(ini.expression)
              ? ini.expression
              : null
          if (lit && eFrase(lit.text)) soltos.push(`${f} [${n.name.getText()}]: ${lit.text.slice(0, 70)}`)
        }
        ts.forEachChild(n, visitar)
      }
      visitar(sf)
    }
    expect(soltos).toEqual([])
  })

  // O PONTO CEGO DOS DOIS PORTÕES ANTERIORES (R104): olhavam para JSX — texto
  // entre tags e atributos — e não para strings passadas a FUNÇÕES. Havia 46
  // mensagens de estado em português fixo (`setStatus('O anfitrião silenciou o
  // teu microfone')`), duas delas com emoji, invisíveis para os dois.
  //
  // Isto importa mais desde que a linha de estado passou a ser anunciada por
  // leitor de ecrã: anunciar português a quem escolheu inglês é pior do que não
  // anunciar.
  it('as mensagens de estado não são literais em português', () => {
    const MARCA_OU_URL = /Delonix|sha256|http|\/api\//
    const soltos: string[] = []
    for (const f of listarTsx('web/src')) {
      if (f.includes('/locales/')) continue
      for (const m of read(f).matchAll(/\b(setStatus|setErr|setError|setMsg)\(\s*'([^']{6,120})'/g)) {
        const v = m[2]
        if (/[a-zà-ú]{3}/.test(v) && /\s/.test(v) && !MARCA_OU_URL.test(v)) soltos.push(`${f}: ${v}`)
      }
    }
    expect(soltos).toEqual([])
  })

  /**
   * Lê um ficheiro de locale para um mapa `caminho.da.chave -> texto`.
   *
   * O portão anterior olhava SÓ para o bloco `room`, e por isso não viu que o
   * francês tinha **20 chaves a menos** — o painel de SSO inteiro e metade das
   * gravações. Uma chave em falta não falha nem avisa: o i18next mostra o
   * identificador cru, e o utilizador francês lê `admin.ssoTitle` no ecrã.
   */
  function mapaDeLocale(loc: string): Record<string, string> {
    const out: Record<string, string> = {}
    const pilha: string[] = []
    for (const linha of read(`web/src/locales/${loc}.ts`).split('\n')) {
      const t = linha.trim()
      const abre = t.match(/^([A-Za-z0-9_]+):\s*\{$/)
      if (abre) { pilha.push(abre[1]); continue }
      if (t.startsWith('}')) { pilha.pop(); continue }
      const par = t.match(/^([A-Za-z0-9_]+):\s*(['"])((?:\\.|(?!\2).)*)\2\s*,?$/)
      if (par) out[[...pilha, par[1]].join('.')] = par[3]
    }
    return out
  }

  it('pt, en e fr têm exactamente as mesmas chaves — em TODOS os blocos', () => {
    const pt = Object.keys(mapaDeLocale('pt')).sort()
    expect(pt.length).toBeGreaterThan(900)
    expect(Object.keys(mapaDeLocale('en')).sort()).toEqual(pt)
    expect(Object.keys(mapaDeLocale('fr')).sort()).toEqual(pt)
  })

  it('nenhuma tradução é uma FRASE onde o português é um rótulo', () => {
    // Encontrado a medir, não a olhar: o francês tinha «🔒 Créer une réunion
    // E2EE (chiffrée de bout en bout, avec phrase secrète)» onde o português
    // tem «Reunião E2EE» — os dois no MESMO botão. Um rótulo que quadruplica de
    // comprimento não cabe onde cabia, e ninguém dá por isso sem abrir a app em
    // francês.
    //
    // O limiar não é uma opinião sobre estilo: 2,2× MAIS 12 caracteres deixa
    // passar a expansão normal do francês e do inglês (que é real e ronda os
    // 20 %) e apanha quem escreveu uma explicação onde devia estar um rótulo.
    const pt = mapaDeLocale('pt')
    const maus: string[] = []
    for (const loc of ['en', 'fr']) {
      const o = mapaDeLocale(loc)
      for (const [k, v] of Object.entries(pt)) {
        const w = o[k]
        if (!w || v.length < 6) continue
        if (w.length > v.length * 2.2 + 12) maus.push(`${k} [${loc}] ${v.length} → ${w.length}: ${w.slice(0, 50)}`)
      }
    }
    expect(maus).toEqual([])
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

  // O símbolo era METADE do problema. O nome continuava escrito à mão ao lado
  // dele — `<BrandMark /> Delonix <span>Meet</span>` — em cinco páginas.
  // Renomear a aplicação trocava o símbolo e deixava o nome antigo colado a
  // ele, que é PIOR do que não ter mudado nada (R101).
  it('o nome da aplicação também não se escreve à mão', () => {
    const fixos: string[] = []
    for (const f of [
      'web/src/pages/Status.tsx', 'web/src/pages/Legal.tsx', 'web/src/pages/Lobby.tsx',
      'web/src/pages/Landing.tsx', 'web/src/pages/ApiDocs.tsx', 'web/src/components/Shell.tsx',
      'web/src/pages/SharePage.tsx',
    ]) {
      // O que se proíbe é o LOCKUP escrito à mão. O nome dentro de uma frase
      // traduzida é outro problema (i18n), e resolve-se por interpolação.
      if (/Delonix\s*<span>/.test(read(f))) fixos.push(f)
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
