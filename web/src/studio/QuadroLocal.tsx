/**
 * O quadro local: quando a cena é «Quadro branco», escreve-se directamente
 * sobre o palco com o rato, a caneta ou o dedo. Os traços vão para um canvas
 * do compositor (em fracções, como o recorte), que o desenha como conteúdo —
 * é imagem, não uma sala partilhada.
 */
import { PointerEvent as ReactPointerEvent, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '../ui/kit'
import { TINTAS_DO_QUADRO } from './palco'

/** Os nomes das tintas, pela ordem de `TINTAS_DO_QUADRO`. */
const NOMES_DAS_TINTAS = ['preto', 'vermelho', 'azul', 'verde'] as const

export default function QuadroLocal({
  onRiscar,
  onLimpar,
}: {
  onRiscar: (de: [number, number], ate: [number, number], cor: string, espessura: number) => void
  onLimpar: () => void
}) {
  const { t } = useTranslation()
  const [tinta, setTinta] = useState(0)
  const [borracha, setBorracha] = useState(false)
  const ultimo = useRef<[number, number] | null>(null)

  function ponto(e: ReactPointerEvent<HTMLDivElement>): [number, number] {
    const r = e.currentTarget.getBoundingClientRect()
    return [Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), Math.min(1, Math.max(0, (e.clientY - r.top) / r.height))]
  }
  const cor = borracha ? '#ffffff' : TINTAS_DO_QUADRO[tinta]
  const espessura = borracha ? 0.04 : 0.008

  return (
    <div className="st-board">
      <div
        className="st-board__area"
        data-studio="quadro"
        onPointerDown={(e) => {
          // Sem isto o arrasto vira selecção de texto e o browser cancela o traço.
          e.preventDefault()
          e.currentTarget.setPointerCapture(e.pointerId)
          const p = ponto(e)
          ultimo.current = p
          onRiscar(p, p, cor, espessura)
        }}
        onPointerMove={(e) => {
          if (!ultimo.current) return
          const p = ponto(e)
          onRiscar(ultimo.current, p, cor, espessura)
          ultimo.current = p
        }}
        onPointerUp={() => (ultimo.current = null)}
        onPointerCancel={() => (ultimo.current = null)}
      />
      <div className="st-board__bar" role="toolbar" aria-label={t('studio.quadro.ferramentas')}>
        {TINTAS_DO_QUADRO.map((c, i) => (
          <button
            key={c}
            type="button"
            className="st-board__ink"
            // A amostra É a cor do traço (imagem, não interface) — ver TINTAS_DO_QUADRO.
            style={{ background: c }}
            aria-pressed={!borracha && tinta === i}
            aria-label={t(`studio.quadro.tintas.${NOMES_DAS_TINTAS[i]}`)}
            title={t(`studio.quadro.tintas.${NOMES_DAS_TINTAS[i]}`)}
            onClick={() => {
              setTinta(i)
              setBorracha(false)
            }}
          />
        ))}
        <Button size="sm" variant={borracha ? 'primary' : 'secondary'} icon="eraser" aria-pressed={borracha} onClick={() => setBorracha((b) => !b)}>
          {t('studio.quadro.borracha')}
        </Button>
        <Button size="sm" variant="secondary" icon="trash" data-studio="quadro-limpar" onClick={onLimpar}>
          {t('studio.quadro.limpar')}
        </Button>
      </div>
    </div>
  )
}
