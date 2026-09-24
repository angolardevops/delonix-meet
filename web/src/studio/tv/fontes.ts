/**
 * As fontes da mesa de corte: câmaras locais (uma por dispositivo), o ecrã
 * partilhado no Estúdio, o quadro local e os participantes da sala ligada.
 *
 * Cada fonte tem uma imagem (vídeo ou canvas), uma correcção de imagem
 * opcional e MEDIDAS reais: resolução e fps entregues pelo dispositivo, e o
 * atraso captura→ecrã quando o browser o dá (`requestVideoFrameCallback` com
 * `captureTime`). Um número que o browser não dá fica `null` — e a interface
 * esconde-o em vez de o inventar.
 *
 * O barramento são as SEIS primeiras fontes pela ordem desta lista; a ordem
 * arruma-se nas Fontes.
 */
import type { Fonte } from '../../room/compositor'
import { CorrectorDeImagem } from '../../media/correccaoGl'
import { type CorreccaoDeImagem, CORRECCAO_NEUTRA, ehNeutra, sanearCorreccao } from './correccao'
import type { FontesParaDesenho, ImagemDeFonte } from './desenhoDaMesa'

export type TipoDeFonte = 'camara' | 'ecra' | 'quadro' | 'participante'

export interface MedidasDaFonte {
  largura: number
  altura: number
  fps: number | null
  /** Atraso da captura ao ecrã, em ms; `null` quando o browser não o dá. */
  latenciaMs: number | null
}

export interface FonteDaMesa {
  id: string
  tipo: TipoDeFonte
  nome: string
  /** O nome do dispositivo (câmara), ou vazio. */
  dispositivo: string
  stream: MediaStream | null
  elemento: ImagemDeFonte
  deviceId?: string
}

interface Viva extends FonteDaMesa {
  corrector: CorrectorDeImagem | null
  correccao: CorreccaoDeImagem
  quadroCorrigido: number
  medidas: MedidasDaFonte
  pararMedicao: (() => void) | null
}

export const MAX_BARRAMENTO = 6

const CHAVE_CORRECCOES = 'dx_studio_tv_correccoes'

function lerCorreccoes(): Record<string, CorreccaoDeImagem> {
  try {
    const o = JSON.parse(globalThis.localStorage?.getItem(CHAVE_CORRECCOES) ?? '{}') as Record<string, unknown>
    return Object.fromEntries(Object.entries(o).map(([k, v]) => [k, sanearCorreccao(v)]))
  } catch {
    return {}
  }
}

function novoVideo(): HTMLVideoElement {
  const v = document.createElement('video')
  v.muted = true
  v.playsInline = true
  v.autoplay = true
  return v
}

type VideoComFrames = HTMLVideoElement & {
  requestVideoFrameCallback?: (cb: (agora: number, meta: { captureTime?: number; presentedFrames?: number; expectedDisplayTime?: number }) => void) => number
  cancelVideoFrameCallback?: (h: number) => void
}

/** Mede fps e atraso de um vídeo pelos frames realmente apresentados. */
function medirVideo(v: VideoComFrames, medidas: MedidasDaFonte): () => void {
  if (typeof v.requestVideoFrameCallback !== 'function') {
    const t = window.setInterval(() => {
      medidas.largura = v.videoWidth
      medidas.altura = v.videoHeight
    }, 1000)
    return () => window.clearInterval(t)
  }
  let h = 0
  let vivo = true
  let janelaInicio = 0
  let framesInicio = 0
  const atrasos: number[] = []
  const passo = (agora: number, meta: { captureTime?: number; presentedFrames?: number; expectedDisplayTime?: number }) => {
    if (!vivo) return
    medidas.largura = v.videoWidth
    medidas.altura = v.videoHeight
    const pf = meta.presentedFrames ?? 0
    if (!janelaInicio) {
      janelaInicio = agora
      framesInicio = pf
    } else if (agora - janelaInicio >= 1000) {
      medidas.fps = Math.round(((pf - framesInicio) * 1000) / (agora - janelaInicio))
      janelaInicio = agora
      framesInicio = pf
    }
    if (typeof meta.captureTime === 'number' && meta.captureTime > 0) {
      const fim = meta.expectedDisplayTime ?? agora
      const d = fim - meta.captureTime
      if (d >= 0 && d < 5000) {
        atrasos.push(d)
        if (atrasos.length > 30) atrasos.shift()
        const ord = [...atrasos].sort((a, b) => a - b)
        medidas.latenciaMs = Math.round(ord[Math.floor(ord.length / 2)])
      }
    }
    h = v.requestVideoFrameCallback!(passo)
  }
  h = v.requestVideoFrameCallback(passo)
  return () => {
    vivo = false
    v.cancelVideoFrameCallback?.(h)
  }
}

export class RegistoDeFontes implements FontesParaDesenho {
  private fontes = new Map<string, Viva>()
  private ordem: string[] = []
  private correccoesGuardadas = lerCorreccoes()
  private quadroN = 0
  private raf = 0
  /** Chamado quando a lista (ou a ordem) muda — não a cada frame. */
  aoMudar: (() => void) | null = null

  constructor() {
    const tique = () => {
      this.quadroN++
      this.raf = requestAnimationFrame(tique)
    }
    this.raf = requestAnimationFrame(tique)
  }

  // ------------------------------------------------------------ leitura

  lista(): FonteDaMesa[] {
    return this.ordem.map((id) => this.fontes.get(id)!).filter(Boolean)
  }

  barramento(): FonteDaMesa[] {
    return this.lista().slice(0, MAX_BARRAMENTO)
  }

  /** O id da fonte n (1–6) do barramento. */
  idDoNumero(n: number): string | null {
    return this.ordem[n - 1] ?? null
  }

  numeroDe(id: string): number {
    const i = this.ordem.indexOf(id)
    return i < 0 || i >= MAX_BARRAMENTO ? 0 : i + 1
  }

  obter(id: string): FonteDaMesa | null {
    return this.fontes.get(id) ?? null
  }

  medidas(id: string): MedidasDaFonte | null {
    return this.fontes.get(id)?.medidas ?? null
  }

  ids(): Set<string> {
    return new Set(this.ordem)
  }

  // ------------------------------------------------------------ desenho

  ajuste(id: string): 'cover' | 'contain' {
    const t = this.fontes.get(id)?.tipo
    return t === 'ecra' || t === 'quadro' ? 'contain' : 'cover'
  }

  /** A imagem SEM correcção — o «antes» da iluminação. */
  imagemCrua(id: string): ImagemDeFonte | null {
    return this.fontes.get(id)?.elemento ?? null
  }

  imagem(id: string): ImagemDeFonte | null {
    const f = this.fontes.get(id)
    if (!f) return null
    if (ehNeutra(f.correccao)) return f.elemento
    f.corrector ??= new CorrectorDeImagem()
    if (f.quadroCorrigido !== this.quadroN) {
      if (!f.corrector.desenhar(f.elemento, f.correccao)) return f.elemento
      f.quadroCorrigido = this.quadroN
    }
    return f.corrector.canvas
  }

  correccao(id: string): CorreccaoDeImagem {
    return this.fontes.get(id)?.correccao ?? { ...CORRECCAO_NEUTRA }
  }

  /** `false` quando a correcção corre em 2D (sem temperatura nem matiz). */
  correccaoCompleta(id: string): boolean {
    const f = this.fontes.get(id)
    if (!f) return true
    f.corrector ??= new CorrectorDeImagem()
    return f.corrector.temTemperatura
  }

  definirCorreccao(id: string, c: CorreccaoDeImagem): void {
    const f = this.fontes.get(id)
    if (!f) return
    f.correccao = c
    f.quadroCorrigido = -1
    // Guarda-se pela CHAVE da fonte (o dispositivo), para a mesma câmara voltar
    // com a mesma correcção amanhã.
    this.correccoesGuardadas[this.chave(f)] = c
    try {
      globalThis.localStorage?.setItem(CHAVE_CORRECCOES, JSON.stringify(this.correccoesGuardadas))
    } catch {
      /* sem armazenamento: vale para esta sessão */
    }
  }

  private chave(f: FonteDaMesa): string {
    return f.tipo === 'camara' ? `camara:${f.deviceId ?? f.dispositivo}` : f.tipo === 'participante' ? `p:${f.nome}` : f.tipo
  }

  // ------------------------------------------------------------ escrita

  private adicionar(f: FonteDaMesa): Viva {
    const medidas: MedidasDaFonte = { largura: 0, altura: 0, fps: null, latenciaMs: null }
    const viva: Viva = {
      ...f,
      corrector: null,
      correccao: { ...CORRECCAO_NEUTRA },
      quadroCorrigido: -1,
      medidas,
      pararMedicao: f.elemento instanceof HTMLVideoElement ? medirVideo(f.elemento as VideoComFrames, medidas) : null,
    }
    viva.correccao = this.correccoesGuardadas[this.chave(viva)] ?? { ...CORRECCAO_NEUTRA }
    this.fontes.set(f.id, viva)
    if (!this.ordem.includes(f.id)) this.ordem.push(f.id)
    return viva
  }

  private retirar(id: string): void {
    const f = this.fontes.get(id)
    if (!f) return
    f.pararMedicao?.()
    f.corrector?.destruir()
    if (f.elemento instanceof HTMLVideoElement && f.tipo !== 'ecra') {
      f.elemento.pause()
      f.elemento.srcObject = null
    }
    this.fontes.delete(id)
    this.ordem = this.ordem.filter((x) => x !== id)
  }

  /** Liga uma câmara local. Rejeita se o browser recusar (a razão vem no erro). */
  async ligarCamara(deviceId: string, rotulo: string): Promise<string> {
    const id = `camara:${deviceId}`
    if (this.fontes.has(id)) return id
    const stream = await navigator.mediaDevices.getUserMedia({
      video: {
        deviceId: deviceId ? { exact: deviceId } : undefined,
        width: { ideal: 1920 },
        height: { ideal: 1080 },
        frameRate: { ideal: 30 },
      },
      audio: false,
    })
    const v = novoVideo()
    v.srcObject = stream
    await v.play().catch(() => {})
    const track = stream.getVideoTracks()[0]
    const nome = rotulo || track?.label || ''
    this.adicionar({ id, tipo: 'camara', nome, dispositivo: track?.label ?? rotulo, stream, elemento: v, deviceId })
    track?.addEventListener('ended', () => this.desligar(id))
    this.aoMudar?.()
    return id
  }

  desligar(id: string): void {
    const f = this.fontes.get(id)
    if (!f) return
    if (f.tipo === 'camara') f.stream?.getTracks().forEach((t) => t.stop())
    this.retirar(id)
    this.aoMudar?.()
  }

  /** O ecrã partilhado no Estúdio (é do compositor: aqui só se mostra). */
  definirEcra(video: HTMLVideoElement | null, stream: MediaStream | null, nome: string): void {
    const id = 'ecra'
    const f = this.fontes.get(id)
    if (!video) {
      if (f) {
        this.retirar(id)
        this.aoMudar?.()
      }
      return
    }
    if (f && f.elemento === video && f.stream === stream) return
    if (f) this.retirar(id)
    this.adicionar({ id, tipo: 'ecra', nome, dispositivo: stream?.getVideoTracks()[0]?.label ?? '', stream, elemento: video })
    this.aoMudar?.()
  }

  definirQuadro(canvas: HTMLCanvasElement | null, nome: string): void {
    const id = 'quadro'
    if (!canvas) {
      if (this.fontes.has(id)) {
        this.retirar(id)
        this.aoMudar?.()
      }
      return
    }
    const f = this.fontes.get(id)
    if (f) {
      f.nome = nome
      return
    }
    this.adicionar({ id, tipo: 'quadro', nome, dispositivo: '', stream: null, elemento: canvas })
    this.aoMudar?.()
  }

  /** Os participantes da sala ligada, de forma incremental (quem sai larga o vídeo). */
  definirParticipantes(pessoas: Fonte[]): void {
    let mudou = false
    const ids = new Set(pessoas.map((p) => `p:${p.id}`))
    for (const id of [...this.fontes.keys()]) {
      if (id.startsWith('p:') && !ids.has(id)) {
        this.retirar(id)
        mudou = true
      }
    }
    for (const p of pessoas) {
      const id = `p:${p.id}`
      const f = this.fontes.get(id)
      if (f && f.stream === p.stream) {
        if (f.nome !== p.nome) {
          f.nome = p.nome
          mudou = true
        }
        continue
      }
      if (f) this.retirar(id)
      const v = novoVideo()
      v.srcObject = p.stream
      void v.play().catch(() => {})
      this.adicionar({ id, tipo: 'participante', nome: p.nome, dispositivo: '', stream: p.stream, elemento: v })
      mudou = true
    }
    if (mudou) this.aoMudar?.()
  }

  /** Mover uma fonte uma posição (−1 sobe, +1 desce) — muda o número do barramento. */
  mover(id: string, delta: -1 | 1): void {
    const i = this.ordem.indexOf(id)
    const j = i + delta
    if (i < 0 || j < 0 || j >= this.ordem.length) return
    const o = [...this.ordem]
    ;[o[i], o[j]] = [o[j], o[i]]
    this.ordem = o
    this.aoMudar?.()
  }

  destruir(): void {
    cancelAnimationFrame(this.raf)
    for (const id of [...this.fontes.keys()]) {
      const f = this.fontes.get(id)
      if (f?.tipo === 'camara') f.stream?.getTracks().forEach((t) => t.stop())
      this.retirar(id)
    }
  }
}
