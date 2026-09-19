import { useState } from 'react'
import { currentUser } from '../api'
import { HOME_TOUR_STEPS } from './tourSteps'

interface TourState {
  step: number
  /** Chegou ao fim (ou saltou): fecha o cartão, mas o botão de ajuda reabre. */
  done: boolean
  /** Desligado pelo interruptor do cartão: nem o botão de ajuda volta a mostrá-lo. */
  off: boolean
}

const DEFAULT: TourState = { step: 0, done: false, off: false }

const key = (userId: string | number) => `dx_tour_home:${userId}`

function read(userId: string | number): TourState {
  try {
    const raw = localStorage.getItem(key(userId))
    if (!raw) return DEFAULT
    return { ...DEFAULT, ...JSON.parse(raw) }
  } catch {
    return DEFAULT
  }
}

function write(userId: string | number, state: TourState) {
  try {
    localStorage.setItem(key(userId), JSON.stringify(state))
  } catch {
    /* sem armazenamento: vale para esta sessão */
  }
}

/** Guia de primeira utilização do Início — ver `tourSteps.ts` para os passos. */
export function useOnboardingTour() {
  const userId = currentUser()?.id ?? 'anon'
  const [state, setState] = useState<TourState>(() => read(userId))
  const [open, setOpen] = useState(() => {
    const s = read(userId)
    return !s.done && !s.off
  })

  function persist(next: TourState) {
    setState(next)
    write(userId, next)
  }

  const total = HOME_TOUR_STEPS.length
  const current = HOME_TOUR_STEPS[state.step] ?? HOME_TOUR_STEPS[0]

  function next() {
    if (state.step >= total - 1) {
      persist({ ...state, done: true })
      setOpen(false)
      return
    }
    persist({ ...state, step: state.step + 1 })
  }

  function prev() {
    if (state.step === 0) return
    persist({ ...state, step: state.step - 1 })
  }

  /** «Saltar»: fecha sem marcar concluído — o botão de ajuda retoma no mesmo passo. */
  function skip() {
    setOpen(false)
  }

  /** O interruptor «Guia activo»: desliga para sempre, até a Preferências mudar isso. */
  function setEnabled(on: boolean) {
    if (on) {
      persist({ step: 0, done: false, off: false })
      setOpen(true)
    } else {
      persist({ ...state, off: true, done: true })
      setOpen(false)
    }
  }

  function reopen() {
    if (state.done && !state.off) persist({ ...state, step: 0, done: false })
    setOpen(true)
  }

  return {
    open,
    step: state.step,
    total,
    current,
    next,
    prev,
    skip,
    reopen,
    enabled: !state.off,
    setEnabled,
    /** Para o botão de ajuda: só existe enquanto o guia não foi desligado de vez. */
    resumable: !state.off,
    completedCount: state.done ? total : state.step,
  }
}
