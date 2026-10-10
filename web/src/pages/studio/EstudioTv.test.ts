/**
 * Os CINCO ecrãs do estúdio de TV, desenhados para HTML.
 *
 * PORQUE EXISTE. Estes cinco ecrãs nunca foram vistos: a validação no browser
 * de 2026-10-05 anotou-os como «o que falta ver — precisam de câmaras, e o
 * painel do browser bloqueia a captura». Um teste não substitui ver, mas dura
 * mais do que uma observação, e passa a correr em cada commit.
 *
 * Não prova layout nem o clique. Prova que cada um dos cinco DESENHA sem
 * câmara, sem `AudioContext` e sem `canvas` (os efeitos não correm em
 * `renderToStaticMarkup`), que diz algo em cada uma das quatro línguas, e que
 * NENHUMA chave de tradução escapa crua — o defeito que já aconteceu nesta
 * área, quando a folha de atalhos reutilizou os fragmentos `tv.atalhos.*`.
 *
 * A sessão é falsificada, mas com as PEÇAS REAIS onde isso é possível: o
 * `RegistoDeFontes` e a `MESA_INICIAL` são os do produto. O som fica em `null`
 * de propósito — é o estado de arranque, e é o caminho que mostra o «ligar
 * som» em vez de o esconder atrás de uma dobra optimista.
 */
import { createElement as h } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import { SOBREPOSICOES_INICIAIS } from '../../studio/palco'
import { MESA_INICIAL, planoDe } from '../../studio/tv/mesa'
import { ECRAS_TV, type EcraTv } from '../../studio/tv/ecras'

// O registo de fontes lê as correcções guardadas ao construir-se: em Node não
// há armazenamento.
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} })
// O registo agenda um quadro no construtor (é ele que desenha as miniaturas).
// Em Node não há `requestAnimationFrame`, e sem isto o construtor estoura antes
// de o ecrã chegar a desenhar.
vi.stubGlobal('requestAnimationFrame', () => 0)
vi.stubGlobal('cancelAnimationFrame', () => undefined)

// A mesa que a sessão falsa devolve. É uma variável de módulo porque o
// `vi.mock` é içado para o topo: os testes mudam-na ANTES de desenhar, e é
// assim que o segundo estado («no ar») entra sem um segundo mock.
const mesaDaSessao: { actual: unknown } = { actual: null }

vi.mock('./tv/useSessaoTv', async () => {
  const { RegistoDeFontes } = await import('../../studio/tv/fontes')
  const { MESA_INICIAL } = await import('../../studio/tv/mesa')
  const nada = () => undefined
  const sessao = {
    registo: new RegistoDeFontes(),
    versaoFontes: 0,
    camaras: [],
    microfones: [],
    saidas: [],
    procurarDispositivos: async () => undefined,
    ligarCamara: async () => undefined,
    erroFonte: '',
    get mesa() {
      return mesaDaSessao.actual ?? MESA_INICIAL
    },
    get mesaRef() {
      return { current: mesaDaSessao.actual ?? MESA_INICIAL }
    },
    accoes: { previa: nada, ar: nada, cortar: nada, auto: nada, tbar: nada, escolher: nada, duracao: nada },
    sobreposicaoLigada: () => false,
    definirSobreposicao: nada,
    som: null,
    versaoSom: 0,
    ligarSom: async () => null,
    erroSom: '',
    vozLigada: false,
    setVozLigada: nada,
    canaisComFonte: new Set<string>(),
    macros: [],
    progressoMacro: null,
    correrMacroDaTecla: () => false,
  }
  return { useSessaoTv: () => sessao }
})

const { default: i18n } = await import('../../i18n')
const { default: EstudioTv } = await import('./EstudioTv')
const { ShellCtx } = await import('../../components/shellContext')

// `setLanguage` mexe no `document`; aqui carregam-se os dicionários à mão.
const DICTS = {
  'pt-AO': null,
  en: () => import('../../locales/en'),
  'fr-FR': () => import('../../locales/fr'),
  'zh-CN': () => import('../../locales/zh'),
} as const
type Lng = keyof typeof DICTS
async function usar(lng: Lng) {
  const load = DICTS[lng]
  if (load && !i18n.hasResourceBundle(lng, 'translation')) {
    i18n.addResourceBundle(lng, 'translation', (await load()).default, true, true)
  }
  await i18n.changeLanguage(lng)
}

/** O contexto do Estúdio na forma de arranque: nada a gravar, nada no ar. */
function contexto() {
  return {
    compRef: { current: null },
    canvasHostRef: { current: null },
    // A peça real, não um literal: os ecrãs leem `nome`/`cargo` para decidir
    // se a legenda está disponível, e um `{}` rebentava com «.trim() de
    // undefined» — foi assim que este teste encontrou a primeira coisa.
    palco: { sobreposicoes: SOBREPOSICOES_INICIAIS },
    // Um título que NÃO pode parecer texto do produto: com «Programa de prova»
    // a asserção de tradução acusava «Programa» como português não traduzido —
    // e vinha desta dobra, não do código (medido).
    titulo: 'ZZ-TITULO-DE-PROVA-ZZ',
    temEcra: false,
    participantes: [],
    haSondagem: false,
    gravacao: { estado: 'parado' as const, lerSegundos: () => 0, e4k: false },
    directo: { fase: 'parado' as const },
    destinos: [],
    kbps: 0,
    onPararGravacao: () => undefined,
    onTerminarEmissao: () => undefined,
    onNavegar: () => undefined,
  }
}

/**
 * O MESMO estúdio, mas a emitir: programa no ar, prévia carregada, a gravar e
 * com o rodapé ligado. Metade dos ecrãs só mostra selos, cronómetros e estado
 * de botão NESTE estado — desenhar só o repouso media metade da história.
 */
function contextoNoAr() {
  return {
    ...contexto(),
    haSondagem: true,
    gravacao: { estado: 'a-gravar' as const, lerSegundos: () => 42, e4k: true },
    directo: { fase: 'no-ar' as const, desde: 1_700_000_000_000, bytes: 1234 },
    destinos: [{ nome: 'ZZ-DESTINO-ZZ', url: 'rtmp://exemplo/x', chave: 'k' }],
    kbps: 4500,
    palco: { sobreposicoes: { ...SOBREPOSICOES_INICIAIS, rodape: true, cronometro: true, nome: 'ZZ-NOME-ZZ', cargo: 'ZZ-CARGO-ZZ' } },
  }
}

const MESA_NO_AR = { ...MESA_INICIAL, programa: planoDe('zz-cam-1'), previa: planoDe('zz-cam-2'), noArDesde: 1_700_000_000_000 }

function desenhar(ecra: EcraTv | null, c: object = contexto()) {
  // O contexto real tem campos que só o Estúdio inteiro sabe construir (o
  // `Palco` é um `ReturnType` de um hook); os ecrãs leem destes.
  const props = { ecra, ...c } as unknown as Parameters<typeof EstudioTv>[0]
  // O topo dos ecrãs lê `navOpen`/`setNavOpen` do Shell. Usa-se o Provider REAL
  // com o mínimo que ele pede, em vez de falsificar o `useShell` — assim o
  // teste continua a passar pelo mesmo contexto que a app.
  const shell = { navOpen: false, setNavOpen: () => undefined } as unknown as Parameters<
    typeof ShellCtx.Provider
  >[0]['value']
  return renderToStaticMarkup(h(ShellCtx.Provider, { value: shell }, h(EstudioTv, props)))
}

// As quatro famílias de chaves que estes ecrãs usam. Uma chave crua aparece
// como `tv.algo` no HTML, que é o que isto recusa.
const semChavesCruas = (html: string) => expect(html).not.toMatch(/\b(tv|studio|editor|ui)\.[a-zA-Z]/)

/** As línguas que TÊM dicionário próprio — o pt-AO é o fallback, não se compara. */
type Traduzida = Exclude<Lng, 'pt-AO'>
const DICT_PATH: Record<Traduzida, string> = {
  en: '../../locales/en',
  'fr-FR': '../../locales/fr',
  'zh-CN': '../../locales/zh',
}

/** Os pares `chave.com.pontos` → texto de um ramo do dicionário. */
function folhas(no: unknown, prefixo: string): [string, string][] {
  if (typeof no === 'string') return [[prefixo, no]]
  if (!no || typeof no !== 'object' || Array.isArray(no)) return []
  return Object.entries(no).flatMap(([k, v]) => folhas(v, `${prefixo}.${k}`))
}

/**
 * O que a pessoa LÊ: o texto entre etiquetas, mais os `aria-label` e `title`
 * (que um leitor de ecrã lê em voz alta). Não as classes CSS — o `tv-vazio` e o
 * `tv-mini--vazio` contêm palavras portuguesas que não são texto nenhum, e
 * comparar o HTML cru dava falso positivo (medido: «vazio» de
 * `tv.tally.semFonte`).
 */
function textoVisivel(html: string): string {
  const legendas = [...html.matchAll(/(?:aria-label|title|placeholder)="([^"]*)"/g)].map((m) => m[1])
  return html.replace(/<[^>]*>/g, ' ') + ' ' + legendas.join(' ')
}

/**
 * Controlos SEM nome acessível: nem texto visível, nem `aria-label`, nem
 * `aria-labelledby`, nem `title`. É o defeito nº1 de um leitor de ecrã — ele diz
 * «botão» e mais nada. Um botão com texto visível TEM nome: não é defeito.
 */
export function controlosSemNome(html: string): string[] {
  const maus: string[] = []
  for (const m of html.matchAll(/<(button|a)\b([^>]*)>([\s\S]*?)<\/\1>/g)) {
    const attrs = m[2]
    const texto = m[3].replace(/<[^>]*>/g, '').trim()
    const temNome = /aria-label="[^"]+"/.test(attrs) || /aria-labelledby="[^"]+"/.test(attrs) || /title="[^"]+"/.test(attrs)
    if (!texto && !temNome) maus.push(`<${m[1]} ${attrs.trim().slice(0, 90)}>`)
  }
  return maus
}

/**
 * Botões cujo estado vive só na classe CSS: ligados para quem VÊ e apagados
 * para quem OUVE.
 */
export function estadoSoEmCss(html: string): string[] {
  const maus: string[] = []
  for (const m of html.matchAll(/<button\b([^>]*)>/g)) {
    const a = m[1]
    const classeDeEstado = /class="[^"]*(--on\b|--activ|--ligad|is-on\b|is-activ|--no-ar|--previa|--sel)/.test(a)
    const estadoAria = /aria-(pressed|checked|current|selected|expanded)="/.test(a)
    if (classeDeEstado && !estadoAria) maus.push((/class="([^"]*)"/.exec(a) ?? ['', '?'])[1])
  }
  return maus
}

/** Quantos controlos o `controlosSemNome` chegou a ver. */
export function quantosControlos(html: string): number {
  return [...html.matchAll(/<(button|a)\b[^>]*>[\s\S]*?<\/\1>/g)].length
}

/** O valor de `a.b.c` noutro dicionário, ou `undefined` se faltar. */
function procurar(dict: Record<string, unknown>, chave: string): unknown {
  return chave.split('.').reduce<unknown>((o, k) => (o && typeof o === 'object' ? (o as Record<string, unknown>)[k] : undefined), dict)
}

/** Desenha um ecrã num dos dois estados, pondo a mesa certa na sessão falsa. */
function desenharEstado(ecra: EcraTv, estado: 'repouso' | 'no ar') {
  mesaDaSessao.actual = estado === 'no ar' ? MESA_NO_AR : null
  const html = desenhar(ecra, estado === 'no ar' ? contextoNoAr() : contexto())
  mesaDaSessao.actual = null
  return html
}

const OS_DOIS: ('repouso' | 'no ar')[] = ['repouso', 'no ar']

describe('os cinco ecrãs do estúdio de TV', () => {
  it('são exactamente cinco, e o catálogo é a fonte', () => {
    expect([...ECRAS_TV]).toEqual(['mesa-de-corte', 'mesa-de-som', 'iluminacao', 'fontes', 'cena'])
  })

  it('cada um desenha sem câmara, sem som e sem canvas', () => {
    for (const ecra of ECRAS_TV) {
      const html = desenhar(ecra)
      expect(html, `${ecra}: desenhou vazio`).not.toBe('')
      expect(html.length, `${ecra}: desenhou quase nada (${html.length} bytes)`).toBeGreaterThan(200)
    }
  })

  it('`ecra = null` não desenha nada — o canvas volta ao palco do Estúdio', () => {
    expect(desenhar(null)).toBe('')
  })

  it('a emitir, os cinco continuam a desenhar — E mudam', () => {
    for (const ecra of ECRAS_TV) {
      const repouso = desenharEstado(ecra, 'repouso')
      const html = desenharEstado(ecra, 'no ar')
      expect(html, `${ecra} no ar: desenhou vazio`).not.toBe('')
      expect(html.length, `${ecra} no ar: desenhou quase nada`).toBeGreaterThan(200)
      // Sem isto, o segundo estado era decoração: um ecrã que ignorasse o «no
      // ar» passava os testes de acessibilidade abaixo sem os exercitar.
      expect(html, `${ecra}: o estado «no ar» não muda nada no ecrã`).not.toBe(repouso)
    }
  })

  it('todo o controlo tem nome acessível, nos dois estados', () => {
    // O defeito nº1 de um leitor de ecrã é um botão de ícone sem nome: ele diz
    // «botão» e mais nada. Isto NÃO substitui ouvir com um leitor de ecrã —
    // mede a propriedade de que ele depende.
    const semNome: string[] = []
    let vistos = 0
    for (const estado of OS_DOIS) {
      for (const ecra of ECRAS_TV) {
        const html = desenharEstado(ecra, estado)
        vistos += quantosControlos(html)
        semNome.push(...controlosSemNome(html).map((c) => `${ecra} (${estado}): ${c}`))
      }
    }
    expect(semNome, 'controlos sem nome acessível').toEqual([])
    // O piso é MEDIDO (108 controlos nos dois estados, 48 deles por
    // `aria-label`), e existe para a asserção não poder esvaziar-se em
    // silêncio: um ecrã que deixe de desenhar os seus botões passaria o
    // `toEqual([])` sem ter verificado nada.
    expect(vistos, 'poucos controlos verificados — o teste deixou de medir').toBeGreaterThan(90)
  })

  it('nenhum botão guarda o estado só na classe CSS, nos dois estados', () => {
    // Um botão que diz «ligado» por uma classe e não por `aria-pressed` está
    // ligado para quem VÊ e apagado para quem OUVE.
    const maus: string[] = []
    for (const estado of OS_DOIS) {
      for (const ecra of ECRAS_TV) {
        maus.push(...estadoSoEmCss(desenharEstado(ecra, estado)).map((c) => `${ecra} (${estado}): ${c}`))
      }
    }
    expect([...new Set(maus)], 'estado visual sem estado ARIA').toEqual([])
  })

  // Os dois detectores acima passam porque os ecrãs estão bem. Isso, sozinho,
  // não distingue «está bem» de «o detector não detecta» — tentei atacá-los
  // mexendo nos ecrãs e as duas tentativas quebraram a compilação do JSX em vez
  // de fazer o teste falhar. Por isso os detectores levam controlos próprios,
  // com HTML escrito à mão.
  it('o detector de nomes acusa o que deve e perdoa o que deve', () => {
    expect(controlosSemNome('<button class="x"><svg></svg></button>')).toHaveLength(1)
    expect(controlosSemNome('<button aria-label=""><svg></svg></button>')).toHaveLength(1)
    expect(controlosSemNome('<a href="#"><i></i></a>')).toHaveLength(1)
    // E o que NÃO é defeito:
    expect(controlosSemNome('<button>Cortar</button>')).toEqual([])
    expect(controlosSemNome('<button aria-label="Cortar"><svg></svg></button>')).toEqual([])
    expect(controlosSemNome('<button title="Cortar"><svg></svg></button>')).toEqual([])
    expect(controlosSemNome('<button aria-labelledby="t1"><svg></svg></button>')).toEqual([])
  })

  it('o detector de estado acusa o que deve e perdoa o que deve', () => {
    expect(estadoSoEmCss('<button class="tv-bt tv-bt--activo">A</button>')).toEqual(['tv-bt tv-bt--activo'])
    expect(estadoSoEmCss('<button class="x is-on">A</button>')).toHaveLength(1)
    expect(estadoSoEmCss('<button class="tv--no-ar">A</button>')).toHaveLength(1)
    // E o que NÃO é defeito:
    expect(estadoSoEmCss('<button class="tv-bt tv-bt--activo" aria-pressed="true">A</button>')).toEqual([])
    expect(estadoSoEmCss('<button class="x is-on" aria-checked="true">A</button>')).toEqual([])
    expect(estadoSoEmCss('<button class="tv-bt">A</button>')).toEqual([])
    // `--vazio` não é estado de um comando: é a ausência de fonte.
    expect(estadoSoEmCss('<button class="tv-mini--vazio">A</button>')).toEqual([])
  })

  it('nenhuma chave de tradução crua, nas quatro línguas', async () => {
    for (const lng of Object.keys(DICTS) as Lng[]) {
      await usar(lng)
      for (const ecra of ECRAS_TV) {
        const html = desenhar(ecra)
        semChavesCruas(html)
      }
    }
    await usar('pt-AO')
  })

  it('os ecrãs estão REALMENTE traduzidos, chave a chave', async () => {
    // Uma chave em falta cai no português sem erro nenhum (`fallbackLng`), por
    // isso «sem chave crua» não diz nada sobre estar traduzido.
    //
    // A PRIMEIRA versão desta asserção era OCA: comparava o HTML inteiro com o
    // português e exigia que fosse diferente. Como os ecrãs também usam chaves
    // `studio.*` e `ui.*`, bastava uma palavra traduzida em qualquer sítio para
    // passar — esvaziar o `locales/en/tv.ts` COMPLETO não a fazia falhar
    // (medido). Agora é chave a chave: se o dicionário da língua tem um valor
    // DIFERENTE do português para uma chave, o português dessa chave não pode
    // aparecer no HTML dessa língua.
    const PT = (await import('../../locales/pt')).default as Record<string, unknown>
    for (const lng of Object.keys(DICT_PATH) as Traduzida[]) {
      await usar(lng)
      const OUTRO = (await import(DICT_PATH[lng])).default as Record<string, unknown>
      const pares = [...folhas(PT.tv, 'tv'), ...folhas(PT.studio, 'studio')]
      let comparadas = 0
      for (const ecra of ECRAS_TV) {
        const html = textoVisivel(desenhar(ecra))
        for (const [chave, valorPt] of pares) {
          const valorOutro = procurar(OUTRO, chave)
          // Só interessa quando a tradução EXISTE e é diferente: «AUTO», «REC»
          // e os números são iguais em todas as línguas de propósito.
          if (typeof valorOutro !== 'string' || valorOutro === valorPt) continue
          if (valorPt.length < 4) continue // «ao», «de» aparecem dentro de outras palavras
          comparadas++
          // A chave na mensagem é a PRIMEIRA que casa, e o texto pode vir de
          // outra que a contenha («Estúdio» casa dentro de «Palco do Estúdio»).
          // Para encontrar o culpado, procura-se o valor no HTML, não a chave.
          expect(html, `${ecra} em ${lng}: «${valorPt}» (${chave}) ficou em português`).not.toContain(valorPt)
        }
      }
      expect(comparadas, `${lng}: nenhuma chave comparada — o teste não mediu nada`).toBeGreaterThan(50)
    }
    await usar('pt-AO')
  })
})
