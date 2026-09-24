import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

/**
 * Pico da mistura final, ao vivo. Lê `compositor.lerPicoMestre()` por
 * polling (~12 Hz) e escreve directamente no DOM por ref — o mesmo padrão
 * do `MicLevel` (room/MicLevel.tsx): medir som não pode fazer o painel
 * inteiro voltar a desenhar-se várias vezes por segundo.
 */
export default function NivelMestre({ ler }: { ler: () => number }) {
  const { t } = useTranslation()
  const barRef = useRef<HTMLSpanElement>(null)
  const dbRef = useRef<HTMLSpanElement>(null)
  useEffect(() => {
    let raf = 0
    let last = 0
    const tick = (agora: number) => {
      raf = requestAnimationFrame(tick)
      if (agora - last < 80) return
      last = agora
      const db = ler()
      const pct = Math.round(((db + 60) / 60) * 100)
      if (barRef.current) barRef.current.style.width = `${pct}%`
      if (dbRef.current) dbRef.current.textContent = db <= -60 ? '−∞ dB' : `${Math.round(db)} dB`
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [ler])
  return (
    <div className="st-nivel" role="presentation" title={t('studio.audio.nivelMestre')}>
      <span className="st-nivel__rotulo">{t('studio.audio.nivelMestre')}</span>
      <span className="dx-meter dx-meter--success st-nivel__meter">
        <span ref={barRef} style={{ width: 0 }} />
      </span>
      <span ref={dbRef} className="dx-num dx-muted st-nivel__db">
        −∞ dB
      </span>
    </div>
  )
}
