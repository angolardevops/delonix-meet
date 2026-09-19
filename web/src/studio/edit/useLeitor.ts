/**
 * Leitor da pré-visualização: a linha de tempo tocada com elementos de media,
 * sem exportar nada.
 *
 * Um relógio próprio (`performance.now`) é a verdade; cada faixa tem o seu
 * elemento e é ACERTADA a esse relógio — só se procura quando o desvio passa
 * de 0,3 s a tocar, para não soluçar. Mudar de clipe é mudar o `src` e a
 * posição do elemento da faixa.
 *
 * O que a pré-visualização NÃO faz, e diz no painel: a redução de ruído, o
 * alvo LUFS e a normalização só se ouvem na exportação.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { Clip, FaixaDeClipe, Projecto } from './projecto'
import { clipEm, duracaoDoProjecto, estadoDaFaixa, tempoNaFonte } from './projecto'
import { dbParaGanho } from './sinal'

export type Elementos = Partial<Record<FaixaDeClipe, HTMLMediaElement | null>>

export function useLeitor(projecto: Projecto | null, urls: Map<string, string>) {
  const [tempo, setTempo] = useState(0)
  const [aTocar, setATocar] = useState(false)
  const [ritmo, setRitmo] = useState(1)
  const tempoRef = useRef(0)
  const aTocarRef = useRef(false)
  const ritmoRef = useRef(1)
  const projRef = useRef(projecto)
  const urlsRef = useRef(urls)
  const elementos = useRef<Elementos>({})
  const [activos, setActivos] = useState<Partial<Record<FaixaDeClipe, Clip | null>>>({})
  projRef.current = projecto
  urlsRef.current = urls
  ritmoRef.current = ritmo

  const sincronizar = useCallback((forcar: boolean) => {
    const p = projRef.current
    if (!p) return
    const T = tempoRef.current
    const novos: Partial<Record<FaixaDeClipe, Clip | null>> = {}
    for (const faixa of ['V1', 'V2', 'A1', 'A2'] as const) {
      const el = elementos.current[faixa]
      const c = clipEm(p, faixa, T)
      novos[faixa] = c
      if (!el) continue
      const estado = estadoDaFaixa(p, faixa)
      const url = c ? urlsRef.current.get(c.fonteId) : undefined
      if (!c || !url || !estado.visivel) {
        if (!el.paused) el.pause()
        continue
      }
      if (el.dataset.fonte !== c.fonteId) {
        el.dataset.fonte = c.fonteId
        el.src = url
      }
      const alvo = tempoNaFonte(c, T)
      if (faixa === 'A1' || faixa === 'A2') el.volume = Math.max(0, Math.min(1, dbParaGanho(c.ganhoDb + estado.ganhoDb)))
      if (c.congelado !== null || !aTocarRef.current) {
        if (!el.paused) el.pause()
        if (forcar || Math.abs(el.currentTime - alvo) > 1 / 60) el.currentTime = alvo
        continue
      }
      el.playbackRate = c.velocidade * ritmoRef.current
      if (forcar || Math.abs(el.currentTime - alvo) > 0.3) el.currentTime = alvo
      if (el.paused) void el.play().catch(() => undefined)
    }
    setActivos((a) => (['V1', 'V2', 'A1', 'A2'] as const).some((f) => a[f]?.id !== novos[f]?.id || a[f] !== novos[f]) ? novos : a)
  }, [])

  // Relógio.
  useEffect(() => {
    let raf = 0
    let antes = performance.now()
    let ultimoEstado = 0
    const passo = (agora: number) => {
      const dt = (agora - antes) / 1000
      antes = agora
      const p = projRef.current
      if (aTocarRef.current && p) {
        const fim = duracaoDoProjecto(p)
        tempoRef.current = Math.min(fim, tempoRef.current + dt * ritmoRef.current)
        if (tempoRef.current >= fim) {
          aTocarRef.current = false
          setATocar(false)
        }
        sincronizar(false)
        // O React só é avisado ~15 vezes por segundo — a linha de tempo não
        // precisa de 60 renders por segundo para parecer contínua.
        if (agora - ultimoEstado > 66) {
          ultimoEstado = agora
          setTempo(tempoRef.current)
        }
      }
      raf = requestAnimationFrame(passo)
    }
    raf = requestAnimationFrame(passo)
    return () => cancelAnimationFrame(raf)
  }, [sincronizar])

  // Mudou o projecto (uma edição) ou chegou uma fonte: volta a acertar.
  useEffect(() => {
    sincronizar(false)
  }, [projecto, urls, sincronizar])

  const buscar = useCallback(
    (t: number) => {
      const p = projRef.current
      const fim = p ? duracaoDoProjecto(p) : 0
      tempoRef.current = Math.max(0, Math.min(fim, t))
      setTempo(tempoRef.current)
      sincronizar(true)
    },
    [sincronizar],
  )

  const tocar = useCallback(
    (sim: boolean) => {
      const p = projRef.current
      if (sim && p && tempoRef.current >= duracaoDoProjecto(p) - 0.05) tempoRef.current = 0
      aTocarRef.current = sim
      setATocar(sim)
      sincronizar(true)
      if (!sim) {
        for (const el of Object.values(elementos.current)) el?.pause()
        setTempo(tempoRef.current)
      }
    },
    [sincronizar],
  )

  // Refs ESTÁVEIS por faixa: uma função nova a cada render faria o React
  // desligar e religar o elemento — e cada religação forçava uma procura.
  const ligar = useMemo(() => {
    const fazer = (faixa: FaixaDeClipe) => (el: HTMLMediaElement | null) => {
      if (elementos.current[faixa] === el) return
      elementos.current[faixa] = el
      if (el) sincronizar(true)
    }
    return { V1: fazer('V1'), V2: fazer('V2'), A1: fazer('A1'), A2: fazer('A2') }
  }, [sincronizar])

  return { tempo, tempoRef, aTocar, tocar, buscar, ritmo, setRitmo, ligar, activos }
}

export type Leitor = ReturnType<typeof useLeitor>
