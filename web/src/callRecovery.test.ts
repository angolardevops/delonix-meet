import { describe, it, expect } from 'vitest'
import {
  onPeerState,
  onGraceExpired,
  backoffDelay,
  DEFAULT_RECOVERY,
  type CallState,
} from './callRecovery'

// Codifica a lacuna que a auditoria de 2026-08-25 apontou como a que manda em
// todas as outras: NÃO HAVIA `restartIce()` em lado nenhum. A recuperação era
// recarregar a página. Estes testes fixam o comportamento que a substitui.

const CFG = DEFAULT_RECOVERY
/** rng determinista para o jitter (meio do intervalo). */
const rng = () => 0.5

describe('backoffDelay', () => {
  it('cresce exponencialmente e pára no tecto', () => {
    const noJitter = () => 1 // devolve o valor inteiro, sem redução
    expect(backoffDelay(0, CFG, noJitter)).toBe(800)
    expect(backoffDelay(1, CFG, noJitter)).toBe(1600)
    expect(backoffDelay(2, CFG, noJitter)).toBe(3200)
    // Tecto: sem ele, a 10.ª tentativa esperaria 13 minutos.
    expect(backoffDelay(20, CFG, noJitter)).toBe(CFG.maxMs)
  })

  it('aplica jitter dentro de [metade, inteiro] — nunca zero, nunca acima do tecto', () => {
    for (const r of [0, 0.25, 0.5, 0.99]) {
      const d = backoffDelay(3, CFG, () => r)
      const raw = Math.min(CFG.baseMs * 8, CFG.maxMs)
      expect(d).toBeGreaterThanOrEqual(Math.round(raw * 0.5))
      expect(d).toBeLessThanOrEqual(raw)
    }
  })

  it('espalha as tentativas: sem jitter, uma falha de rede da sala inteira faria N restarts no mesmo instante', () => {
    const delays = new Set([0, 0.1, 0.3, 0.7, 0.9].map((r) => backoffDelay(2, CFG, () => r)))
    expect(delays.size).toBeGreaterThan(1)
  })
})

describe('onPeerState', () => {
  it('`connected` zera as tentativas — uma chamada longa com soluços espaçados não pode esgotar o orçamento', () => {
    const d = onPeerState('connected', 'recovering', 4, CFG, rng)
    expect(d.state).toBe('connected')
    expect(d.attempts).toBe(0)
    expect(d.action).toEqual({ kind: 'none' })
  })

  it('`disconnected` NÃO reinicia já — observa, porque o ICE recupera sozinho com frequência', () => {
    const d = onPeerState('disconnected', 'connected', 0, CFG, rng)
    expect(d.state).toBe('degraded')
    expect(d.action).toEqual({ kind: 'observe', graceMs: CFG.graceMs })
    expect(d.attempts).toBe(0)
  })

  it('`failed` reinicia o ICE já, com backoff, e consome uma tentativa', () => {
    const d = onPeerState('failed', 'connected', 0, CFG, rng)
    expect(d.state).toBe('reconnecting')
    expect(d.action.kind).toBe('restart')
    if (d.action.kind === 'restart') {
      expect(d.action.attempt).toBe(0)
      expect(d.action.delayMs).toBeGreaterThan(0)
    }
    expect(d.attempts).toBe(1)
  })

  it('esgotadas as tentativas, desiste em vez de insistir para sempre', () => {
    const d = onPeerState('failed', 'recovering', CFG.maxAttempts, CFG, rng)
    expect(d.state).toBe('failed')
    expect(d.action).toEqual({ kind: 'give-up' })
  })

  it('`disconnected` DURANTE uma recuperação é ruído da própria recuperação, não um evento novo', () => {
    for (const cur of ['reconnecting', 'recovering'] as CallState[]) {
      const d = onPeerState('disconnected', cur, 2, CFG, rng)
      expect(d.state).toBe(cur)
      expect(d.action).toEqual({ kind: 'none' })
      expect(d.attempts).toBe(2)
    }
  })

  it('`connecting` a meio de uma recuperação não volta a dizer «a ligar» — seria mentir ao utilizador', () => {
    expect(onPeerState('connecting', 'recovering', 1, CFG, rng).state).toBe('recovering')
    expect(onPeerState('connecting', 'reconnecting', 1, CFG, rng).state).toBe('reconnecting')
    // Mas no arranque genuíno, sim.
    expect(onPeerState('connecting', 'connected', 0, CFG, rng).state).toBe('connecting')
  })

  it('a saída intencional é TERMINAL — desligar não pode disparar uma recuperação', () => {
    for (const pc of ['failed', 'disconnected', 'connected'] as RTCPeerConnectionState[]) {
      const d = onPeerState(pc, 'disconnected', 0, CFG, rng)
      expect(d.state).toBe('disconnected')
      expect(d.action).toEqual({ kind: 'none' })
    }
  })

  it('`closed` termina sem tentar recuperar', () => {
    expect(onPeerState('closed', 'connected', 0, CFG, rng).state).toBe('disconnected')
  })
})

describe('onGraceExpired', () => {
  it('continua em `degraded` depois da graça → reinicia o ICE', () => {
    const d = onGraceExpired('degraded', 0, CFG, rng)
    expect(d.state).toBe('reconnecting')
    expect(d.action.kind).toBe('restart')
    expect(d.attempts).toBe(1)
  })

  it('já recuperou entretanto → o temporizador é obsoleto e não faz nada', () => {
    const d = onGraceExpired('connected', 0, CFG, rng)
    expect(d.state).toBe('connected')
    expect(d.action).toEqual({ kind: 'none' })
    expect(d.attempts).toBe(0)
  })

  it('não reinicia depois de a chamada ter sido desligada', () => {
    expect(onGraceExpired('disconnected', 0, CFG, rng).action).toEqual({ kind: 'none' })
  })

  it('respeita o orçamento de tentativas', () => {
    const d = onGraceExpired('degraded', CFG.maxAttempts, CFG, rng)
    expect(d.state).toBe('failed')
    expect(d.action).toEqual({ kind: 'give-up' })
  })
})

describe('percurso completo de uma quebra de rede', () => {
  it('conectado → soluço → graça → restart → recuperado, com as tentativas a zerar', () => {
    let state: CallState = 'connected'
    let attempts = 0

    // 1. A rede muda de caminho: o ICE cai para `disconnected`.
    let d = onPeerState('disconnected', state, attempts, CFG, rng)
    ;({ state, attempts } = d)
    expect(state).toBe('degraded')
    expect(d.action.kind).toBe('observe')

    // 2. A graça expira sem melhoria → reinicia.
    d = onGraceExpired(state, attempts, CFG, rng)
    ;({ state, attempts } = d)
    expect(state).toBe('reconnecting')
    expect(attempts).toBe(1)

    // 3. A PC passa por `connecting` durante o restart — sem regredir a mensagem.
    d = onPeerState('connecting', state, attempts, CFG, rng)
    ;({ state, attempts } = d)
    expect(state).toBe('reconnecting')

    // 4. Media de volta. O orçamento repõe-se para a próxima quebra.
    d = onPeerState('connected', state, attempts, CFG, rng)
    ;({ state, attempts } = d)
    expect(state).toBe('connected')
    expect(attempts).toBe(0)
  })
})
