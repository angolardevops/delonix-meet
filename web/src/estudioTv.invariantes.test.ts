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

  it('a mesa de som nasce com a SESSÃO, não com o ecrã dela', () => {
    // O que estava partido (EST-D da auditoria de 2026-10-14): o `ligarSom`
    // vivia no `useEffect` de montagem da Mesa de som e da Cena completa.
    // Quem fosse ao ar sem passar por um desses dois ecrãs emitia pelo
    // microfone cru do compositor, sem EQ, porta, compressor nem medidor — e
    // com o cancelamento de eco do browser ligado, que é o certo numa reunião
    // e errado numa emissão.
    const sessao = read('web/src/pages/studio/tv/useSessaoTv.ts')
    expect(sessao).toContain('const abrir = () => void ligarSom()')
    for (const f of FICHEIROS) {
      if (f.endsWith('useSessaoTv.ts')) continue
      expect(read(f), `${f} não pode ligar a mesa por si`).not.toContain('ligarSom()')
    }
  })

  it('o som do ecrã partilhado tem UM dono: a mesa, ou o compositor', () => {
    // EST-B da auditoria de 2026-10-14: o tipo de canal `'ecra'` existia e
    // nada o criava — o som de um vídeo partilhado ia ao ar sem fader, sem
    // medidor e sem porta. Agora é canal da mesa; e o compositor TEM de o
    // largar enquanto ela o tem, senão o mesmo som entra duas vezes.
    const sessao = read('web/src/pages/studio/tv/useSessaoTv.ts')
    expect(sessao).toMatch(/som\.adicionarFluxo\('ecra'.*'ecra'/)
    const comp = read('web/src/studio/compositor.ts')
    expect(comp).toMatch(/private ligarSomDoEcra\(\): void \{[\s\S]*?if \(this\.somExterno\) return/)
    // E a troca de dono tem de reavaliar o ecrã, nos dois sentidos.
    expect(comp).toMatch(/definirSomExterno\(s: MediaStream \| null\): void \{[\s\S]*?this\.ligarSomDoEcra\(\)/)
  })

  it('e a música também tem UM dono — o mestre, ou a mesa', () => {
    // EST-C: a música vivia num barramento que a mesa não via. Agora é canal,
    // e o barramento vai a UM destino de cada vez — somá-la consigo própria
    // seria pior do que não a ter na mesa.
    expect(read('web/src/pages/studio/tv/useSessaoTv.ts')).toMatch(/som\.adicionarFluxo\('musica'/)
    const comp = read('web/src/studio/compositor.ts')
    expect(comp).toMatch(/this\.ganhoMusica\.connect\(this\.somExterno \? this\.destinoMedia : this\.compressorMestre\)/)
    // O mestre deixa de a receber directamente: quem a liga é o barramento.
    expect(comp).not.toMatch(/for \(const g of \[this\.ganhoPalco, this\.ganhoMusica/)
  })

  it('a mesa limita no mestre, e a sonoridade mede-se DEPOIS disso', () => {
    // EST-03 e EST-06 da auditoria de 2026-10-14: a mesa não tinha limitador
    // de mestre, e o número de sonoridade do cabeçalho era medido no fader —
    // antes de qualquer limitação, aqui e no compositor. Um número que não
    // descreve o que sai é pior do que não ter número.
    const mesa = read('web/src/studio/tv/mesaDeSom.ts')
    expect(mesa).toContain('this.mestre.connect(this.limitadorMestre)')
    expect(mesa).toContain('this.limitadorMestre.connect(this.destino)')
    expect(mesa).not.toContain('this.mestre.connect(this.destino)')
    // A captura da sonoridade pendura-se no limitador, não no fader.
    expect(mesa).toContain('this.limitadorMestre.connect(n)')
    // E o tecto é o MESMO do cinto de segurança do compositor: a mesa limita
    // primeiro, e o de baixo quase nunca morde.
    expect(mesa).toContain('this.limitadorMestre.threshold.value = -3')
    expect(read('web/src/studio/compositor.ts')).toContain('this.limiterMestre.threshold.value = -3')
  })

  it('e cada canal tem atraso próprio e entrada estéreo', () => {
    // EST-04 e EST-05: sem atraso não há como alinhar o som com a imagem (a
    // câmara atrasa, o microfone não), e `channelCount: 1` dobrava para mono
    // uma mesa física ou um leitor ligados por USB.
    const mesa = read('web/src/studio/tv/mesaDeSom.ts')
    expect(mesa).toContain('const atraso = c.createDelay(ATRASO_MAXIMO_MS / 1000)')
    expect(mesa).toContain('entrada.connect(atraso)')
    expect(mesa).toContain('channelCount: { ideal: 2 }')
    // O atraso muda sem rampa: uma rampa num atraso varre a altura do som.
    expect(mesa).toMatch(/n\.atraso\.delayTime\.setValueAtTime/)
  })

  it('a ligação canal↔fonte faz alguma coisa: tally na tira e seguir a imagem', () => {
    // EST-E: o selector «fonte de imagem» existia e só servia o corte por voz
    // — ligar um canal a uma fonte não mudava nada no que se via nem no que se
    // ouvia. Agora a tira mostra o tally dessa fonte, e o canal pode seguir a
    // imagem (abrir no ar, calar fora dele).
    const ecra = read('web/src/pages/studio/tv/MesaDeSomEcra.tsx')
    expect(ecra).toContain('const e = tallyDe(s.mesa, canal.fonte)')
    expect(ecra).toContain('data-tv="tally-do-canal"')
    // Desligar o «seguir» tem de LARGAR o mudo: senão um canal calado pelo
    // automatismo ficava calado para sempre, e sem ninguém o ter calado.
    expect(ecra).toContain('{ seguirImagem: v, ...(v ? {} : { mudo: false }) }')
    const sessao = read('web/src/pages/studio/tv/useSessaoTv.ts')
    expect(sessao).toContain("const mudo = tallyDe(mesa, c.fonte) !== 'programa'")
    // E não vem ligado de origem: numa emissão, um canal que se cala sozinho
    // sem ninguém ter pedido é pior do que um canal esquecido aberto.
    expect(read('web/src/studio/tv/mesaDeSom.ts')).toContain('seguirImagem: false,')
  })

  it('um efeito é um CANAL da mesa, não um `<audio>` solto', () => {
    // Um genérico que saísse por fora da mesa era exactamente o som que
    // ninguém controla: sem fader, sem medidor, sem limitador — e a mesa a
    // mostrar um nível que não é o que vai ao ar.
    const mesa = read('web/src/studio/tv/mesaDeSom.ts')
    expect(mesa).toContain("adicionarEfeito(id: string, nome: string): void")
    expect(mesa).toContain('fonte.connect(c.nos.fonte)')
    // E o disparo não se soma a si próprio: duas vezes na mesma tecla é um
    // engano, não um coro.
    expect(mesa).toMatch(/tocarEfeito\([\s\S]*?this\.pararEfeito\(id\)/)
    const sessao = read('web/src/pages/studio/tv/useSessaoTv.ts')
    expect(sessao).not.toMatch(/new Audio\(/)
  })

  it('o ducking mexe num ganho PRÓPRIO, nunca no fader do operador', () => {
    // Mexer no fader seria o automatismo a apagar o que ele pôs — e ao largar
    // ficava no sítio errado, sem ninguém lhe ter tocado.
    const mesa = read('web/src/studio/tv/mesaDeSom.ts')
    expect(mesa).toContain('fader.connect(duck)')
    expect(mesa).toContain('duck.connect(mudo)')
    expect(mesa).toMatch(/aplicarDuck\(reduzDb: number\): void \{[\s\S]*?c\.nos\.duck\.gain/)
    // E a decisão é da função pura, não de um `setInterval` com a regra dentro.
    expect(read('web/src/pages/studio/tv/useSessaoTv.ts')).toContain('haVoz(som.niveisDeVoz()) ? DUCK_DB : 0')
  })

  it('a mesa física mexe nos MESMOS objectos que o rato', () => {
    // Um caminho de controlo à parte dava dois estados a divergir: o fader do
    // ecrã numa posição e o da superfície noutra, e o ar a seguir um deles sem
    // se saber qual.
    const sessao = read('web/src/pages/studio/tv/useSessaoTv.ts')
    expect(sessao).toContain('const mesaFisica = useMesaFisica({')
    expect(sessao).toMatch(/case 'fader': \{[\s\S]*?m\.mudar\(c\.id, \{ fader: a\.posicao \}\)/)
    expect(sessao).toMatch(/case 'cortar':[\s\S]*?setMesa\(\(e\) => cortar\(e, Date\.now\(\)\)\)/)
  })

  it('e o acesso MIDI pede-se a um CLIQUE, nunca ao abrir', () => {
    // Um estúdio que levanta um pedido de permissão ao abrir ensina a pessoa a
    // recusar sem ler.
    const hook = read('web/src/pages/studio/tv/useMesaFisica.ts')
    expect(hook).toContain('const ligar = useCallback(async () => {')
    expect(hook).toContain('await navigator.requestMIDIAccess({ sysex: false })')
    // O pedido não pode viver num efeito de montagem.
    expect(hook).not.toMatch(/useEffect\([^)]*\{[\s\S]{0,200}requestMIDIAccess/)
    expect(read('web/src/pages/studio/tv/MesaDeSomEcra.tsx')).toContain('data-tv="ligar-mesa-fisica"')
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
