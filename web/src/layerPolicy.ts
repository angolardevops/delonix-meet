// Política de camada simulcast — QUE qualidade pedir de cada publicador.
//
// O que existia: `wanted_rid(kind, room_size, shift)` no servidor, com DOIS
// sinais — o tamanho da sala e um degrau derivado da perda. Nada mais. Uma sala
// de dez pessoas servia `q` a toda a gente, incluindo ao orador em palco a
// ocupar 70% do ecrã; e uma sala de três servia `f` a toda a gente, incluindo a
// tiles de 90 px num canto.
//
// Porque é que a decisão vive AQUI e não no servidor: quase todos os sinais que
// interessam só existem no cliente. O servidor não sabe o tamanho a que um tile
// está a ser desenhado, se a aba está em segundo plano, se o portátil está a
// ferver, se a bateria está em 8%, nem se o utilizador escolheu poupar dados.
// Adivinhar isso pelo número de participantes é o que estava a ser feito.
//
// O servidor continua a mandar no que é DELE: a perda real medida por RTCP e a
// protecção do nó. A regra de arbitragem é simples — **o cliente pede, a
// realidade da rede corta**. Ver `wanted_rid` em `sfu.rs`.

export type Layer = 'q' | 'h' | 'f'

/** Ordem de qualidade. Índice maior = melhor. */
const ORDER: Layer[] = ['q', 'h', 'f']
const rank = (l: Layer): number => ORDER.indexOf(l)
const shift = (l: Layer, by: number): Layer => ORDER[Math.max(0, Math.min(2, rank(l) - by))]

/** Custo aproximado de cada camada, kbps. Alinhado com `SIMULCAST_ENCODINGS`. */
const COST: Record<Layer, number> = { q: 150, h: 500, f: 1500 }

export interface TileSignal {
  peerId: string
  /** Largura a que o tile está MESMO a ser desenhado, em px de CSS. */
  widthPx: number
  /** Está fixado (pin) pelo utilizador. */
  pinned?: boolean
  /** Está no palco (orador activo ou spotlight). */
  onStage?: boolean
  /** Está a falar agora. */
  speaking?: boolean
  /** É uma partilha de ecrã. */
  presenting?: boolean
}

export interface LocalConditions {
  /** A aba está em segundo plano (o utilizador não está sequer a ver). */
  backgrounded?: boolean
  /** O encoder/decoder está travado por CPU (`qualityLimitationReason`). */
  cpuLimited?: boolean
  /** Perda medida no downlink, %. */
  lossPct?: number
  /** RTT ao SFU, ms. */
  rttMs?: number
  /** Banda descendente estimada, kbps. `null` = desconhecida (não se orçamenta). */
  downlinkKbps?: number | null
  /** Bateria fraca e sem carregador. */
  batteryLow?: boolean
  /** `navigator.connection.saveData`. */
  dataSaver?: boolean
  /** Escolha explícita do utilizador. `auto` deixa a política decidir. */
  preference?: 'auto' | 'data-saver' | 'high'
}

/**
 * Camada base pelo tamanho REAL do tile.
 *
 * Os limiares são as larguras das próprias camadas: `q` é ¼ e `h` é ½ do vídeo
 * inteiro (ver `SIMULCAST_ENCODINGS` em `webrtc.ts`). Pedir `f` para um tile de
 * 160 px é gastar 1,5 Mbps para deitar fora 90% dos pixels no `object-fit`.
 */
export function layerForWidth(widthPx: number): Layer {
  if (widthPx <= 240) return 'q'
  if (widthPx <= 640) return 'h'
  return 'f'
}

/** Importância do tile — decide quem desce primeiro quando a banda não chega. */
function priority(t: TileSignal): number {
  if (t.presenting) return 4 // texto partilhado é o que menos se pode borratar
  if (t.pinned) return 3
  if (t.onStage) return 2
  if (t.speaking) return 1
  return 0
}

/**
 * Escolhe a camada a pedir de cada publicador.
 *
 * A ordem importa e é deliberada: primeiro o que o utilizador está mesmo a ver
 * (tamanho, palco, pin), depois os cortes por condições locais, e só no fim o
 * orçamento de banda — que corta pelos tiles menos importantes primeiro.
 */
export function chooseLayers(
  tiles: TileSignal[],
  cond: LocalConditions = {},
): Record<string, Layer> {
  const out: Record<string, Layer> = {}
  if (tiles.length === 0) return out

  // --- Cortes globais que dispensam qualquer cálculo ---
  //
  // Aba em segundo plano: o utilizador não está a ver NADA. Continua-se a pedir
  // a camada mínima em vez de cancelar a subscrição, para o vídeo reaparecer
  // instantaneamente ao voltar — cancelar obrigaria a uma renegociação e a
  // esperar por um keyframe, que é o «tile preto ao mudar de separador».
  // O ÁUDIO nunca passa por aqui: vive no `AudioSink` e não se toca (R19).
  const pouparTudo =
    cond.backgrounded === true ||
    cond.dataSaver === true ||
    cond.preference === 'data-saver'
  if (pouparTudo) {
    for (const t of tiles) out[t.peerId] = 'q'
    return out
  }

  // --- Base pelo que está a ser desenhado ---
  for (const t of tiles) {
    let l = layerForWidth(t.widthPx)
    // Partilha de ecrã: é texto, e texto ilegível não serve para nada. Sobe
    // para o máximo mesmo num tile pequeno — quem partilha ecrã costuma estar
    // a ser visto em palco logo a seguir.
    if (t.presenting) l = 'f'
    // Pin e palco garantem pelo menos meia camada: o utilizador PEDIU para ver
    // esta pessoa, e servir-lhe `q` porque a grelha é grande é ignorá-lo.
    else if ((t.pinned || t.onStage) && rank(l) < rank('h')) l = 'h'
    out[t.peerId] = l
  }

  // --- Cortes por condições locais ---
  //
  // Somam-se em vez de se escolher o pior: uma máquina a ferver NUMA rede com
  // perda está pior do que qualquer um dos dois isoladamente, e tratá-los como
  // alternativas subestimava sempre o problema.
  let degraus = 0
  if (cond.cpuLimited) degraus += 1
  if (cond.batteryLow) degraus += 1
  const loss = cond.lossPct ?? 0
  if (loss > 8) degraus += 2
  else if (loss > 3) degraus += 1
  if ((cond.rttMs ?? 0) > 400) degraus += 1

  // `high` é uma preferência, não uma ordem: absorve UM degrau, e nunca os que
  // vêm da rede. Um utilizador não pode escolher que a perda de pacotes não
  // exista, e prometer-lhe o contrário dá uma imagem aos solavancos em vez de
  // uma imagem menor.
  if (cond.preference === 'high' && degraus > 0) degraus -= 1

  if (degraus > 0) {
    for (const t of tiles) out[t.peerId] = shift(out[t.peerId], degraus)
  }

  // --- Orçamento de banda ---
  //
  // Sem estimativa não se orçamenta: inventar um tecto seria pior do que não ter
  // nenhum — o controlo de congestão do WebRTC já reage, e um tecto errado
  // impede-o de subir quando a rede melhora.
  const budget = cond.downlinkKbps
  if (budget != null && budget > 0) {
    const custo = () => tiles.reduce((s, t) => s + COST[out[t.peerId]], 0)
    // Desce pelos MENOS importantes primeiro, e ESGOTA cada um antes de passar
    // ao seguinte. A primeira versão descia um degrau em cada tile à vez, o que
    // parece justo e não é: com o orçamento apertado acabava por degradar TODOS,
    // incluindo a partilha de ecrã e o palco. O ponto de haver prioridades é
    // precisamente que o tile que o utilizador está a olhar seja o ÚLTIMO a
    // perder qualidade — não o último a perder o primeiro degrau.
    const porImportancia = [...tiles].sort((a, b) => priority(a) - priority(b))
    for (const t of porImportancia) {
      if (custo() <= budget) break
      while (custo() > budget && out[t.peerId] !== 'q') {
        out[t.peerId] = shift(out[t.peerId], 1)
      }
    }
  }

  return out
}
