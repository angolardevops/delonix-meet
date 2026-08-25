// Tempos de estabelecimento de chamada — os números que faltavam para haver
// SLO nenhum sobre «quanto demora a entrar».
//
// A auditoria de 2026-08-25 mediu que se recolhiam três métricas de qualidade
// e ZERO de tempo. E são as de tempo que o cliente pergunta primeiro: «quanto
// demora a entrar numa reunião?» não se responde com bitrate.
//
// Nada disto vem do `getStats()`. O `getStats()` diz o que está a acontecer
// agora; estes números são sobre INSTANTES — quando o utilizador quis entrar,
// quando a media apareceu — e só o cliente os conhece.
//
// O módulo é puro: marca instantes e calcula diferenças. Sem browser, sem rede.

/** Os instantes que interessam, por ordem de acontecimento. */
export type Marco =
  /** O utilizador quis entrar. É daqui que conta o tempo que ELE sente. */
  | 'intencao'
  /** Chegou o room token (a API respondeu). */
  | 'token'
  /** O WebSocket de sinalização abriu. */
  | 'ws'
  /** A oferta SDP saiu para o SFU. */
  | 'oferta'
  /** A resposta do SFU chegou. */
  | 'resposta'
  /** A recolha de candidatos ICE terminou. */
  | 'ice_completo'
  /** A RTCPeerConnection ficou `connected`. */
  | 'ligado'
  /** Chegou o primeiro áudio de outra pessoa. */
  | 'primeiro_audio'
  /** Chegou o primeiro vídeo de outra pessoa. */
  | 'primeiro_video'

export interface Tempos {
  /** Do querer entrar até haver media. É o número que o utilizador sente. */
  join_ms: number | null
  /** Da intenção até o WebSocket abrir. Isola a API + a rede do sinal. */
  ws_ms: number | null
  /** Duração da recolha de candidatos ICE. */
  ice_gathering_ms: number | null
  /** Da intenção até ouvir alguém. */
  first_audio_ms: number | null
  /** Da intenção até ver alguém. */
  first_video_ms: number | null
  /** Reinícios de ICE nesta sessão (ver callRecovery.ts). */
  ice_restarts: number
  /** Recuperações completas (degraded → connected). */
  reconnects: number
}

/**
 * Linha do tempo de UMA sessão de chamada.
 *
 * Cada marco só é registado UMA vez: o segundo `primeiro_audio` não é o
 * primeiro. Sem esta regra, uma renegociação a meio da chamada reescrevia o
 * instante e o «tempo até ouvir» passava a medir a última renegociação —
 * um número que parece bom e não quer dizer nada.
 */
export class LinhaDoTempo {
  private marcos = new Map<Marco, number>()
  private reinicios = 0
  private recuperacoes = 0

  constructor(private agora: () => number = () => performance.now()) {}

  marcar(m: Marco): void {
    if (this.marcos.has(m)) return
    this.marcos.set(m, this.agora())
  }

  /** Já foi marcado? Útil para não repetir trabalho caro. */
  tem(m: Marco): boolean {
    return this.marcos.has(m)
  }

  contarReinicioIce(): void {
    this.reinicios += 1
  }
  contarRecuperacao(): void {
    this.recuperacoes += 1
  }

  /** Milissegundos entre dois marcos. `null` se algum não aconteceu. */
  entre(de: Marco, ate: Marco): number | null {
    const a = this.marcos.get(de)
    const b = this.marcos.get(ate)
    if (a === undefined || b === undefined) return null
    // Nunca negativo: marcos fora de ordem são um erro de instrumentação, e
    // um -12 num painel é pior do que um buraco — parece um dado.
    return b >= a ? Math.round(b - a) : null
  }

  resumo(): Tempos {
    return {
      // O «tempo de entrada» é até haver MEDIA, não até o socket abrir. Um
      // WebSocket aberto com o ecrã preto não é ter entrado numa reunião.
      join_ms: this.entre('intencao', 'ligado'),
      ws_ms: this.entre('intencao', 'ws'),
      ice_gathering_ms: this.entre('oferta', 'ice_completo'),
      first_audio_ms: this.entre('intencao', 'primeiro_audio'),
      first_video_ms: this.entre('intencao', 'primeiro_video'),
      ice_restarts: this.reinicios,
      reconnects: this.recuperacoes,
    }
  }

  /** Há alguma coisa que valha a pena reportar? */
  vale_reportar(): boolean {
    // Sem `ligado` não houve chamada — reportar isso enviesava a média de
    // «tempo até entrar» com sessões que nunca entraram. Essas contam-se
    // noutro sítio (a taxa de sucesso), não aqui.
    return this.marcos.has('ligado')
  }
}
