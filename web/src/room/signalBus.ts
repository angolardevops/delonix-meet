import type { ClientMsg, ServerMsg, Signaling } from '../signaling'

type Tipo = ServerMsg['type']
type Handler = (m: never) => void

/**
 * Ponto único de sinalização da sala.
 *
 * PORQUÊ: o `Signaling` nasce DENTRO do efeito de entrada (depois do token, da
 * chave E2EE e da admissão), mas cada funcionalidade da sala — chat, sondagens,
 * quadro, gravação — vive no seu próprio hook e precisa de registar os seus
 * handlers ao montar, quando ainda não há socket nenhum. Este barramento é
 * estável durante toda a vida do componente: os hooks registam-se nele uma vez,
 * e quando o socket existir (ou for recriado por uma nova tentativa ou por uma
 * frase-chave E2EE) passa a encaminhar para eles.
 *
 * Um socket antigo que ainda entregue uma mensagem depois de ter sido
 * substituído é ignorado: só o `actual` fala com os hooks.
 */
export class RoomSignal {
  private handlers = new Map<string, Set<Handler>>()
  private actual: Signaling | null = null
  private encaminhados = new WeakMap<Signaling, Set<string>>()

  /** Liga um socket novo. Os handlers já registados passam a ouvi-lo. */
  attach(s: Signaling) {
    this.actual = s
    if (!this.encaminhados.has(s)) this.encaminhados.set(s, new Set())
    for (const tipo of this.handlers.keys()) this.encaminhar(s, tipo)
  }

  /** Desliga-o (saída, nova tentativa). Mensagens tardias deixam de chegar. */
  detach(s: Signaling) {
    if (this.actual === s) this.actual = null
  }

  get ligado(): boolean {
    return this.actual !== null
  }

  private encaminhar(s: Signaling, tipo: string) {
    const feitos = this.encaminhados.get(s)
    if (!feitos || feitos.has(tipo)) return
    feitos.add(tipo)
    s.on(tipo as Tipo, (m) => {
      if (this.actual !== s) return
      this.handlers.get(tipo)?.forEach((h) => h(m as never))
    })
  }

  /** Regista um handler; devolve a função que o retira. */
  on<T extends Tipo>(tipo: T, handler: (msg: Extract<ServerMsg, { type: T }>) => void): () => void {
    let set = this.handlers.get(tipo)
    if (!set) {
      set = new Set()
      this.handlers.set(tipo, set)
    }
    set.add(handler as Handler)
    if (this.actual) this.encaminhar(this.actual, tipo)
    return () => {
      set?.delete(handler as Handler)
    }
  }

  send(msg: ClientMsg) {
    this.actual?.send(msg)
  }
}
