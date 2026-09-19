/**
 * Desmultiplexador mínimo de WebM/Matroska — só o que a exportação precisa:
 * as pistas, e os blocos com tempo, frame-chave e bytes.
 *
 * PORQUE EXISTE: a exportação de um projecto com vários clipes e duas faixas de
 * vídeo precisa do frame EXACTO de cada fonte em cada instante. O `<video>` a
 * procurar frame a frame recomeça a decodificar do frame-chave anterior a cada
 * procura (o MediaRecorder põe um a cada poucos segundos) — um minuto de vídeo
 * eram dezenas de milhares de decodificações. Com os blocos à mão, o
 * `VideoDecoder` decodifica cada troço UMA vez, em sequência, e por hardware.
 *
 * O que suporta, porque é o que existe neste produto: os WebM do MediaRecorder
 * (Segment e Cluster com tamanho DESCONHECIDO), os do `webm-muxer` e os que o
 * ffmpeg do servidor escreve (tamanhos conhecidos, BlockGroup). Blocos com
 * «lacing» nas pistas de vídeo não aparecem nestes ficheiros e são recusados
 * em vez de lidos mal.
 */

export interface PistaWebm {
  numero: number
  tipo: 'video' | 'audio' | 'outra'
  codec: string
  privado: Uint8Array | null
  largura: number
  altura: number
  taxa: number
  canais: number
}

export interface BlocoWebm {
  pista: number
  /** Microssegundos. */
  tempo: number
  chave: boolean
  dados: Uint8Array
}

export interface Webm {
  pistas: PistaWebm[]
  blocos: BlocoWebm[]
  /** Segundos, do cabeçalho; `null` quando o ficheiro não a traz (MediaRecorder). */
  duracao: number | null
}

const ID = {
  EBML: 0x1a45dfa3,
  Segment: 0x18538067,
  Info: 0x1549a966,
  TimecodeScale: 0x2ad7b1,
  Duration: 0x4489,
  Tracks: 0x1654ae6b,
  TrackEntry: 0xae,
  TrackNumber: 0xd7,
  TrackType: 0x83,
  CodecID: 0x86,
  CodecPrivate: 0x63a2,
  Video: 0xe0,
  PixelWidth: 0xb0,
  PixelHeight: 0xba,
  Audio: 0xe1,
  SamplingFrequency: 0xb5,
  Channels: 0x9f,
  Cluster: 0x1f43b675,
  Timecode: 0xe7,
  SimpleBlock: 0xa3,
  BlockGroup: 0xa0,
  Block: 0xa1,
  ReferenceBlock: 0xfb,
} as const

/** Elementos em que se ENTRA (os filhos seguem-se no fluxo) em vez de os saltar. */
const ENTRAR = new Set<number>([ID.Segment, ID.Cluster, ID.Tracks, ID.TrackEntry, ID.Video, ID.Audio, ID.Info])

class Leitor {
  pos = 0
  constructor(readonly b: Uint8Array) {}

  get fim(): boolean {
    return this.pos >= this.b.length
  }

  id(): number | null {
    const primeiro = this.b[this.pos]
    if (primeiro === undefined) return null
    let largura = 1
    while (largura <= 4 && !(primeiro & (0x80 >> (largura - 1)))) largura++
    if (largura > 4 || this.pos + largura > this.b.length) return null
    let v = 0
    for (let i = 0; i < largura; i++) v = v * 256 + this.b[this.pos + i]
    this.pos += largura
    return v
  }

  /** Tamanho; `-1` = desconhecido (todos os bits a 1). */
  tamanho(): number | null {
    const primeiro = this.b[this.pos]
    if (primeiro === undefined) return null
    let largura = 1
    while (largura <= 8 && !(primeiro & (0x80 >> (largura - 1)))) largura++
    if (largura > 8 || this.pos + largura > this.b.length) return null
    let v = primeiro & (0xff >> largura)
    let todosUns = v === 0xff >> largura
    for (let i = 1; i < largura; i++) {
      const byte = this.b[this.pos + i]
      if (byte !== 0xff) todosUns = false
      v = v * 256 + byte
    }
    this.pos += largura
    return todosUns ? -1 : v
  }
}

function uint(b: Uint8Array): number {
  let v = 0
  for (const x of b) v = v * 256 + x
  return v
}

function float(b: Uint8Array): number {
  const dv = new DataView(b.buffer, b.byteOffset, b.byteLength)
  return b.byteLength === 4 ? dv.getFloat32(0) : b.byteLength === 8 ? dv.getFloat64(0) : 0
}

function texto(b: Uint8Array): string {
  let s = ''
  for (const x of b) if (x) s += String.fromCharCode(x)
  return s
}

export function lerWebm(bytes: Uint8Array): Webm {
  const r = new Leitor(bytes)
  const pistas: PistaWebm[] = []
  const blocos: BlocoWebm[] = []
  let escala = 1_000_000 // TimecodeScale em ns; omisso = 1 ms
  let duracaoBruta: number | null = null
  let cluster = 0
  let pista: PistaWebm | null = null
  // Um BlockGroup guarda o Block e só no fim se sabe se tinha ReferenceBlock.
  let grupo: { bloco: Uint8Array | null; referencia: boolean; fim: number } | null = null

  const fecharGrupo = () => {
    if (grupo?.bloco) lerBloco(grupo.bloco, !grupo.referencia, true)
    grupo = null
  }

  const lerBloco = (d: Uint8Array, chaveDoGrupo: boolean, doGrupo: boolean) => {
    const sub = new Leitor(d)
    const numero = sub.tamanho()
    if (numero === null || sub.pos + 3 > d.length) return
    const relativo = (d[sub.pos] << 24) >> 16 | d[sub.pos + 1]
    const flags = d[sub.pos + 2]
    const dados = d.subarray(sub.pos + 3)
    const lacing = (flags & 0x06) !== 0
    const p = pistas.find((x) => x.numero === numero)
    if (lacing && p?.tipo === 'video') throw new Error('WebM com lacing em vídeo não é suportado')
    const chave = doGrupo ? chaveDoGrupo : (flags & 0x80) !== 0
    blocos.push({ pista: numero, tempo: Math.round(((cluster + relativo) * escala) / 1000), chave, dados })
  }

  while (!r.fim) {
    if (grupo && r.pos >= grupo.fim) fecharGrupo()
    const inicio = r.pos
    const id = r.id()
    const tam = r.tamanho()
    if (id === null || tam === null) break
    if (ENTRAR.has(id)) {
      if (id === ID.TrackEntry) {
        pista = { numero: 0, tipo: 'outra', codec: '', privado: null, largura: 0, altura: 0, taxa: 0, canais: 0 }
        pistas.push(pista)
      }
      continue
    }
    if (id === ID.BlockGroup) {
      fecharGrupo()
      if (tam < 0) break
      grupo = { bloco: null, referencia: false, fim: r.pos + tam }
      continue
    }
    if (tam < 0) {
      // Tamanho desconhecido num elemento em que não se entra: não se sabe
      // onde acaba, e continuar seria ler lixo como blocos.
      if (id === ID.EBML) break
      r.pos = inicio
      break
    }
    const corpo = bytes.subarray(r.pos, Math.min(bytes.length, r.pos + tam))
    r.pos += tam
    switch (id) {
      case ID.TimecodeScale:
        escala = uint(corpo)
        break
      case ID.Duration:
        duracaoBruta = float(corpo)
        break
      case ID.TrackNumber:
        if (pista) pista.numero = uint(corpo)
        break
      case ID.TrackType:
        if (pista) pista.tipo = uint(corpo) === 1 ? 'video' : uint(corpo) === 2 ? 'audio' : 'outra'
        break
      case ID.CodecID:
        if (pista) pista.codec = texto(corpo)
        break
      case ID.CodecPrivate:
        if (pista) pista.privado = corpo
        break
      case ID.PixelWidth:
        if (pista) pista.largura = uint(corpo)
        break
      case ID.PixelHeight:
        if (pista) pista.altura = uint(corpo)
        break
      case ID.SamplingFrequency:
        if (pista) pista.taxa = float(corpo)
        break
      case ID.Channels:
        if (pista) pista.canais = uint(corpo)
        break
      case ID.Timecode:
        fecharGrupo()
        cluster = uint(corpo)
        break
      case ID.SimpleBlock:
        fecharGrupo()
        lerBloco(corpo, false, false)
        break
      case ID.Block:
        if (grupo) grupo.bloco = corpo
        break
      case ID.ReferenceBlock:
        if (grupo) grupo.referencia = true
        break
      default:
        break
    }
  }
  fecharGrupo()
  blocos.sort((a, b) => a.pista - b.pista || a.tempo - b.tempo)
  return {
    pistas,
    blocos,
    duracao: duracaoBruta === null ? null : (duracaoBruta * escala) / 1e9,
  }
}

/** A string de codec que o `VideoDecoder` entende, ou `null` se não se sabe. */
export function codecParaDecoder(p: PistaWebm): { codec: string; description?: Uint8Array } | null {
  switch (p.codec) {
    case 'V_VP8':
      return { codec: 'vp8' }
    case 'V_VP9':
      return { codec: 'vp09.00.10.08' }
    case 'V_AV1':
      return { codec: 'av01.0.08M.08' }
    case 'V_MPEG4/ISO/AVC': {
      const d = p.privado
      if (!d || d.length < 4) return null
      const hex = (n: number) => n.toString(16).padStart(2, '0')
      return { codec: `avc1.${hex(d[1])}${hex(d[2])}${hex(d[3])}`, description: d }
    }
    default:
      return null
  }
}
