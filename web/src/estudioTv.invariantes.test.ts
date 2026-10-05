/**
 * Portão do estúdio de TV: cada chave `tv.*` que os ecrãs usam existe nas
 * QUATRO línguas.
 *
 * Porque existe: uma chave sem tradução não falha, não avisa e não cai para o
 * português — o i18next escreve o IDENTIFICADOR no ecrã. Um botão de emissão
 * que diz «tv.topo.rec» em vez de «REC» é o que um operador vê a três minutos
 * do ar. Foi por isso que estes cinco ecrãs ficaram um ano fora da `main`: o
 * bloco `tv` não existia em dicionário nenhum, e as 298 chaves saíam cruas.
 *
 * O portão NÃO se contenta com as chaves escritas à mão. Oito famílias são
 * construídas com literais de template (`tv.macros.${macro.id}.nome`,
 * `tv.fontes.tipos.${f.tipo}`, …) e uma delas em falta é exactamente igual no
 * ecrã. As enumerações vêm do CÓDIGO — das constantes do motor onde elas
 * existem, e do texto dos tipos onde são uniões — para que acrescentar um tipo
 * de fonte ou uma macro sem a traduzir fique vermelho aqui.
 */
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import en from './locales/en'
import fr from './locales/fr'
import pt from './locales/pt'
import zh from './locales/zh'
import { MACROS_INICIAIS, SOBREPOSICOES_DA_MESA } from './studio/tv/macros'
import { TRANSICOES } from './studio/tv/mesa'
import { EQ_INICIAL } from './studio/tv/som'

const root = join(__dirname, '..', '..')
const read = (p: string) => readFileSync(join(root, p), 'utf8')

/** Os ecrãs do estúdio de TV: a página e tudo o que vive em `pages/studio/tv/`. */
const FICHEIROS = [
  'web/src/pages/studio/EstudioTv.tsx',
  ...readdirSync(join(root, 'web/src/pages/studio/tv'))
    .filter((f) => /\.tsx?$/.test(f))
    .map((f) => `web/src/pages/studio/tv/${f}`),
]

/** Uma união de literais escrita no código — `'a' | 'b' | 'c'`. */
function uniaoDeTexto(fonte: string, padrao: RegExp): string[] {
  const m = fonte.match(padrao)
  expect(m, `a união não foi encontrada: ${padrao}`).toBeTruthy()
  const valores = [...m![1].matchAll(/'([^']+)'/g)].map((x) => x[1])
  expect(valores.length).toBeGreaterThan(1)
  return valores
}

/** `programa | previa | livre` — o que o `tallyDe` da mesa devolve. */
const TALLY = uniaoDeTexto(read('web/src/studio/tv/mesa.ts'), /export function tallyDe\([^)]*\): ((?:'[a-z]+'(?: \| )?)+)/)
const TIPOS_DE_FONTE = uniaoDeTexto(read('web/src/studio/tv/fontes.ts'), /export type TipoDeFonte = ((?:'[a-z]+'(?: \| )?)+)/)
const TIPOS_DE_CANAL = uniaoDeTexto(read('web/src/studio/tv/mesaDeSom.ts'), /export type TipoDeCanal = ((?:'[a-z]+'(?: \| )?)+)/)
const PRESSAO = uniaoDeTexto(read('web/src/pages/studio/tv/CenaCompleta.tsx'), /type Pressao = ((?:'[a-z]+'(?: \| )?)+)/)
/**
 * As razões de um passo indisponível. Não há constante para elas — o executor
 * da macro escreve-as em `indisponivel('…')`, e é dali que se lêem.
 */
const RAZOES = [...read('web/src/pages/studio/tv/useSessaoTv.ts').matchAll(/indisponivel\('([A-Za-z]+)'\)/g)].map((m) => m[1])

/**
 * As oito famílias construídas com template, e a enumeração real de cada uma.
 * A chave do mapa é o texto do template tal como aparece no código, para o
 * portão reclamar de uma família NOVA que ninguém tenha ensinado aqui.
 */
const FAMILIAS: Record<string, string[]> = {
  'tv.tally.${': TALLY,
  'tv.transicoes.${': [...TRANSICOES],
  'tv.sobreposicoes.${': [...SOBREPOSICOES_DA_MESA],
  'tv.fontes.tipos.${': TIPOS_DE_FONTE,
  'tv.fontes.barramento.${': TALLY,
  'tv.som.tipos.${': TIPOS_DE_CANAL,
  'tv.som.bandas.${': EQ_INICIAL.map((_, i) => String(i)),
  'tv.cena.pressao.${': PRESSAO,
  'tv.macros.razoes.${': RAZOES,
  'tv.macros.${': MACROS_INICIAIS.map((m) => m.id),
}

/** Todas as chaves `tv.*` que os ecrãs pedem, já com as famílias expandidas. */
function chavesUsadas(): string[] {
  const chaves = new Set<string>()
  const familiasVistas = new Set<string>()
  for (const f of FICHEIROS) {
    const fonte = read(f)
    // Escritas à mão: t('tv.x.y').
    for (const m of fonte.matchAll(/t\((['"])(tv\.[A-Za-z0-9_.]+)\1/g)) chaves.add(m[2])
    // Construídas: t(`tv.x.${expr}`) e t(`tv.x.${expr}.sufixo`).
    for (const m of fonte.matchAll(/`(tv\.[A-Za-z0-9_.]*)\$\{[^}]*\}([A-Za-z0-9_.]*)`/g)) {
      const prefixo = `${m[1]}$\{`
      const valores = FAMILIAS[prefixo]
      expect(valores, `família de template sem enumeração conhecida: ${m[1]}\${…} em ${f}`).toBeTruthy()
      familiasVistas.add(prefixo)
      for (const v of valores!) chaves.add(`${m[1]}${v}${m[2]}`)
    }
  }
  // Uma família que já ninguém usa é uma linha morta neste ficheiro.
  expect([...Object.keys(FAMILIAS)].filter((k) => !familiasVistas.has(k))).toEqual([])
  return [...chaves].sort()
}

/**
 * `true` se o dicionário resolve a chave. Um plural (`{ count }`) vive em
 * `<chave>_one` e `<chave>_other`, e TEM de ter os dois: só com `_other` o
 * singular cai na chave crua.
 */
function resolve(dicionario: unknown, chave: string): boolean {
  const ler = (k: string) => k.split('.').reduce<unknown>((o, p) => (o && typeof o === 'object' ? (o as Record<string, unknown>)[p] : undefined), dicionario)
  const directa = ler(chave)
  if (typeof directa === 'string') return directa.trim() !== ''
  const um = ler(`${chave}_one`)
  const outros = ler(`${chave}_other`)
  return typeof um === 'string' && um.trim() !== '' && typeof outros === 'string' && outros.trim() !== ''
}

describe('o estúdio de TV fala as quatro línguas', () => {
  it('as chaves lêem-se do código, e são muitas — um extractor que devolve poucas não guarda nada', () => {
    const chaves = chavesUsadas()
    expect(chaves.length).toBeGreaterThan(280)
    expect(chaves).toContain('tv.topo.rec')
    // As famílias expandiram-se de verdade, e com a enumeração completa.
    expect(chaves).toContain('tv.macros.encerrar.nota')
    expect(chaves).toContain('tv.fontes.tipos.quadro')
    expect(chaves).toContain('tv.som.bandas.3')
    expect(chaves).toContain('tv.cena.pressao.critical')
    expect(chaves).toContain('tv.macros.razoes.naoAGravar')
  })

  for (const [lingua, dicionario] of Object.entries({ pt, en, fr, zh })) {
    it(lingua, () => {
      expect(chavesUsadas().filter((k) => !resolve(dicionario, k))).toEqual([])
    })
  }

  it('o dicionário `tv` está registado nas quatro — um ficheiro que ninguém compõe não chega ao i18next', () => {
    for (const loc of ['pt', 'en', 'fr', 'zh']) {
      const index = read(`web/src/locales/${loc}/index.ts`)
      expect(index).toMatch(/import tv from '\.\/tv'/)
      expect(index).toMatch(/export default \{[^}]*\btv\b/)
    }
  })
})

describe('os ecrãs do estúdio de TV são alcançáveis', () => {
  const pagina = () => read('web/src/pages/Studio.tsx')

  it('a página monta-os por `lazy`, e só depois da primeira visita', () => {
    // Um import estático arrastava a mesa de som (Web Audio) e as cinco vistas
    // para quem só quer gravar uma aula.
    expect(pagina()).toContain("const EstudioTv = lazy(() => import('./studio/EstudioTv'))")
    expect(pagina()).not.toMatch(/^import EstudioTv from/m)
    expect(pagina()).toMatch(/\{tvVisitada && \(\s*<Suspense/)
  })

  it('há um botão para a vista `tv` no selector de vistas', () => {
    // O selector é o `Segmented` do kit, que escreve um `data-<chave>` por
    // opção: mede-se a opção AQUI e o atributo no kit, para que o e2e continue
    // a poder agarrar `[data-studio-vista="tv"]` sem depender da língua.
    expect(pagina()).toContain('dataKey="studio-vista"')
    expect(pagina()).toMatch(/value: 'tv', label: t\('studio\.vistas\.tv'\)/)
    expect(read('web/src/ui/kit.tsx')).toContain('[`data-${dataKey}`]: o.value')
  })

  it('e as cinco vistas têm atalho de teclado, com a mesa a ignorá-lo', () => {
    // O que faltava: as legendas e as exportações só se alcançavam de dentro
    // do editor. O portão das combinações vive em `ui/atalhos.test.ts`.
    expect(pagina()).toContain("useAtalhos('estudio', {")
    for (const id of ['vistaEmissao', 'vistaEdicao', 'vistaLegendas', 'vistaExportacoes', 'vistaTv']) {
      expect(read('web/src/ui/atalhos.ts')).toContain(`id: '${id}'`)
    }
  })

  it('e a vista lê-se do endereço, com o ecrã dentro dela', () => {
    // Sem isto, recarregar a página no meio de uma emissão devolvia a pessoa à
    // emissão do Estúdio em vez do ecrã onde estava.
    expect(pagina()).toContain("v === 'tv'")
    expect(pagina()).toContain('?vista=tv&ecra=')
  })

  it('cada um dos cinco ecrãs tem quem o abra', () => {
    // A `cena` não tinha caminho nenhum e as `fontes` só se alcançavam por um
    // lugar VAZIO do barramento: com seis fontes ligadas ficavam ambas
    // inalcançáveis, e o ecrã existia sem existir.
    const ecras = read('web/src/studio/tv/ecras.ts')
    const lista = uniaoDeTexto(ecras, /export type EcraTv = ((?:'[a-z-]+'(?: \| )?)+)/)
    const codigo = FICHEIROS.map(read).join('\n')
    for (const ecra of lista) {
      const abrem = FICHEIROS.filter((f) => read(f).includes(`onNavegar('${ecra}')`))
      // O ecrã inicial abre-se pelo selector de vistas da página, não por um
      // `onNavegar` de dentro.
      if (ecras.includes(`ECRA_TV_INICIAL: EcraTv = '${ecra}'`)) {
        expect(codigo).toContain(`case '${ecra}':`)
        continue
      }
      expect(abrem.length, `nada abre o ecrã «${ecra}»`).toBeGreaterThan(0)
    }
  })
})
