/**
 * Fitness functions do lote 2 (docs/ux-perf-review.md) e das práticas de estado
 * trazidas do `delonix-portal`.
 */
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
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
  // `⌘` e `⌥` são NOMES DE TECLAS dentro de <kbd> — conteúdo, não controlo.
  const TECLAS = /[\u2318\u2325\u21E7\u23CE]/u
  // A REGRA VEM DO PORQUÊ, não de um intervalo escolhido de cabeça. O que o
  // `icons.tsx` diz desde sempre: um emoji renderiza DIFERENTE por sistema
  // operativo, é COLORIDO, e NÃO herda `currentColor` — fica com a sua cor
  // sobre um botão que muda de cor.
  //
  // Isso é verdade dos PICTOGRAMAS (🔒 📞 🤖 💾 🏆). Não é verdade das setas
  // (← → ↑ ↓ ↖), do ✓, do ● nem do ⧉: são tipografia, herdam a cor, e
  // renderizam igual em todo o lado. A segunda versão deste portão metia-os no
  // mesmo saco e acusava 56 sítios, dos quais a maioria eram setas em prosa e
  // indicadores de banda — um portão que grita por tudo é ignorado tal como um
  // portão cego.
  const EMOJI = /[\u{1F300}-\u{1FAFF}\u{FE0F}]/u

  // TERCEIRA versão deste portão, e as duas anteriores davam falsa segurança.
  //
  //   1ª: `>\s*(.{1,4})\s*<` — só apanhava glifos SOZINHOS entre tags. O caso
  //       comum é o glifo SEGUIDO do rótulo, e escapava (R88).
  //   2ª: `{1,120}` — melhor, mas a classe `[^<>{}\n]` exclui `{`, por isso
  //       `⧉ {t('…')}` — emoji ao lado de uma EXPRESSÃO — continuava invisível.
  //       Vinte e um escaparam assim.
  //
  // A lição: um portão construído sobre «que forma tem o defeito que EU vi» erra
  // na forma seguinte. Este procura o glifo em QUALQUER linha de marcação, e
  // deixa de fora o que é conteúdo por razão escrita — não por acidente de
  // sintaxe.
  // Os emoji do PAINEL DE REACÇÕES e do teclado de chat são conteúdo: é a
  // pessoa que os escolhe e envia. Ficam de fora — mas SÓ eles.
  //
  // QUARTA versão, e a terceira tinha o pior tipo de defeito: um portão que
  // dava verde por cima de 223 linhas. A regra era «a partir de uma linha que
  // mencione REACTION_EMOJIS, ignora até um `]`», e a linha
  // `{REACTION_EMOJIS.map((e) => (` está a meio do JSX da barra — o `]` que a
  // fechava só aparecia 223 linhas abaixo. Toda a barra de controlo ficava
  // fora do portão, com dois emoji lá dentro.
  //
  // A lição repete-se pela quarta vez: uma regra construída sobre a FORMA que
  // o defeito tinha da última vez erra na forma seguinte. Esta não adivinha
  // onde o conteúdo acaba — a isenção vale para a linha que nomeia a constante
  // (uma linha, nunca um intervalo) e para o corpo das DECLARAÇÕES, delimitado
  // por contagem de parênteses rectos a partir do `const X = [`.
  const CONTEUDO = /REACTION_EMOJIS|CHAT_EMOJIS/
  const DECLARACAO = /const (REACTION_EMOJIS|CHAT_EMOJIS)\s*=/
  function linhasDeConteudo(texto: string): Set<number> {
    const fora = new Set<number>()
    const linhas = texto.split('\n')
    for (let i = 0; i < linhas.length; i++) {
      if (CONTEUDO.test(linhas[i])) fora.add(i)
      if (!DECLARACAO.test(linhas[i])) continue
      let nivel = 0
      for (let j = i; j < linhas.length; j++) {
        fora.add(j)
        for (const ch of linhas[j]) {
          if (ch === '[') nivel++
          else if (ch === ']') nivel--
        }
        if (nivel <= 0 && j > i - 1 && linhas[j].includes(']')) break
      }
    }
    return fora
  }
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
    'web/src/pages/Landing.tsx',
    'web/src/pages/Analytics.tsx',
    'web/src/pages/Roadmap.tsx',
    'web/src/pages/ApiDocs.tsx',
    'web/src/pages/Studio.tsx',
  ]
  for (const f of CHROME) {
    it(`${f.split('/').pop()} não usa emoji como iconografia`, () => {
      const soltos: string[] = []
      const texto = read(f)
      const conteudo = linhasDeConteudo(texto)
      const linhas = texto.split('\n')
      for (let n = 0; n < linhas.length; n++) {
        const l = linhas[n]
        if (conteudo.has(n)) continue
        const t = l.trim()
        if (t.startsWith('//') || t.startsWith('*') || t.startsWith('/*')) continue
        // comentário JSX (`{/* … */}`) e comentário no fim de uma linha de
        // código não são interface.
        if (t.includes('{/*') || /\S\s+\/\/ /.test(l)) continue
        // só linhas de MARCAÇÃO: com tag ou com chaveta de expressão
        if (!/[<>]/.test(l) && !/\{/.test(l)) continue
        if (EMOJI.test(l) && !TECLAS.test(l)) soltos.push(t.slice(0, 70))
      }
      expect(soltos).toEqual([])
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

  it('o Room.tsx não tem literais visíveis fora do t()', () => {
    const src = read('web/src/pages/Room.tsx')
    const soltos: string[] = []
    for (const m of src.matchAll(/>\s*([A-ZÀ-Ú][^<>{}\n]{3,300})\s*</g)) {
      const v = m[1].trim()
      // `Promise` e afins aparecem em tipos e comentários de código, não na
      // interface: exige-se uma palavra com letras minúsculas acentuadas ou
      // um espaço, que é o que distingue uma frase de um identificador.
      if (/[a-zà-ú]{3}/.test(v) && /\s/.test(v)) soltos.push(v)
    }
    for (const attr of ['title', 'placeholder', 'aria-label']) {
      for (const m of src.matchAll(new RegExp(`${attr}="([A-ZÀ-Ú][^"]{3,90})"`, 'g'))) {
        soltos.push(`${attr}="${m[1]}"`)
      }
    }
    expect(soltos).toEqual([])
  })

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
    'X-Delonix-Signature: sha256=…',   // um header HTTP não tem tradução
  ]
  /** Tira as expressões `{…}` de um nó de texto, respeitando o encaixe. */
  function semExpressoes(txt: string): string {
    let out = ''
    let nivel = 0
    for (const c of txt) {
      if (c === '{') nivel++
      else if (c === '}') { if (nivel > 0) nivel-- }
      else if (nivel === 0) out += c
    }
    return out
  }
  /** Uma FRASE lê-se; um identificador não. É isto que distingue as duas. */
  const eFrase = (v: string) => /\s/.test(v) && /[a-zà-ú]{3}/.test(v) && !NOMES.includes(v)

  it('nenhum nó de texto MISTURADO com expressões escapa ao t()', () => {
    // O portão dos nós de texto usava `[^<>{}\n]` — a classe exclui `{`, por
    // isso um nó como `Notas AI {transcribing && <span/>}` ou
    // `A IA segmenta-te localmente… {bgBusy ? T() : ''}` era invisível. São
    // dezasseis, e é a MESMA falha de sempre: a regra desenhada para a forma
    // que o defeito tinha da última vez. Aqui a expressão é retirada e o que
    // sobra é julgado como prosa.
    const soltos: string[] = []
    for (const f of listarTsx('web/src')) {
      for (const m of read(f).matchAll(/>([A-ZÀ-Ú][^<>]{3,400})</g)) {
        const prosa = semExpressoes(m[1]).replace(/\s+/g, ' ').trim()
        if (prosa.length >= 8 && eFrase(prosa)) soltos.push(`${f}: ${prosa}`)
      }
    }
    expect(soltos).toEqual([])
  })

  it('nenhum ATRIBUTO leva uma frase escrita à mão', () => {
    // A versão anterior verificava três atributos por nome: `title`,
    // `placeholder`, `aria-label`. Mas quem escreve um componente inventa os
    // seus: `label=`, `desc=`, `data-tip=` — e todos acabam no ecrã ou no
    // leitor. Onze escaparam assim, incluindo o rótulo de leitor de ecrã de
    // cinco botões da barra. A regra deixou de nomear atributos.
    const soltos: string[] = []
    for (const f of listarTsx('web/src')) {
      for (const m of read(f).matchAll(/\b([a-zA-Z-]+)="([A-ZÀ-Ú][^"]{3,300})"/g)) {
        if (eFrase(m[2])) soltos.push(`${f}: ${m[1]}="${m[2]}"`)
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
  it('nenhum ficheiro tem texto de interface fora do t()', () => {
    // Marca e identificadores técnicos NÃO são texto traduzível: o nome de uma
    // marca configura-se (R100/R101) e um cabeçalho HTTP é o que é.
    const MARCA_OU_CODIGO = /Delonix|X-Delonix|sha256|curl|NFS|WebDAV|Análises →/
    const soltos: string[] = []
    for (const f of listarTsx('web/src')) {
      if (f.includes('/locales/')) continue
      const src = read(f)
      // O tecto era 80 caracteres e deixava passar as frases MAIS LONGAS — que
      // são as explicativas, precisamente as que um utilizador não-lusófono
      // mais precisa. Três escaparam assim. Mesmo tipo de buraco que o `{1,4}`
      // do portão de emoji (R88): um limite escolhido de cabeça, sem medir o
      // que fica de fora.
      for (const m of src.matchAll(/>\s*([A-ZÀ-Ú][^<>{}\n]{3,300})\s*</g)) {
        const v = m[1].trim()
        if (/[a-zà-ú]{3}/.test(v) && /\s/.test(v) && !MARCA_OU_CODIGO.test(v)) {
          soltos.push(`${f}: ${v}`)
        }
      }
      for (const attr of ['title', 'placeholder', 'aria-label']) {
        for (const m of src.matchAll(new RegExp(`${attr}="([A-ZÀ-Ú][^"]{3,90})"`, 'g'))) {
          if (!MARCA_OU_CODIGO.test(m[1])) soltos.push(`${f} [${attr}]: ${m[1]}`)
        }
      }
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

  it('pt, en e fr têm exactamente as mesmas chaves em `room`', () => {
    const chaves = (f: string) => {
      const s = read(`web/src/locales/${f}.ts`)
      const i = s.indexOf('  room: {')
      expect(i, `${f}.ts não tem bloco room`).toBeGreaterThan(-1)
      const bloco = s.slice(i, s.indexOf('\n  },', i))
      return [...bloco.matchAll(/^\s{6}(\w+):/gm)].map((m) => m[1]).sort()
    }
    const pt = chaves('pt')
    expect(pt.length).toBeGreaterThan(100)
    expect(chaves('en')).toEqual(pt)
    expect(chaves('fr')).toEqual(pt)
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
