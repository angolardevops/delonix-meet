// Recuperação de chamada — a máquina de estados que decide QUANDO reiniciar o
// ICE, extraída para aqui pelo mesmo motivo que o `sfuLifecycle.ts`: a decisão
// tem de ser testável sem um browser, sem rede e sem um SFU.
//
// O que existia antes: NADA. Não havia uma única chamada a `restartIce()` no
// frontend. Quando o caminho de rede mudava — Wi-Fi para dados móveis, o NAT a
// refazer o binding, o portátil a acordar da suspensão — a `RTCPeerConnection`
// ficava em `failed` para sempre e a única recuperação era o utilizador
// RECARREGAR A PÁGINA. Numa rede que oscila, que é o caso normal do mercado a
// que isto se destina, esse comportamento anula boa parte do valor do resto.
//
// Nota sobre o servidor: não precisa de alteração nenhuma. O `webrtc-rs`
// 0.17.1 trata a oferta com credenciais ICE novas em
// `peer_connection/mod.rs:1517` — detecta `have_remote_credentials_change` e
// chama `ice_transport.restart()`. O caminho `apply_client_offer` que já existe
// serve tal e qual.

/** Os estados que uma chamada pode ter, do ponto de vista de quem a usa. */
export type CallState =
  /** A estabelecer pela primeira vez. */
  | 'connecting'
  /** Media a fluir. */
  | 'connected'
  /** ICE em `disconnected`: pode ser um soluço da rede. Ainda não se mexe. */
  | 'degraded'
  /** Desistiu-se de esperar: vai reiniciar-se o ICE (a aguardar o backoff). */
  | 'reconnecting'
  /** Oferta de ICE restart enviada; à espera que a media volte. */
  | 'recovering'
  /** Esgotaram-se as tentativas. Cabe à UI oferecer o último recurso. */
  | 'failed'
  /** Saída intencional. */
  | 'disconnected'

export interface RecoveryConfig {
  /** Quanto se espera em `degraded` antes de assumir que não recupera sozinho. */
  graceMs: number
  /** Primeiro atraso do backoff. */
  baseMs: number
  /** Tecto do backoff. */
  maxMs: number
  /** Tentativas de ICE restart antes de desistir. */
  maxAttempts: number
}

export const DEFAULT_RECOVERY: RecoveryConfig = {
  // O ICE em `disconnected` recupera sozinho com frequência (perda transitória
  // de alguns pacotes de consent). Reiniciar à primeira seria trocar um soluço
  // por uma renegociação completa — mais cara e mais arriscada que o problema.
  graceMs: 4_000,
  baseMs: 800,
  maxMs: 15_000,
  // Seis tentativas com este backoff cobrem ~40 s de rede ausente. Passado
  // isso, o problema não é um soluço e insistir só gasta bateria.
  maxAttempts: 6,
}

export type RecoveryAction =
  /** Nada a fazer. */
  | { kind: 'none' }
  /** Esperar `graceMs` e reavaliar — o ICE pode voltar sozinho. */
  | { kind: 'observe'; graceMs: number }
  /** Reiniciar o ICE daqui a `delayMs`. */
  | { kind: 'restart'; attempt: number; delayMs: number }
  /** Sem mais tentativas: a UI decide o que oferecer ao utilizador. */
  | { kind: 'give-up' }

export interface Decision {
  state: CallState
  action: RecoveryAction
  /** Tentativas consumidas depois desta decisão. */
  attempts: number
}

/**
 * Atraso da tentativa `attempt` (0-based) com **jitter**.
 *
 * O jitter não é enfeite: sem ele, uma falha de rede que atinge a sala inteira
 * põe todos os clientes a reiniciar o ICE no MESMO milissegundo, e o SFU recebe
 * N renegociações simultâneas — a recuperação vira uma segunda avaria. Com
 * jitter, o atraso fica algures em [metade, inteiro] e as tentativas espalham-se.
 */
export function backoffDelay(
  attempt: number,
  cfg: RecoveryConfig = DEFAULT_RECOVERY,
  rng: () => number = Math.random,
): number {
  const raw = Math.min(cfg.baseMs * 2 ** Math.max(0, attempt), cfg.maxMs)
  return Math.round(raw * (0.5 + 0.5 * rng()))
}

/**
 * Decide o próximo estado e a acção, a partir do estado da `RTCPeerConnection`.
 *
 * É puro de propósito: nenhum temporizador, nenhuma promessa, nenhum acesso ao
 * browser. Quem chama é que executa a acção — e é isso que torna todos os
 * caminhos, incluindo os que só acontecem com a rede em baixo, testáveis.
 */
export function onPeerState(
  pcState: RTCPeerConnectionState,
  current: CallState,
  attempts: number,
  cfg: RecoveryConfig = DEFAULT_RECOVERY,
  rng: () => number = Math.random,
): Decision {
  // Saída intencional é terminal: nada a recuperar.
  if (current === 'disconnected') return { state: 'disconnected', action: { kind: 'none' }, attempts }

  switch (pcState) {
    case 'connected':
      // Recuperou. O contador de tentativas ZERA — sem isto, uma chamada longa
      // com vários soluços espaçados esgotava o orçamento e desistia de uma
      // ligação perfeitamente saudável.
      return { state: 'connected', action: { kind: 'none' }, attempts: 0 }

    case 'new':
    case 'connecting':
      // Durante uma recuperação a PC volta a `connecting`; não é um arranque
      // novo, e mostrar «a ligar» a meio de uma recuperação mentia ao utilizador.
      return {
        state: current === 'recovering' || current === 'reconnecting' ? current : 'connecting',
        action: { kind: 'none' },
        attempts,
      }

    case 'disconnected':
      // Já estamos a recuperar? Então isto é ruído da própria recuperação.
      if (current === 'reconnecting' || current === 'recovering') {
        return { state: current, action: { kind: 'none' }, attempts }
      }
      return { state: 'degraded', action: { kind: 'observe', graceMs: cfg.graceMs }, attempts }

    case 'failed':
      // `failed` é definitivo do ponto de vista do browser: não recupera sozinho.
      if (attempts >= cfg.maxAttempts) {
        return { state: 'failed', action: { kind: 'give-up' }, attempts }
      }
      return {
        state: 'reconnecting',
        action: { kind: 'restart', attempt: attempts, delayMs: backoffDelay(attempts, cfg, rng) },
        attempts: attempts + 1,
      }

    case 'closed':
      return { state: 'disconnected', action: { kind: 'none' }, attempts }
  }
}

/**
 * A janela de graça expirou e continuamos em `degraded`: o ICE não voltou
 * sozinho. É aqui que se decide reiniciar.
 */
export function onGraceExpired(
  current: CallState,
  attempts: number,
  cfg: RecoveryConfig = DEFAULT_RECOVERY,
  rng: () => number = Math.random,
): Decision {
  if (current !== 'degraded') {
    // Já recuperou (ou já estamos a tratar disto): o temporizador é obsoleto.
    return { state: current, action: { kind: 'none' }, attempts }
  }
  if (attempts >= cfg.maxAttempts) {
    return { state: 'failed', action: { kind: 'give-up' }, attempts }
  }
  return {
    state: 'reconnecting',
    action: { kind: 'restart', attempt: attempts, delayMs: backoffDelay(attempts, cfg, rng) },
    attempts: attempts + 1,
  }
}
