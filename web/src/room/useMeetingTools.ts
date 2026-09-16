import { useEffect, useRef, useState } from 'react'
import type { PollView, QaView } from '../signaling'
import type { RoomCore } from './useRoomCore'

export interface PollDraft {
  question: string
  options: string[]
  /** Índice na lista COMPLETA (com vazias) — remapeia-se ao enviar. */
  correct: number | null
  durationSecs: number
}

/**
 * Sondagens e quizzes, perguntas e respostas, temporizador da reunião. O
 * servidor é a fonte de verdade: a vista mostra o que ele devolve.
 */
export function useMeetingTools(core: RoomCore) {
  const { signal, code, isHost } = core
  const [polls, setPolls] = useState<PollView[]>([])
  /** Quando cada sondagem apareceu NESTE dispositivo — para a pôr no fio do chat. */
  const [pollSeenAt, setPollSeenAt] = useState<Record<string, number>>({})
  const [questions, setQuestions] = useState<QaView[]>([])
  /** Quando cada pergunta apareceu NESTE dispositivo — para a pôr no fio do chat. */
  const [qaSeenAt, setQaSeenAt] = useState<Record<string, number>>({})
  /** Fim do temporizador, em SEGUNDOS epoch (`room_tools.rs`). */
  const [timerEndsAt, setTimerEndsAt] = useState<number | null>(null)
  const [myVotes, setMyVotes] = useState<Record<string, number>>(() => {
    // Sobrevive a um reload a meio do quiz — senão a festa nunca dispara.
    try {
      return JSON.parse(sessionStorage.getItem(`dx_votes_${code}`) ?? '{}')
    } catch {
      return {}
    }
  })
  const [myUpvotes, setMyUpvotes] = useState<Record<string, boolean>>({})
  const [dismissed, setDismissed] = useState<Record<string, boolean>>({})
  const [revealUntil, setRevealUntil] = useState<Record<string, number>>({})
  const [winnerFx, setWinnerFx] = useState(false)
  const pollPrevOpenRef = useRef<Record<string, boolean>>({})
  const pollCloseSentRef = useRef<Record<string, boolean>>({})

  useEffect(() => {
    const offs = [
      signal.on('polls', (m) => {
        setPolls(m.polls)
        setPollSeenAt((seen) => {
          const novas = m.polls.filter((p) => seen[p.id] == null)
          if (novas.length === 0) return seen
          const agora = Date.now()
          return { ...seen, ...Object.fromEntries(novas.map((p) => [p.id, agora])) }
        })
      }),
      signal.on('qa', (m) => {
        setQuestions(m.questions)
        setQaSeenAt((seen) => {
          const novas = m.questions.filter((q) => seen[q.id] == null)
          if (novas.length === 0) return seen
          const agora = Date.now()
          return { ...seen, ...Object.fromEntries(novas.map((q) => [q.id, agora])) }
        })
      }),
      signal.on('timer', (m) => setTimerEndsAt(m.ends_at)),
    ]
    return () => offs.forEach((off) => off())
  }, [signal])

  useEffect(() => {
    try {
      sessionStorage.setItem(`dx_votes_${code}`, JSON.stringify(myVotes))
    } catch {
      /* armazenamento cheio: segue sem persistir */
    }
  }, [myVotes, code])

  // Fecho do quiz: a revelação volta a aparecer (mesmo dispensada) e há festa
  // para quem acertou — na transição e quando já chega fechada (reload), com
  // guarda para nunca celebrar duas vezes.
  useEffect(() => {
    for (const p of polls) {
      const was = pollPrevOpenRef.current[p.id]
      const closedNow = was && !p.open
      const arrivedClosed = was === undefined && !p.open
      if (closedNow) {
        setRevealUntil((m) => ({ ...m, [p.id]: Date.now() + 10_000 }))
        setDismissed((m) => (m[p.id] ? { ...m, [p.id]: false } : m))
      }
      if ((closedNow || arrivedClosed) && p.correct != null && myVotes[p.id] === p.correct) {
        const guard = `dx_fx_${p.id}`
        if (!sessionStorage.getItem(guard)) {
          sessionStorage.setItem(guard, '1')
          setWinnerFx(true)
          window.setTimeout(() => setWinnerFx(false), 4500)
        }
      }
      pollPrevOpenRef.current[p.id] = p.open
    }
  }, [polls, myVotes])

  // Tique de revelação: existe SÓ enquanto há uma janela aberta. Fora dela não
  // há relógio na raiz (2.1).
  const revelacaoAberta = Object.values(revealUntil).some((v) => v > Date.now())
  const [, setRevTick] = useState(0)
  useEffect(() => {
    if (!revelacaoAberta) return
    const id = setInterval(() => setRevTick((n) => n + 1), 1000)
    return () => clearInterval(id)
  }, [revelacaoAberta])

  // O anfitrião fecha o quiz quando o tempo acaba — o servidor valida, revela e
  // persiste. Não usa estado: precisa do TIQUE, não de um render. O intervalo
  // lê o relógio e a lista pelo ref.
  //
  // `ends_at` das sondagens vem em MILISSEGUNDOS (`now_ms()` em room_tools.rs).
  // A versão anterior comparava-o com segundos e o fecho automático nunca
  // disparava.
  const pollsRef = useRef(polls)
  pollsRef.current = polls
  useEffect(() => {
    if (!isHost) return
    const id = setInterval(() => {
      const agora = Date.now()
      for (const p of pollsRef.current) {
        if (p.open && p.ends_at && agora >= p.ends_at && !pollCloseSentRef.current[p.id]) {
          pollCloseSentRef.current[p.id] = true
          signal.send({ type: 'poll-close', poll: p.id })
        }
      }
    }, 1000)
    return () => clearInterval(id)
  }, [isHost, signal])

  /** O cartão que aparece a TODOS: a última aberta, ou a que está a revelar. */
  const popup =
    [...polls].reverse().find((x) => !dismissed[x.id] && (x.open || (revealUntil[x.id] ?? 0) > Date.now())) ?? null

  function createPoll(d: PollDraft) {
    const opts = d.options.map((o, i) => ({ o: o.trim(), i })).filter((x) => x.o)
    const correctIdx = d.correct != null ? opts.findIndex((x) => x.i === d.correct) : -1
    signal.send({
      type: 'poll-create',
      question: d.question,
      options: opts.map((x) => x.o),
      correct_option: correctIdx >= 0 ? correctIdx : null,
      duration_secs: d.durationSecs || null,
    })
  }

  function vote(pollId: string, option: number) {
    signal.send({ type: 'poll-vote', poll: pollId, option })
    setMyVotes((v) => ({ ...v, [pollId]: option }))
  }

  return {
    polls,
    pollSeenAt,
    questions,
    qaSeenAt,
    /** A pergunta em destaque no palco (para todos). */
    spotlitQuestion: questions.find((q) => q.spotlight && !q.hidden) ?? null,
    timerEndsAt,
    myVotes,
    myUpvotes,
    popup,
    winnerFx,
    createPoll,
    vote,
    closePoll: (id: string) => signal.send({ type: 'poll-close', poll: id }),
    dismissPoll: (id: string) => setDismissed((m) => ({ ...m, [id]: true })),
    ask: (text: string) => signal.send({ type: 'qa-ask', text }),
    upvote: (id: string) => {
      signal.send({ type: 'qa-upvote', id })
      setMyUpvotes((m) => ({ ...m, [id]: !m[id] }))
    },
    markAnswered: (id: string) => signal.send({ type: 'qa-answered', id }),
    /** Só anfitrião: quem não é anfitrião deixa de receber a pergunta. */
    hideQuestion: (id: string, hidden = true) => signal.sendB1({ type: 'qa-hide', id, hidden }),
    /** Só anfitrião: uma de cada vez; `null` limpa. Destacar também a mostra. */
    spotlightQuestion: (id: string | null) => signal.sendB1({ type: 'qa-spotlight', id }),
    setTimer: (minutes: number) => signal.send({ type: 'timer-set', minutes }),
    clearTimer: () => signal.send({ type: 'timer-clear' }),
  }
}

export type MeetingTools = ReturnType<typeof useMeetingTools>
