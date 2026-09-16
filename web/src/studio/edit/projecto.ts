/**
 * Projecto de edição NÃO DESTRUTIVO.
 *
 * PORQUE NÃO DESTRUTIVO: o editor anterior substituía a gravação pelo resultado
 * de cada corte. Um corte mal feito não tinha volta — e «desfazer» é a primeira
 * coisa que quem edita procura. Aqui as fontes (os blobs gravados ou
 * importados) NUNCA são tocadas: o projecto é uma descrição em JSON do que se
 * quer tirar delas, e o ficheiro final só nasce na exportação.
 *
 * Este módulo é PURO — sem DOM, sem IndexedDB, sem WebCodecs. Cada edição é
 * uma função `(projecto, edição) → projecto novo`, o que torna o desfazer uma
 * pilha de estados e deixa toda a matemática da linha de tempo testável em
 * Node (`projecto.test.ts`).
 *
 * Tempos: `inicio` é tempo da LINHA DE TEMPO; `entrada`/`saida` são tempo da
 * FONTE. A velocidade liga os dois: um clipe a 2× dura metade na linha de tempo.
 */

export type FaixaDeVideo = 'V1' | 'V2'
export type FaixaDeAudio = 'A1' | 'A2'
export type FaixaDeClipe = FaixaDeVideo | FaixaDeAudio
export type FaixaId = FaixaDeClipe | 'CC'
export const FAIXAS: readonly FaixaId[] = ['V1', 'V2', 'A1', 'A2', 'CC']

export type TipoDeFonte = 'video' | 'audio' | 'av'
export type OrigemDaFonte = 'completo' | 'video' | 'audio' | 'camara' | 'ecra' | 'biblioteca' | 'ficheiro'

export interface Fonte {
  id: string
  nome: string
  tipo: TipoDeFonte
  origem: OrigemDaFonte
  /** Segundos. */
  duracao: number
  largura: number | null
  altura: number | null
  bytes: number
  criadaEm: number
}

export interface Cor {
  /** −1…1 (brilho relativo). */
  exposicao: number
  /** −100…100. */
  contraste: number
  /** −100…100. */
  saturacao: number
  /** −100 (frio) … 100 (quente). */
  temperatura: number
}
export const COR_NEUTRA: Cor = { exposicao: 0, contraste: 0, saturacao: 0, temperatura: 0 }

/** Recorte rectangular em FRACÇÕES da imagem — sobrevive a qualquer resolução. */
export interface Mascara {
  x: number
  y: number
  w: number
  h: number
}

export interface Transicao {
  tipo: 'dissolver' | 'negro'
  duracao: number
}

export interface Clip {
  id: string
  fonteId: string
  faixa: FaixaDeClipe
  inicio: number
  entrada: number
  saida: number
  velocidade: number
  /** Congelar frame: mostra o frame em `entrada` durante estes segundos. */
  congelado: number | null
  cor: Cor
  ganhoDb: number
  transicao: Transicao | null
  mascara: Mascara | null
  /** Clipes de vídeo e áudio da mesma fonte andam juntos até se separar o áudio. */
  grupo: string | null
}

export interface Texto {
  id: string
  inicio: number
  duracao: number
  texto: string
  /** Centro, em fracções do quadro. */
  x: number
  y: number
  /** Altura da letra em fracção da altura do quadro. */
  tamanho: number
}

export interface Marcador {
  id: string
  t: number
  rotulo: string
  tipo: 'marcador' | 'capitulo'
}

export interface Palavra {
  inicio: number
  fim: number
  texto: string
}

export interface Cue {
  id: string
  inicio: number
  fim: number
  texto: string
  palavras?: Palavra[]
  orador?: string
}

export interface Legendas {
  /** Código da língua de origem (ex.: `pt`). */
  lingua: string
  cues: Cue[]
  /** `true` quando os tempos por palavra foram distribuídos, não medidos. */
  estimadas: boolean
  traducoes: Record<string, Cue[]>
}

export interface EstadoDaFaixa {
  id: FaixaId
  visivel: boolean
  bloqueada: boolean
  ganhoDb: number
}

export interface Mistura {
  /** `null` = sem alvo de sonoridade. */
  alvoLufs: number | null
  reduzirRuido: boolean
  normalizar: boolean
}

export type Canto = 'superior-esquerdo' | 'superior-direito' | 'inferior-esquerdo' | 'inferior-direito'

export interface Marca {
  marcaDeAgua: boolean
  canto: Canto
  opacidade: number
}

export type ModoDeLegenda = 'caixa' | 'contorno' | 'faixa' | 'karaoke'
export interface EstiloDeLegenda {
  modo: ModoDeLegenda
  /** Píxeis a 1080p; escala com a altura da saída. */
  tamanho: number
  /** Segunda língua mostrada por baixo, ou `null`. */
  segundaLingua: string | null
}

export interface Projecto {
  versao: 1
  id: string
  titulo: string
  criadoEm: number
  alteradoEm: number
  largura: number
  altura: number
  fps: number
  fontes: Fonte[]
  clips: Clip[]
  textos: Texto[]
  marcadores: Marcador[]
  faixas: EstadoDaFaixa[]
  mistura: Mistura
  legendas: Legendas | null
  estilo: EstiloDeLegenda
  marca: Marca
}

export interface Intervalo {
  inicio: number
  fim: number
}

/** Um frame a 30 fps — abaixo disto um troço não sobrevive ao reencode. */
export const MINIMO = 1 / 30
const EPS = 1e-6

let contador = 0
export function novoId(prefixo = 'x'): string {
  contador = (contador + 1) % 1_000_000
  const aleatorio = Math.random().toString(36).slice(2, 8)
  return `${prefixo}${Date.now().toString(36)}${contador.toString(36)}${aleatorio}`
}

export function novoProjecto(titulo: string, agora = Date.now()): Projecto {
  return {
    versao: 1,
    id: novoId('p'),
    titulo,
    criadoEm: agora,
    alteradoEm: agora,
    largura: 1920,
    altura: 1080,
    fps: 30,
    fontes: [],
    clips: [],
    textos: [],
    marcadores: [],
    faixas: FAIXAS.map((id) => ({ id, visivel: true, bloqueada: false, ganhoDb: id === 'A2' ? -24 : 0 })),
    mistura: { alvoLufs: -14, reduzirRuido: false, normalizar: false },
    legendas: null,
    estilo: { modo: 'caixa', tamanho: 32, segundaLingua: null },
    marca: { marcaDeAgua: false, canto: 'inferior-direito', opacidade: 0.6 },
  }
}

// ---------------------------------------------------------------------------
//  Consultas
// ---------------------------------------------------------------------------

export function duracaoDoClip(c: Clip): number {
  return c.congelado !== null ? c.congelado : (c.saida - c.entrada) / c.velocidade
}

export function fimDoClip(c: Clip): number {
  return c.inicio + duracaoDoClip(c)
}

export function duracaoDoProjecto(p: Projecto): number {
  let fim = 0
  for (const c of p.clips) fim = Math.max(fim, fimDoClip(c))
  return fim
}

export function faixaDeVideo(f: FaixaId): f is FaixaDeVideo {
  return f === 'V1' || f === 'V2'
}

/** Tempo na FONTE que corresponde a `t` na linha de tempo. */
export function tempoNaFonte(c: Clip, t: number): number {
  if (c.congelado !== null) return c.entrada
  return c.entrada + (t - c.inicio) * c.velocidade
}

export function clipEm(p: Projecto, faixa: FaixaDeClipe, t: number): Clip | null {
  for (const c of p.clips) if (c.faixa === faixa && t >= c.inicio - EPS && t < fimDoClip(c) - EPS) return c
  return null
}

export function clipsDaFaixa(p: Projecto, faixa: FaixaDeClipe): Clip[] {
  return p.clips.filter((c) => c.faixa === faixa).sort((a, b) => a.inicio - b.inicio)
}

export function estadoDaFaixa(p: Projecto, id: FaixaId): EstadoDaFaixa {
  return p.faixas.find((f) => f.id === id) ?? { id, visivel: true, bloqueada: false, ganhoDb: 0 }
}

export function fonte(p: Projecto, id: string): Fonte | undefined {
  return p.fontes.find((f) => f.id === id)
}

/** O clipe e os que lhe estão ligados (vídeo+áudio da mesma fonte). */
export function comLigados(p: Projecto, c: Clip): Clip[] {
  if (!c.grupo) return [c]
  return p.clips.filter((o) => o.grupo === c.grupo)
}

/** Cortes (pontos de edição) ordenados — para saltar entre eles no transporte. */
export function pontosDeEdicao(p: Projecto): number[] {
  const s = new Set<number>([0])
  for (const c of p.clips) {
    s.add(round(c.inicio))
    s.add(round(fimDoClip(c)))
  }
  return [...s].sort((a, b) => a - b)
}

export function capitulos(p: Projecto): Marcador[] {
  return p.marcadores.filter((m) => m.tipo === 'capitulo').sort((a, b) => a.t - b.t)
}

/** A cue da língua pedida em `t` (origem se `lingua` for a de origem ou nula). */
export function cueEm(p: Projecto, t: number, lingua?: string | null): Cue | null {
  const l = p.legendas
  if (!l) return null
  const lista = !lingua || lingua === l.lingua ? l.cues : l.traducoes[lingua]
  if (!lista) return null
  for (const c of lista) if (t >= c.inicio && t < c.fim) return c
  return null
}

function round(v: number): number {
  return Math.round(v * 1000) / 1000
}

// ---------------------------------------------------------------------------
//  Intervalos
// ---------------------------------------------------------------------------

/** Ordena, recorta a ≥ 0 e junta os que se tocam. */
export function normalizarIntervalos(xs: Intervalo[]): Intervalo[] {
  const ord = xs
    .map((i) => ({ inicio: Math.max(0, Math.min(i.inicio, i.fim)), fim: Math.max(0, Math.max(i.inicio, i.fim)) }))
    .filter((i) => i.fim - i.inicio > EPS)
    .sort((a, b) => a.inicio - b.inicio)
  const out: Intervalo[] = []
  for (const i of ord) {
    const ult = out[out.length - 1]
    if (ult && i.inicio <= ult.fim + EPS) ult.fim = Math.max(ult.fim, i.fim)
    else out.push({ ...i })
  }
  return out
}

/**
 * Onde vai parar `t` depois de se removerem `intervalos` (normalizados) e
 * fechar os buracos. `null` se `t` cai dentro de um intervalo removido.
 */
export function mapearTempo(t: number, intervalos: Intervalo[]): number | null {
  let removido = 0
  for (const i of intervalos) {
    if (t < i.inicio - EPS) break
    if (t < i.fim - EPS) return null
    removido += i.fim - i.inicio
  }
  return t - removido
}

/** O que sobra de [a, b) tirando os intervalos. */
export function subtrair(a: number, b: number, intervalos: Intervalo[]): Intervalo[] {
  const out: Intervalo[] = []
  let cursor = a
  for (const i of intervalos) {
    if (i.fim <= cursor + EPS) continue
    if (i.inicio >= b - EPS) break
    if (i.inicio > cursor + EPS) out.push({ inicio: cursor, fim: Math.min(i.inicio, b) })
    cursor = Math.max(cursor, i.fim)
    if (cursor >= b - EPS) break
  }
  if (cursor < b - EPS) out.push({ inicio: cursor, fim: b })
  return out
}

/**
 * Intervalos em tempo de FONTE (ex.: as pausas encontradas na faixa de áudio
 * gravada) convertidos para a LINHA DE TEMPO, através dos clipes dessa fonte
 * na faixa indicada. Uma pausa que já foi cortada não aparece.
 */
export function intervalosDaFonteNaLinha(
  p: Projecto,
  fonteId: string,
  faixa: FaixaDeClipe,
  naFonte: Intervalo[],
): Intervalo[] {
  const out: Intervalo[] = []
  for (const c of clipsDaFaixa(p, faixa)) {
    if (c.fonteId !== fonteId || c.congelado !== null) continue
    for (const i of naFonte) {
      const a = Math.max(i.inicio, c.entrada)
      const b = Math.min(i.fim, c.saida)
      if (b - a <= EPS) continue
      out.push({ inicio: c.inicio + (a - c.entrada) / c.velocidade, fim: c.inicio + (b - c.entrada) / c.velocidade })
    }
  }
  return normalizarIntervalos(out)
}

// ---------------------------------------------------------------------------
//  Edições
// ---------------------------------------------------------------------------

export type Edicao =
  | { tipo: 'titulo'; titulo: string }
  | { tipo: 'formato'; largura: number; altura: number; fps: number }
  | { tipo: 'fonte'; fonte: Fonte }
  | { tipo: 'remover-fonte'; fonteId: string }
  | { tipo: 'inserir'; fonteId: string; faixa: FaixaDeClipe; inicio?: number; ligar?: boolean }
  | { tipo: 'dividir'; t: number; clipIds?: string[] }
  | { tipo: 'aparar'; clipId: string; entrada?: number; saida?: number; ripple: boolean }
  | { tipo: 'deslizar'; clipId: string; delta: number }
  | { tipo: 'mover'; clipId: string; inicio: number }
  | { tipo: 'remover'; clipIds: string[]; ripple: boolean }
  | { tipo: 'velocidade'; clipId: string; velocidade: number }
  | { tipo: 'congelar'; t: number; duracao: number }
  | { tipo: 'cor'; clipId: string; cor: Partial<Cor> }
  | { tipo: 'ganho'; clipId: string; ganhoDb: number }
  | { tipo: 'transicao'; clipId: string; transicao: Transicao | null }
  | { tipo: 'mascara'; clipId: string; mascara: Mascara | null }
  | { tipo: 'separar-audio'; clipId: string }
  | { tipo: 'texto'; texto: Texto }
  | { tipo: 'remover-texto'; id: string }
  | { tipo: 'marcador'; marcador: Marcador }
  | { tipo: 'remover-marcador'; id: string }
  | { tipo: 'faixa'; id: FaixaId; patch: Partial<Omit<EstadoDaFaixa, 'id'>> }
  | { tipo: 'mistura'; patch: Partial<Mistura> }
  | { tipo: 'cortar-intervalos'; intervalos: Intervalo[] }
  | { tipo: 'legendas'; legendas: Legendas | null }
  | { tipo: 'estilo'; patch: Partial<EstiloDeLegenda> }
  | { tipo: 'marca'; patch: Partial<Marca> }

/**
 * Aplica uma edição. Devolve o MESMO objecto quando a edição não muda nada
 * (clipe inexistente, faixa bloqueada, sobreposição) — é assim que o histórico
 * sabe que não há passo a registar.
 */
export function editar(p: Projecto, e: Edicao, agora = Date.now()): Projecto {
  const q = aplicarEdicao(p, e)
  return q === p ? p : { ...q, alteradoEm: agora }
}

function aplicarEdicao(p: Projecto, e: Edicao): Projecto {
  switch (e.tipo) {
    case 'titulo':
      return e.titulo === p.titulo ? p : { ...p, titulo: e.titulo }
    case 'formato':
      return { ...p, largura: e.largura, altura: e.altura, fps: e.fps }
    case 'fonte':
      return p.fontes.some((f) => f.id === e.fonte.id) ? p : { ...p, fontes: [...p.fontes, e.fonte] }
    case 'remover-fonte':
      return {
        ...p,
        fontes: p.fontes.filter((f) => f.id !== e.fonteId),
        clips: p.clips.filter((c) => c.fonteId !== e.fonteId),
      }
    case 'inserir':
      return inserir(p, e.fonteId, e.faixa, e.inicio, e.ligar ?? true)
    case 'dividir':
      return dividir(p, e.t, e.clipIds)
    case 'aparar':
      return aparar(p, e.clipId, e.entrada, e.saida, e.ripple)
    case 'deslizar':
      return deslizar(p, e.clipId, e.delta)
    case 'mover':
      return mover(p, e.clipId, e.inicio)
    case 'remover':
      return remover(p, e.clipIds, e.ripple)
    case 'velocidade':
      return mudarVelocidade(p, e.clipId, e.velocidade)
    case 'congelar':
      return congelar(p, e.t, e.duracao)
    case 'cor':
      return mudarClip(p, e.clipId, (c) => ({ ...c, cor: { ...c.cor, ...limitarCor(e.cor) } }), false)
    case 'ganho':
      return mudarClip(p, e.clipId, (c) => ({ ...c, ganhoDb: clamp(e.ganhoDb, -60, 24) }), false)
    case 'transicao':
      return mudarClip(
        p,
        e.clipId,
        (c) => ({
          ...c,
          transicao: e.transicao
            ? { ...e.transicao, duracao: clamp(e.transicao.duracao, MINIMO, duracaoDoClip(c)) }
            : null,
        }),
        false,
      )
    case 'mascara':
      return mudarClip(p, e.clipId, (c) => (faixaDeVideo(c.faixa) ? { ...c, mascara: e.mascara && limitarMascara(e.mascara) } : c), false)
    case 'separar-audio': {
      const c = p.clips.find((x) => x.id === e.clipId)
      if (!c?.grupo || bloqueado(p, c.faixa)) return p
      return { ...p, clips: p.clips.map((x) => (x.grupo === c.grupo ? { ...x, grupo: null } : x)) }
    }
    case 'texto': {
      const t = { ...e.texto, duracao: Math.max(MINIMO, e.texto.duracao), inicio: Math.max(0, e.texto.inicio) }
      const existe = p.textos.some((x) => x.id === t.id)
      return { ...p, textos: existe ? p.textos.map((x) => (x.id === t.id ? t : x)) : [...p.textos, t] }
    }
    case 'remover-texto':
      return { ...p, textos: p.textos.filter((x) => x.id !== e.id) }
    case 'marcador': {
      const m = { ...e.marcador, t: Math.max(0, e.marcador.t) }
      const existe = p.marcadores.some((x) => x.id === m.id)
      const lista = existe ? p.marcadores.map((x) => (x.id === m.id ? m : x)) : [...p.marcadores, m]
      return { ...p, marcadores: lista.sort((a, b) => a.t - b.t) }
    }
    case 'remover-marcador':
      return { ...p, marcadores: p.marcadores.filter((x) => x.id !== e.id) }
    case 'faixa':
      return { ...p, faixas: p.faixas.map((f) => (f.id === e.id ? { ...f, ...e.patch, id: f.id } : f)) }
    case 'mistura':
      return { ...p, mistura: { ...p.mistura, ...e.patch } }
    case 'cortar-intervalos':
      return cortarIntervalos(p, e.intervalos)
    case 'legendas':
      return { ...p, legendas: e.legendas }
    case 'estilo':
      return { ...p, estilo: { ...p.estilo, ...e.patch, tamanho: clamp(e.patch.tamanho ?? p.estilo.tamanho, 16, 72) } }
    case 'marca':
      return { ...p, marca: { ...p.marca, ...e.patch, opacidade: clamp(e.patch.opacidade ?? p.marca.opacidade, 0.1, 1) } }
  }
}

function clamp(v: number, a: number, b: number): number {
  return Math.min(b, Math.max(a, v))
}

function limitarCor(c: Partial<Cor>): Partial<Cor> {
  const out: Partial<Cor> = {}
  if (c.exposicao !== undefined) out.exposicao = clamp(c.exposicao, -1, 1)
  if (c.contraste !== undefined) out.contraste = clamp(c.contraste, -100, 100)
  if (c.saturacao !== undefined) out.saturacao = clamp(c.saturacao, -100, 100)
  if (c.temperatura !== undefined) out.temperatura = clamp(c.temperatura, -100, 100)
  return out
}

function limitarMascara(m: Mascara): Mascara {
  const x = clamp(m.x, 0, 1)
  const y = clamp(m.y, 0, 1)
  return { x, y, w: clamp(m.w, 0.05, 1 - x), h: clamp(m.h, 0.05, 1 - y) }
}

function bloqueado(p: Projecto, f: FaixaId): boolean {
  return estadoDaFaixa(p, f).bloqueada
}

/** Muda um clipe (e os ligados, se `ligados`). Faixa bloqueada = sem efeito. */
function mudarClip(p: Projecto, id: string, fn: (c: Clip) => Clip, ligados: boolean): Projecto {
  const c = p.clips.find((x) => x.id === id)
  if (!c || bloqueado(p, c.faixa)) return p
  const alvo = new Set((ligados ? comLigados(p, c) : [c]).map((x) => x.id))
  return { ...p, clips: p.clips.map((x) => (alvo.has(x.id) ? fn(x) : x)) }
}

function novoClip(parcial: Pick<Clip, 'fonteId' | 'faixa' | 'inicio' | 'entrada' | 'saida'> & Partial<Clip>): Clip {
  return {
    id: novoId('c'),
    velocidade: 1,
    congelado: null,
    cor: { ...COR_NEUTRA },
    ganhoDb: 0,
    transicao: null,
    mascara: null,
    grupo: null,
    ...parcial,
  }
}

function fimDaFaixa(p: Projecto, faixa: FaixaDeClipe): number {
  return clipsDaFaixa(p, faixa).reduce((a, c) => Math.max(a, fimDoClip(c)), 0)
}

function sobrepoe(p: Projecto, c: Clip, ignorar: Set<string>): boolean {
  const a = c.inicio
  const b = fimDoClip(c)
  return p.clips.some((o) => !ignorar.has(o.id) && o.faixa === c.faixa && o.inicio < b - EPS && fimDoClip(o) > a + EPS)
}

function inserir(p: Projecto, fonteId: string, faixa: FaixaDeClipe, inicio: number | undefined, ligar: boolean): Projecto {
  const f = fonte(p, fonteId)
  if (!f || f.duracao <= 0 || bloqueado(p, faixa)) return p
  if (faixaDeVideo(faixa) && f.tipo === 'audio') return p
  if (!faixaDeVideo(faixa) && f.tipo === 'video') return p
  const comAudio = faixaDeVideo(faixa) && f.tipo === 'av' && ligar && !bloqueado(p, 'A1')
  const t0 =
    inicio !== undefined ? Math.max(0, inicio) : Math.max(fimDaFaixa(p, faixa), comAudio ? fimDaFaixa(p, 'A1') : 0)
  const grupo = comAudio ? novoId('g') : null
  const novos = [novoClip({ fonteId, faixa, inicio: t0, entrada: 0, saida: f.duracao, grupo })]
  if (comAudio) novos.push(novoClip({ fonteId, faixa: 'A1', inicio: t0, entrada: 0, saida: f.duracao, grupo }))
  const q = { ...p, clips: [...p.clips, ...novos] }
  const ids = new Set(novos.map((c) => c.id))
  return novos.some((c) => sobrepoe(q, c, ids)) ? p : q
}

/** Divide em `t` os clipes que o atravessam (todos os das faixas livres, ou só estes e os ligados). */
function dividir(p: Projecto, t: number, clipIds?: string[]): Projecto {
  let alvo: Set<string> | null = null
  if (clipIds) {
    alvo = new Set<string>()
    for (const id of clipIds) {
      const c = p.clips.find((x) => x.id === id)
      if (c) for (const l of comLigados(p, c)) alvo.add(l.id)
    }
  }
  let mudou = false
  const clips: Clip[] = []
  for (const c of p.clips) {
    const dentro = t > c.inicio + MINIMO - EPS && t < fimDoClip(c) - MINIMO + EPS
    if (!dentro || bloqueado(p, c.faixa) || (alvo && !alvo.has(c.id))) {
      clips.push(c)
      continue
    }
    mudou = true
    const [esq, dir] = partir(c, t)
    clips.push(esq, dir)
  }
  return mudou ? { ...p, clips } : p
}

function partir(c: Clip, t: number): [Clip, Clip] {
  const grupoDir = c.grupo ? `${c.grupo}|${round(t)}` : null
  if (c.congelado !== null) {
    const d = t - c.inicio
    return [
      { ...c, congelado: d },
      { ...c, id: novoId('c'), inicio: t, congelado: c.congelado - d, transicao: null, grupo: grupoDir },
    ]
  }
  const corte = tempoNaFonte(c, t)
  return [
    { ...c, saida: corte },
    { ...c, id: novoId('c'), inicio: t, entrada: corte, transicao: null, grupo: grupoDir },
  ]
}

/** Desloca por `delta` tudo o que começa em/depois de `desde` nas faixas indicadas. */
function deslocar(p: Projecto, desde: number, delta: number, faixas: Set<FaixaId>, ignorar: Set<string>): Projecto {
  if (Math.abs(delta) < EPS) return p
  const clips = p.clips.map((c) =>
    !ignorar.has(c.id) && faixas.has(c.faixa) && c.inicio >= desde - EPS ? { ...c, inicio: Math.max(0, c.inicio + delta) } : c,
  )
  return { ...p, clips }
}

function aparar(p: Projecto, id: string, entrada: number | undefined, saida: number | undefined, ripple: boolean): Projecto {
  const c = p.clips.find((x) => x.id === id)
  if (!c || bloqueado(p, c.faixa) || c.congelado !== null) return p
  const f = fonte(p, c.fonteId)
  const max = f ? f.duracao : c.saida
  let e = clamp(entrada ?? c.entrada, 0, max)
  let s = clamp(saida ?? c.saida, 0, max)
  if (s - e < MINIMO * c.velocidade) {
    if (entrada !== undefined && saida === undefined) e = s - MINIMO * c.velocidade
    else s = e + MINIMO * c.velocidade
  }
  if (Math.abs(e - c.entrada) < EPS && Math.abs(s - c.saida) < EPS) return p
  const grupo = comLigados(p, c)
  const ids = new Set(grupo.map((x) => x.id))
  const fimAntigo = fimDoClip(c)
  const inicioNovo = ripple ? c.inicio : c.inicio + (e - c.entrada) / c.velocidade
  let q: Projecto = {
    ...p,
    clips: p.clips.map((x) => (ids.has(x.id) ? { ...x, entrada: e, saida: s, inicio: inicioNovo } : x)),
  }
  const novo = q.clips.find((x) => x.id === id)!
  if (ripple) {
    const faixas = new Set<FaixaId>(grupo.map((x) => x.faixa))
    q = deslocar(q, fimAntigo, fimDoClip(novo) - fimAntigo, faixas, ids)
  }
  const colide = q.clips.filter((x) => ids.has(x.id)).some((x) => sobrepoe(q, x, ids))
  return colide ? p : q
}

function deslizar(p: Projecto, id: string, delta: number): Projecto {
  const c = p.clips.find((x) => x.id === id)
  if (!c || bloqueado(p, c.faixa) || c.congelado !== null) return p
  const f = fonte(p, c.fonteId)
  const max = f ? f.duracao : c.saida
  const d = clamp(delta, -c.entrada, max - c.saida)
  if (Math.abs(d) < EPS) return p
  return mudarClip(p, id, (x) => ({ ...x, entrada: x.entrada + d, saida: x.saida + d }), true)
}

function mover(p: Projecto, id: string, inicio: number): Projecto {
  const c = p.clips.find((x) => x.id === id)
  if (!c || bloqueado(p, c.faixa)) return p
  const grupo = comLigados(p, c)
  const minInicio = Math.min(...grupo.map((x) => x.inicio))
  const delta = Math.max(inicio, c.inicio - minInicio) - c.inicio
  if (Math.abs(delta) < EPS) return p
  const ids = new Set(grupo.map((x) => x.id))
  const q = { ...p, clips: p.clips.map((x) => (ids.has(x.id) ? { ...x, inicio: x.inicio + delta } : x)) }
  return q.clips.filter((x) => ids.has(x.id)).some((x) => sobrepoe(q, x, ids)) ? p : q
}

function remover(p: Projecto, clipIds: string[], ripple: boolean): Projecto {
  const ids = new Set<string>()
  for (const id of clipIds) {
    const c = p.clips.find((x) => x.id === id)
    if (c && !bloqueado(p, c.faixa)) for (const l of comLigados(p, c)) if (!bloqueado(p, l.faixa)) ids.add(l.id)
  }
  if (!ids.size) return p
  if (!ripple) return { ...p, clips: p.clips.filter((c) => !ids.has(c.id)) }
  // Ripple por faixa: cada buraco fecha-se só na sua faixa.
  let q: Projecto = p
  const removidos = p.clips.filter((c) => ids.has(c.id)).sort((a, b) => b.inicio - a.inicio)
  q = { ...q, clips: q.clips.filter((c) => !ids.has(c.id)) }
  for (const r of removidos) q = deslocar(q, fimDoClip(r) - EPS, -duracaoDoClip(r), new Set<FaixaId>([r.faixa]), new Set())
  return q
}

function mudarVelocidade(p: Projecto, id: string, v: number): Projecto {
  const c = p.clips.find((x) => x.id === id)
  if (!c || bloqueado(p, c.faixa) || c.congelado !== null) return p
  const vel = clamp(Math.round(v * 100) / 100, 0.25, 4)
  if (Math.abs(vel - c.velocidade) < EPS) return p
  const grupo = comLigados(p, c)
  const ids = new Set(grupo.map((x) => x.id))
  const fimAntigo = fimDoClip(c)
  let q: Projecto = { ...p, clips: p.clips.map((x) => (ids.has(x.id) ? { ...x, velocidade: vel } : x)) }
  const novo = q.clips.find((x) => x.id === id)!
  q = deslocar(q, fimAntigo, fimDoClip(novo) - fimAntigo, new Set<FaixaId>(grupo.map((x) => x.faixa)), ids)
  return q
}

/**
 * Congela o frame de V1 em `t` durante `duracao`: divide tudo em `t`, empurra
 * o resto para a frente em TODAS as faixas livres (o som fica em silêncio
 * durante a pausa) e insere o frame parado.
 */
function congelar(p: Projecto, t: number, duracao: number): Projecto {
  const base = clipEm(p, 'V1', t)
  if (!base || bloqueado(p, 'V1') || duracao < MINIMO) return p
  const frameEm = tempoNaFonte(base, t)
  let q = dividir(p, t)
  const livres = new Set<FaixaId>(FAIXAS.filter((f) => !bloqueado(q, f)))
  q = deslocar(q, t, duracao, livres, new Set())
  q = empurrarAnotacoes(q, t, duracao)
  const parado = novoClip({
    fonteId: base.fonteId,
    faixa: 'V1',
    inicio: t,
    entrada: frameEm,
    saida: frameEm,
    congelado: duracao,
    cor: { ...base.cor },
    mascara: base.mascara,
  })
  return { ...q, clips: [...q.clips, parado] }
}

function empurrarAnotacoes(p: Projecto, desde: number, delta: number): Projecto {
  const mexe = (t: number) => (t >= desde - EPS ? t + delta : t)
  const mexeCue = (c: Cue): Cue => ({
    ...c,
    inicio: mexe(c.inicio),
    fim: c.inicio >= desde - EPS ? c.fim + delta : c.fim,
    palavras: c.palavras?.map((w) => ({ ...w, inicio: mexe(w.inicio), fim: w.inicio >= desde - EPS ? w.fim + delta : w.fim })),
  })
  return {
    ...p,
    textos: p.textos.map((x) => ({ ...x, inicio: mexe(x.inicio) })),
    marcadores: p.marcadores.map((m) => ({ ...m, t: mexe(m.t) })),
    legendas: p.legendas && {
      ...p.legendas,
      cues: p.legendas.cues.map(mexeCue),
      traducoes: Object.fromEntries(Object.entries(p.legendas.traducoes).map(([k, v]) => [k, v.map(mexeCue)])),
    },
  }
}

/**
 * Remove intervalos da LINHA DE TEMPO e fecha os buracos, em todas as faixas
 * livres, nas legendas, nos textos e nos marcadores.
 *
 * É a edição que serve a remoção de pausas E o corte pelo texto: as duas
 * decidem o que sai, e esta faz o resto de maneira que imagem, som e legendas
 * continuam alinhados — que era o que o corte destrutivo fazia à mão.
 */
function cortarIntervalos(p: Projecto, brutos: Intervalo[]): Projecto {
  const intervalos = normalizarIntervalos(brutos)
  if (!intervalos.length) return p
  const clips: Clip[] = []
  let mudou = false
  for (const c of p.clips) {
    if (bloqueado(p, c.faixa)) {
      clips.push(c)
      continue
    }
    const fim = fimDoClip(c)
    const pedacos = subtrair(c.inicio, fim, intervalos)
    const inteiro = pedacos.length === 1 && Math.abs(pedacos[0].inicio - c.inicio) < EPS && Math.abs(pedacos[0].fim - fim) < EPS
    const novoInicio = mapearTempo(c.inicio, intervalos)
    if (inteiro) {
      if (novoInicio !== null && Math.abs(novoInicio - c.inicio) > EPS) mudou = true
      clips.push(novoInicio === null ? c : { ...c, inicio: novoInicio })
      continue
    }
    mudou = true
    pedacos.forEach((pd, i) => {
      if (pd.fim - pd.inicio < MINIMO - EPS) return
      const inicio = mapearTempo(pd.inicio, intervalos) ?? pd.inicio
      const grupo = c.grupo ? `${c.grupo}~${round(pd.inicio)}` : null
      if (c.congelado !== null) {
        clips.push({ ...c, id: i === 0 ? c.id : novoId('c'), inicio, congelado: pd.fim - pd.inicio, grupo, transicao: i === 0 ? c.transicao : null })
      } else {
        clips.push({
          ...c,
          id: i === 0 ? c.id : novoId('c'),
          inicio,
          entrada: tempoNaFonte(c, pd.inicio),
          saida: tempoNaFonte(c, pd.fim),
          grupo,
          transicao: pd.inicio > c.inicio + EPS ? null : c.transicao,
        })
      }
    })
  }
  const legendasBloqueadas = bloqueado(p, 'CC')
  const q: Projecto = {
    ...p,
    clips,
    textos: p.textos.flatMap((x) => {
      const t = mapearTempo(x.inicio, intervalos)
      return t === null ? [] : [{ ...x, inicio: t }]
    }),
    marcadores: p.marcadores.flatMap((m) => {
      const t = mapearTempo(m.t, intervalos)
      return t === null ? [] : [{ ...m, t }]
    }),
    legendas:
      p.legendas && !legendasBloqueadas
        ? {
            ...p.legendas,
            cues: cortarCues(p.legendas.cues, intervalos),
            traducoes: Object.fromEntries(
              Object.entries(p.legendas.traducoes).map(([k, v]) => [k, cortarCues(v, intervalos)]),
            ),
          }
        : p.legendas,
  }
  return mudou || q.legendas !== p.legendas || q.marcadores.length !== p.marcadores.length ? q : p
}

/** Cues e palavras depois de um corte. Uma palavra fica se o seu MEIO fica. */
export function cortarCues(cues: Cue[], intervalos: Intervalo[]): Cue[] {
  const out: Cue[] = []
  for (const c of cues) {
    if (c.palavras?.length) {
      const palavras: Palavra[] = []
      for (const w of c.palavras) {
        const meio = (w.inicio + w.fim) / 2
        if (mapearTempo(meio, intervalos) === null) continue
        const a = mapearTempo(w.inicio, intervalos) ?? mapearFronteira(w.inicio, intervalos, 'fim')
        const b = mapearTempo(w.fim, intervalos) ?? mapearFronteira(w.fim, intervalos, 'inicio')
        palavras.push({ ...w, inicio: a, fim: Math.max(a, b) })
      }
      if (!palavras.length) continue
      out.push({
        ...c,
        palavras,
        inicio: palavras[0].inicio,
        fim: Math.max(palavras[palavras.length - 1].fim, palavras[0].inicio + MINIMO),
        texto: juntarPalavras(palavras),
      })
      continue
    }
    const pedacos = subtrair(c.inicio, c.fim, intervalos)
    if (!pedacos.length) continue
    const a = mapearTempo(pedacos[0].inicio, intervalos) ?? 0
    const b = mapearFronteira(pedacos[pedacos.length - 1].fim, intervalos, 'inicio')
    if (b - a < MINIMO) continue
    out.push({ ...c, inicio: a, fim: b })
  }
  return out
}

/** Um tempo que cai numa fronteira de intervalo, levado para o lado pedido. */
function mapearFronteira(t: number, intervalos: Intervalo[], lado: 'inicio' | 'fim'): number {
  const dentro = intervalos.find((i) => t >= i.inicio - EPS && t <= i.fim + EPS)
  if (!dentro) return mapearTempo(t, intervalos) ?? t
  const ref = lado === 'inicio' ? dentro.inicio : dentro.fim
  let removido = 0
  for (const i of intervalos) {
    if (i.fim <= ref + EPS) removido += i.fim - i.inicio
    else if (i.inicio < ref) removido += ref - i.inicio
  }
  return ref - removido
}

export function juntarPalavras(ps: Palavra[]): string {
  return ps
    .map((w) => w.texto.trim())
    .filter(Boolean)
    .join(' ')
    .replace(/\s+([,.;:!?…»)])/g, '$1')
    .replace(/([«(])\s+/g, '$1')
}

// ---------------------------------------------------------------------------
//  Projecto a partir de uma gravação
// ---------------------------------------------------------------------------

/**
 * Monta a linha de tempo inicial: o vídeo em V1 com o áudio ligado em A1. A
 * câmara e o ecrã gravados em separado ficam no bin, à mão para V2 — pô-los
 * logo na linha de tempo mudaria o que se vê sem ninguém o ter pedido.
 */
export function montarInicial(p: Projecto): Projecto {
  if (p.clips.length) return p
  const porOrigem = (o: OrigemDaFonte) => p.fontes.find((f) => f.origem === o)
  const video = porOrigem('video')
  const audio = porOrigem('audio')
  const principal = porOrigem('completo') ?? porOrigem('biblioteca') ?? porOrigem('ficheiro') ?? p.fontes.find((f) => f.tipo !== 'audio')
  let q = p
  if (video && audio) {
    q = inserir(q, video.id, 'V1', 0, false)
    q = inserir(q, audio.id, 'A1', 0, false)
    const v = q.clips.find((c) => c.faixa === 'V1')
    const a = q.clips.find((c) => c.faixa === 'A1')
    if (v && a) {
      const g = novoId('g')
      q = { ...q, clips: q.clips.map((c) => (c.id === v.id || c.id === a.id ? { ...c, grupo: g } : c)) }
    }
  } else if (principal) {
    q = inserir(q, principal.id, principal.tipo === 'audio' ? 'A1' : 'V1', 0, true)
  }
  return q
}
