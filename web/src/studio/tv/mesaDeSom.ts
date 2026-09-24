/**
 * A mesa de som do estúdio de TV, em Web Audio.
 *
 * Por canal:
 *
 *   fonte → ganho de entrada → EQ (4 biquads) → porta → compressor → limitador
 *         → fader → mudo → [medidor] → PGM
 *                              └→ envio AUX 1 (retorno de auscultadores)
 *
 * e no fim: PGM → fader mestre → [captura p/ sonoridade] → saída (MediaStream
 * que entra no compositor, e daí na gravação e no directo).
 *
 * O que é DA APP e se reutiliza: a porta é o `noise-gate-processor` do
 * `noiseGateWorklet.js`; a «redução de ruído» é o `Denoiser` (RNNoise) do
 * `media.ts`; a sonoridade usa os filtros K do editor. O que é do browser:
 * supressão de eco e nivelamento são restrições do `getUserMedia` e trocam-se
 * pedindo o microfone outra vez.
 *
 * O SOLO é pré-escuta: mexe no retorno de auscultadores, não no programa —
 * numa mesa real, carregar em solo a meio de uma emissão não pode calar o ar.
 */
import { Denoiser } from '../../media'
import { type Banda, dbParaGanho, EQ_INICIAL, faderParaDb, type LeituraDeSonoridade, MedidorDeSonoridade, nivelDoBloco } from './som'

export type TipoDeCanal = 'microfone' | 'participante' | 'ecra'

export interface DinamicaDoCanal {
  porta: { ligada: boolean; limiarDb: number }
  compressor: { ligado: boolean; limiarDb: number; razao: number; ataqueMs: number; recuperacaoMs: number }
  limitador: { ligado: boolean; tectoDb: number }
}

export interface LimpezaDoCanal {
  eco: boolean
  ruido: boolean
  nivelamento: boolean
}

export interface EstadoDoCanal {
  id: string
  nome: string
  tipo: TipoDeCanal
  /** A fonte da mesa de corte a que este canal pertence (para o corte por voz). */
  fonte: string | null
  ganhoDb: number
  fader: number
  mudo: boolean
  solo: boolean
  /** Envio para o retorno de auscultadores (posição de fader). */
  aux: number
  eq: Banda[]
  eqLigado: boolean
  dinamica: DinamicaDoCanal
  limpeza: LimpezaDoCanal
}

export const DINAMICA_INICIAL: DinamicaDoCanal = {
  porta: { ligada: false, limiarDb: -50 },
  compressor: { ligado: true, limiarDb: -24, razao: 3, ataqueMs: 10, recuperacaoMs: 250 },
  limitador: { ligado: true, tectoDb: -2 },
}

/** Informação de formato, medida no próprio contexto e nas tracks. */
export interface InfoDaMesa {
  taxa: number
  latenciaMs: number
  /** Bits por amostra que o dispositivo diz entregar; `null` se não diz. */
  bits: number | null
  estado: AudioContextState
}

interface NosDoCanal {
  fonte: AudioNode | null
  entrada: GainNode
  eq: BiquadFilterNode[]
  porta: AudioWorkletNode | null
  compressor: DynamicsCompressorNode
  limitador: DynamicsCompressorNode
  fader: GainNode
  mudo: GainNode
  medidor: AnalyserNode
  espectro: AnalyserNode
  preMedidor: AnalyserNode
  envioAux: GainNode
}

interface CanalVivo {
  estado: EstadoDoCanal
  nos: NosDoCanal
  /** A track original (microfone) ou o fluxo do participante. */
  bruto: MediaStream
  deviceId?: string
  denoiser: Denoiser | null
  /** Dono do fluxo: um microfone pedido aqui pára-se ao remover. */
  proprio: boolean
}

const CHAVE_CENA = 'dx_studio_tv_cena_som'

const buf = (n: number) => new Float32Array(new ArrayBuffer(n))

export class MesaDeSom {
  readonly ctx: AudioContext
  private canais = new Map<string, CanalVivo>()
  private ordem: string[] = []
  private pgm: GainNode
  private mestre: GainNode
  private medidorL: AnalyserNode
  private medidorR: AnalyserNode
  private aux: GainNode
  private auxDestino: MediaStreamAudioDestinationNode
  private auxElemento: HTMLAudioElement
  private destino: MediaStreamAudioDestinationNode
  private captura: AudioWorkletNode | null = null
  private sonoridade: MedidorDeSonoridade
  private portaDisponivel = false
  private tmp = buf(2048)
  /** Posição do fader mestre. */
  faderMestre = 0.75
  /** Nível do retorno de auscultadores (posição de fader). */
  faderAux = 0.6
  auxLigado = false
  aoMudar: (() => void) | null = null

  constructor() {
    let ctx: AudioContext
    try {
      ctx = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
    } catch {
      ctx = new AudioContext({ latencyHint: 'interactive' })
    }
    this.ctx = ctx
    this.pgm = ctx.createGain()
    this.mestre = ctx.createGain()
    this.mestre.gain.value = dbParaGanho(faderParaDb(this.faderMestre))
    const div = ctx.createChannelSplitter(2)
    this.medidorL = ctx.createAnalyser()
    this.medidorR = ctx.createAnalyser()
    this.medidorL.fftSize = 2048
    this.medidorR.fftSize = 2048
    this.destino = ctx.createMediaStreamDestination()
    this.pgm.connect(this.mestre)
    this.mestre.connect(div)
    div.connect(this.medidorL, 0)
    div.connect(this.medidorR, 1)
    this.mestre.connect(this.destino)

    this.aux = ctx.createGain()
    this.aux.gain.value = 0
    this.auxDestino = ctx.createMediaStreamDestination()
    this.aux.connect(this.auxDestino)
    this.auxElemento = new Audio()
    this.auxElemento.srcObject = this.auxDestino.stream

    this.sonoridade = new MedidorDeSonoridade(ctx.sampleRate)
  }

  /** Carrega os worklets (porta e captura). Chamar uma vez, antes de adicionar canais. */
  async preparar(): Promise<void> {
    const [gate, captura] = await Promise.all([
      import('../../noiseGateWorklet.js?url').then((m) => m.default as string),
      import('./capturaWorklet.js?url').then((m) => m.default as string),
    ])
    try {
      await this.ctx.audioWorklet.addModule(gate)
      this.portaDisponivel = true
    } catch {
      this.portaDisponivel = false
    }
    try {
      await this.ctx.audioWorklet.addModule(captura)
      const n = new AudioWorkletNode(this.ctx, 'captura-processor', { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [2] })
      n.port.onmessage = (e: MessageEvent<Float32Array[]>) => this.sonoridade.alimentar(e.data)
      this.mestre.connect(n)
      // Um nó que não chega ao destino não é processado: liga-se por um ganho a zero.
      const zero = this.ctx.createGain()
      zero.gain.value = 0
      n.connect(zero).connect(this.ctx.destination)
      this.captura = n
    } catch {
      this.captura = null
    }
  }

  /** O som que vai para o ar (entra no compositor). */
  get saida(): MediaStream {
    return this.destino.stream
  }

  get temPorta(): boolean {
    return this.portaDisponivel
  }

  get temSonoridade(): boolean {
    return !!this.captura
  }

  async retomar(): Promise<void> {
    if (this.ctx.state === 'suspended') await this.ctx.resume().catch(() => {})
  }

  info(): InfoDaMesa {
    const lat = (this.ctx.baseLatency || 0) + ((this.ctx as AudioContext & { outputLatency?: number }).outputLatency || 0)
    let bits: number | null = null
    for (const c of this.canais.values()) {
      const s = c.bruto.getAudioTracks()[0]?.getSettings() as MediaTrackSettings & { sampleSize?: number }
      if (s?.sampleSize) bits = Math.max(bits ?? 0, s.sampleSize)
    }
    return { taxa: this.ctx.sampleRate, latenciaMs: lat * 1000, bits, estado: this.ctx.state }
  }

  // ------------------------------------------------------------ canais

  lista(): EstadoDoCanal[] {
    return this.ordem.map((id) => this.canais.get(id)!.estado)
  }

  canal(id: string): EstadoDoCanal | null {
    return this.canais.get(id)?.estado ?? null
  }

  private criarNos(): NosDoCanal {
    const c = this.ctx
    const entrada = c.createGain()
    const eq = EQ_INICIAL.map((b) => {
      const f = c.createBiquadFilter()
      f.type = b.tipo
      f.frequency.value = b.freq
      f.Q.value = b.q
      f.gain.value = 0
      return f
    })
    const porta = this.portaDisponivel ? new AudioWorkletNode(c, 'noise-gate-processor') : null
    const compressor = c.createDynamicsCompressor()
    const limitador = c.createDynamicsCompressor()
    const fader = c.createGain()
    const mudo = c.createGain()
    const medidor = c.createAnalyser()
    medidor.fftSize = 1024
    const espectro = c.createAnalyser()
    espectro.fftSize = 2048
    espectro.smoothingTimeConstant = 0.6
    const preMedidor = c.createAnalyser()
    preMedidor.fftSize = 1024
    const envioAux = c.createGain()

    let no: AudioNode = entrada
    entrada.connect(preMedidor)
    for (const f of eq) {
      no.connect(f)
      no = f
    }
    no.connect(espectro)
    if (porta) {
      no.connect(porta)
      no = porta
    }
    no.connect(compressor)
    compressor.connect(limitador)
    limitador.connect(fader)
    fader.connect(mudo)
    mudo.connect(medidor)
    mudo.connect(this.pgm)
    mudo.connect(envioAux)
    envioAux.connect(this.aux)
    return { fonte: null, entrada, eq, porta, compressor, limitador, fader, mudo, medidor, espectro, preMedidor, envioAux }
  }

  private estadoInicial(id: string, nome: string, tipo: TipoDeCanal, fonte: string | null): EstadoDoCanal {
    const guardado = this.lerCena()[this.chaveDoCanal(id, nome, tipo)]
    const base: EstadoDoCanal = {
      id,
      nome,
      tipo,
      fonte,
      ganhoDb: 0,
      fader: 0.75,
      mudo: false,
      solo: false,
      aux: 0.75,
      eq: EQ_INICIAL.map((b) => ({ ...b })),
      eqLigado: true,
      dinamica: structuredClone(DINAMICA_INICIAL),
      limpeza: { eco: tipo === 'microfone', ruido: false, nivelamento: false },
    }
    if (!guardado) return base
    return { ...base, ...guardado, id, nome, tipo, fonte: guardado.fonte ?? fonte, solo: false }
  }

  private chaveDoCanal(id: string, nome: string, tipo: TipoDeCanal): string {
    return tipo === 'microfone' ? id : `${tipo}:${nome}`
  }

  private ligarFonte(c: CanalVivo, stream: MediaStream): void {
    c.nos.fonte?.disconnect()
    c.nos.fonte = stream.getAudioTracks().length ? this.ctx.createMediaStreamSource(stream) : null
    c.nos.fonte?.connect(c.nos.entrada)
  }

  private registar(estado: EstadoDoCanal, bruto: MediaStream, proprio: boolean, deviceId?: string): void {
    const vivo: CanalVivo = { estado, nos: this.criarNos(), bruto, deviceId, denoiser: null, proprio }
    this.canais.set(estado.id, vivo)
    if (!this.ordem.includes(estado.id)) this.ordem.push(estado.id)
    this.ligarFonte(vivo, bruto)
    this.aplicarNos(vivo)
    if (estado.limpeza.ruido) void this.aplicarRuido(vivo, true)
    this.aoMudar?.()
  }

  private restricoes(deviceId: string, l: LimpezaDoCanal): MediaTrackConstraints {
    return {
      deviceId: deviceId ? { exact: deviceId } : undefined,
      echoCancellation: l.eco,
      autoGainControl: l.nivelamento,
      // O ruído trata-o o RNNoise da app, que é melhor que o do browser.
      noiseSuppression: false,
      channelCount: { ideal: 1 },
      sampleRate: { ideal: 48000 },
    }
  }

  /** Um microfone local como canal. */
  async adicionarMicrofone(deviceId: string, nome: string): Promise<string> {
    const id = `mic:${deviceId || 'omissao'}`
    if (this.canais.has(id)) return id
    const estado = this.estadoInicial(id, nome, 'microfone', null)
    const stream = await navigator.mediaDevices.getUserMedia({ audio: this.restricoes(deviceId, estado.limpeza), video: false })
    if (this.canais.has(id)) {
      stream.getTracks().forEach((t) => t.stop())
      return id
    }
    this.registar(estado, stream, true, deviceId)
    return id
  }

  /** Um fluxo que já existe (participante, ecrã) como canal. */
  adicionarFluxo(id: string, nome: string, tipo: TipoDeCanal, stream: MediaStream, fonte: string | null): void {
    const c = this.canais.get(id)
    if (c) {
      if (c.bruto !== stream) {
        c.bruto = stream
        if (!c.denoiser) this.ligarFonte(c, stream)
      }
      if (c.estado.nome !== nome) c.estado = { ...c.estado, nome }
      return
    }
    this.registar(this.estadoInicial(id, nome, tipo, fonte), stream, false)
  }

  remover(id: string): void {
    const c = this.canais.get(id)
    if (!c) return
    c.denoiser?.stop()
    c.nos.fonte?.disconnect()
    c.nos.mudo.disconnect()
    c.nos.envioAux.disconnect()
    if (c.proprio) c.bruto.getTracks().forEach((t) => t.stop())
    this.canais.delete(id)
    this.ordem = this.ordem.filter((x) => x !== id)
    this.actualizarSolo()
    this.aoMudar?.()
  }

  /** Muda o estado de um canal e aplica-o ao grafo com rampas curtas (sem estalidos). */
  mudar(id: string, patch: Partial<EstadoDoCanal>): void {
    const c = this.canais.get(id)
    if (!c) return
    const antes = c.estado
    c.estado = { ...antes, ...patch }
    this.aplicarNos(c)
    if (patch.limpeza) {
      if (patch.limpeza.ruido !== antes.limpeza.ruido) void this.aplicarRuido(c, patch.limpeza.ruido)
      if (c.deviceId !== undefined && (patch.limpeza.eco !== antes.limpeza.eco || patch.limpeza.nivelamento !== antes.limpeza.nivelamento)) {
        void this.repedirMicrofone(c)
      }
    }
    if (patch.solo !== undefined) this.actualizarSolo()
    this.aoMudar?.()
  }

  private rampa(p: AudioParam, v: number): void {
    p.setTargetAtTime(v, this.ctx.currentTime, 0.015)
  }

  private aplicarNos(c: CanalVivo): void {
    const e = c.estado
    const n = c.nos
    this.rampa(n.entrada.gain, dbParaGanho(e.ganhoDb))
    e.eq.forEach((b, i) => {
      const f = n.eq[i]
      if (!f) return
      f.frequency.setTargetAtTime(b.freq, this.ctx.currentTime, 0.015)
      f.Q.setTargetAtTime(b.q, this.ctx.currentTime, 0.015)
      this.rampa(f.gain, e.eqLigado ? b.ganhoDb : 0)
    })
    if (n.porta) {
      const lim = n.porta.parameters.get('thresholdDb')
      // Porta desligada = limiar no chão: abre para qualquer som.
      if (lim) lim.setValueAtTime(e.dinamica.porta.ligada ? e.dinamica.porta.limiarDb : -80, this.ctx.currentTime)
    }
    const k = e.dinamica.compressor
    n.compressor.threshold.setValueAtTime(k.ligado ? k.limiarDb : 0, this.ctx.currentTime)
    n.compressor.ratio.setValueAtTime(k.ligado ? k.razao : 1, this.ctx.currentTime)
    n.compressor.attack.setValueAtTime(k.ataqueMs / 1000, this.ctx.currentTime)
    n.compressor.release.setValueAtTime(k.recuperacaoMs / 1000, this.ctx.currentTime)
    n.compressor.knee.setValueAtTime(6, this.ctx.currentTime)
    const l = e.dinamica.limitador
    n.limitador.threshold.setValueAtTime(l.ligado ? l.tectoDb : 0, this.ctx.currentTime)
    n.limitador.ratio.setValueAtTime(l.ligado ? 20 : 1, this.ctx.currentTime)
    n.limitador.knee.setValueAtTime(0, this.ctx.currentTime)
    n.limitador.attack.setValueAtTime(0.002, this.ctx.currentTime)
    n.limitador.release.setValueAtTime(0.1, this.ctx.currentTime)
    this.rampa(n.fader.gain, dbParaGanho(faderParaDb(e.fader)))
    this.rampa(n.mudo.gain, e.mudo ? 0 : 1)
    this.actualizarSolo()
  }

  private actualizarSolo(): void {
    const haSolo = [...this.canais.values()].some((c) => c.estado.solo)
    for (const c of this.canais.values()) {
      const envio = haSolo ? (c.estado.solo ? 1 : 0) : dbParaGanho(faderParaDb(c.estado.aux))
      this.rampa(c.nos.envioAux.gain, envio)
    }
  }

  private async aplicarRuido(c: CanalVivo, ligar: boolean): Promise<void> {
    if (!ligar) {
      c.denoiser?.stop()
      c.denoiser = null
      this.ligarFonte(c, c.bruto)
      return
    }
    const track = c.bruto.getAudioTracks()[0]
    if (!track || c.denoiser) return
    const d = new Denoiser()
    try {
      const limpa = await d.process(track)
      if (!this.canais.has(c.estado.id) || !c.estado.limpeza.ruido) {
        d.stop()
        return
      }
      c.denoiser = d
      this.ligarFonte(c, new MediaStream([limpa]))
    } catch {
      d.stop()
      c.estado = { ...c.estado, limpeza: { ...c.estado.limpeza, ruido: false } }
      this.aoMudar?.()
    }
  }

  /** Eco e nivelamento são do `getUserMedia`: pede-se o microfone outra vez com as novas restrições. */
  private async repedirMicrofone(c: CanalVivo): Promise<void> {
    if (c.deviceId === undefined) return
    try {
      const novo = await navigator.mediaDevices.getUserMedia({ audio: this.restricoes(c.deviceId, c.estado.limpeza), video: false })
      if (!this.canais.has(c.estado.id)) {
        novo.getTracks().forEach((t) => t.stop())
        return
      }
      const antigo = c.bruto
      c.bruto = novo
      if (c.denoiser) {
        c.denoiser.stop()
        c.denoiser = null
        await this.aplicarRuido(c, true)
      } else {
        this.ligarFonte(c, novo)
      }
      antigo.getTracks().forEach((t) => t.stop())
    } catch {
      /* fica com o microfone que tinha */
    }
  }

  // ------------------------------------------------------------ mestre e retorno

  mudarMestre(pos: number): void {
    this.faderMestre = pos
    this.rampa(this.mestre.gain, dbParaGanho(faderParaDb(pos)))
    this.aoMudar?.()
  }

  mudarAux(pos: number): void {
    this.faderAux = pos
    this.rampa(this.aux.gain, this.auxLigado ? dbParaGanho(faderParaDb(pos)) : 0)
    this.aoMudar?.()
  }

  /** Liga o retorno de auscultadores numa saída de áudio (`setSinkId` quando o browser deixa). */
  async ligarAux(ligar: boolean, saidaId = ''): Promise<void> {
    this.auxLigado = ligar
    this.rampa(this.aux.gain, ligar ? dbParaGanho(faderParaDb(this.faderAux)) : 0)
    const el = this.auxElemento as HTMLAudioElement & { setSinkId?: (id: string) => Promise<void> }
    if (ligar) {
      if (saidaId && el.setSinkId) await el.setSinkId(saidaId).catch(() => {})
      await el.play().catch(() => {})
    } else {
      el.pause()
    }
    this.aoMudar?.()
  }

  get podeEscolherSaida(): boolean {
    return typeof (this.auxElemento as HTMLAudioElement & { setSinkId?: unknown }).setSinkId === 'function'
  }

  // ------------------------------------------------------------ medidas

  /** Pico e RMS pós-fader de um canal, em dBFS. */
  nivel(id: string): { picoDb: number; rmsDb: number } {
    const c = this.canais.get(id)
    if (!c) return { picoDb: -Infinity, rmsDb: -Infinity }
    return this.lerAnalisador(c.nos.medidor)
  }

  /** Nível ANTES do fader (depois do ganho de entrada) — o que o corte por voz ouve. */
  nivelPre(id: string): { picoDb: number; rmsDb: number } {
    const c = this.canais.get(id)
    if (!c) return { picoDb: -Infinity, rmsDb: -Infinity }
    return this.lerAnalisador(c.nos.preMedidor)
  }

  nivelMestre(): [{ picoDb: number; rmsDb: number }, { picoDb: number; rmsDb: number }] {
    return [this.lerAnalisador(this.medidorL), this.lerAnalisador(this.medidorR)]
  }

  private lerAnalisador(a: AnalyserNode): { picoDb: number; rmsDb: number } {
    if (this.tmp.length !== a.fftSize) this.tmp = buf(a.fftSize)
    a.getFloatTimeDomainData(this.tmp)
    return nivelDoBloco(this.tmp)
  }

  /** Espectro (dBFS por balde) do canal depois do EQ; `out` tem `frequencyBinCount` posições. */
  espectro(id: string, out: Float32Array<ArrayBuffer>): boolean {
    const c = this.canais.get(id)
    if (!c || out.length !== c.nos.espectro.frequencyBinCount) return false
    c.nos.espectro.getFloatFrequencyData(out)
    return true
  }

  binsDoEspectro(id: string): number {
    return this.canais.get(id)?.nos.espectro.frequencyBinCount ?? 0
  }

  /** Redução de ganho que o compressor está a aplicar agora, em dB (≤ 0). */
  reducao(id: string): number {
    return this.canais.get(id)?.nos.compressor.reduction ?? 0
  }

  sonoridadeAgora(): LeituraDeSonoridade {
    return this.sonoridade.ler()
  }

  reiniciarSonoridade(): void {
    this.sonoridade.reiniciar()
  }

  // ------------------------------------------------------------ cena local

  private lerCena(): Record<string, EstadoDoCanal> {
    try {
      return JSON.parse(globalThis.localStorage?.getItem(CHAVE_CENA) ?? '{}') as Record<string, EstadoDoCanal>
    } catch {
      return {}
    }
  }

  /** Guarda o estado de todos os canais NESTE dispositivo. Devolve quantos guardou. */
  guardarCena(): number {
    const cena = this.lerCena()
    for (const c of this.canais.values()) cena[this.chaveDoCanal(c.estado.id, c.estado.nome, c.estado.tipo)] = { ...c.estado, solo: false }
    try {
      globalThis.localStorage?.setItem(CHAVE_CENA, JSON.stringify(cena))
      return this.canais.size
    } catch {
      return 0
    }
  }

  destruir(): void {
    for (const id of [...this.ordem]) this.remover(id)
    this.auxElemento.pause()
    this.auxElemento.srcObject = null
    this.captura?.port.close()
    void this.ctx.close().catch(() => {})
  }
}
