import { useEffect, useState } from 'react'

/** `prefers-reduced-motion: reduce`, seguido ao vivo (o sistema pode mudar a meio). */
export function useReducedMotion(): boolean {
  const q = '(prefers-reduced-motion: reduce)'
  const [reduced, setReduced] = useState(() => typeof window !== 'undefined' && !!window.matchMedia?.(q).matches)
  useEffect(() => {
    const mq = window.matchMedia?.(q)
    if (!mq) return
    const on = () => setReduced(mq.matches)
    on()
    mq.addEventListener('change', on)
    return () => mq.removeEventListener('change', on)
  }, [])
  return reduced
}

interface BatteryLike extends EventTarget {
  level: number
  charging: boolean
}

/**
 * Bateria abaixo de 20 % e sem carregador. `false` onde a API não existe
 * (Firefox, Safari): não saber não é o mesmo que estar fraca.
 */
export function useBatteryLow(): boolean {
  const [low, setLow] = useState(false)
  useEffect(() => {
    const nav = navigator as Navigator & { getBattery?: () => Promise<BatteryLike> }
    if (!nav.getBattery) return
    let bat: BatteryLike | null = null
    let alive = true
    const on = () => {
      if (alive && bat) setLow(!bat.charging && bat.level < 0.2)
    }
    nav
      .getBattery()
      .then((b) => {
        bat = b
        on()
        b.addEventListener('levelchange', on)
        b.addEventListener('chargingchange', on)
      })
      .catch(() => {})
    return () => {
      alive = false
      bat?.removeEventListener('levelchange', on)
      bat?.removeEventListener('chargingchange', on)
    }
  }, [])
  return low
}

export function saveDataOn(): boolean {
  return Boolean((navigator as { connection?: { saveData?: boolean } }).connection?.saveData)
}

export function readPref(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback
  } catch {
    return fallback
  }
}

export function writePref(key: string, value: string): void {
  try {
    localStorage.setItem(key, value)
  } catch {
    /* sem armazenamento: vale para esta sessão */
  }
}
