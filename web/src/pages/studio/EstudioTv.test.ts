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
import { ECRAS_TV, type EcraTv } from '../../studio/tv/ecras'

// O registo de fontes lê as correcções guardadas ao construir-se: em Node não
// há armazenamento.
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} })
// O registo agenda um quadro no construtor (é ele que desenha as miniaturas).
// Em Node não há `requestAnimationFrame`, e sem isto o construtor estoura antes
// de o ecrã chegar a desenhar.
vi.stubGlobal('requestAnimationFrame', () => 0)
vi.stubGlobal('cancelAnimationFrame', () => undefined)

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
    mesa: MESA_INICIAL,
    mesaRef: { current: MESA_INICIAL },
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

function desenhar(ecra: EcraTv | null) {
  // O contexto real tem campos que só o Estúdio inteiro sabe construir (o
  // `Palco` é um `ReturnType` de um hook); os ecrãs leem destes.
  const props = { ecra, ...contexto() } as unknown as Parameters<typeof EstudioTv>[0]
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

/** O valor de `a.b.c` noutro dicionário, ou `undefined` se faltar. */
function procurar(dict: Record<string, unknown>, chave: string): unknown {
  return chave.split('.').reduce<unknown>((o, k) => (o && typeof o === 'object' ? (o as Record<string, unknown>)[k] : undefined), dict)
}

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
