import { useEffect, useMemo, useRef, useState } from 'react'
import { loadChart } from '../chart'

export interface UsageWeek {
  week_start: string
  count: number
  minutes: number
}

interface Props {
  weeks: UsageWeek[]
  fmtWeek: (s: string) => string
  labels: { meetings: string; minutes: string }
}

/** Lê uma custom property CSS do :root (para o Chart.js seguir os tokens/tema). */
function cssVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return v || fallback
}

/**
 * Gráfico de uso (reuniões + minutos por semana). Usa Chart.js auto-hospedado
 * (lazy, ver ../chart.ts); se falhar a carregar, cai num FALLBACK CSS4 moderno
 * (barras com `.week-chart`) — nunca fica sem gráfico.
 */
export function UsageChart({ weeks, fmtWeek, labels }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const chartRef = useRef<{ destroy: () => void } | null>(null)
  const [failed, setFailed] = useState(false)
  // Assinatura estável dos dados → só recria o chart quando os dados mudam.
  const sig = useMemo(() => JSON.stringify(weeks), [weeks])

  useEffect(() => {
    let alive = true
    if (weeks.length === 0) return
    loadChart()
      .then((ChartAny) => {
        if (!alive || !canvasRef.current) return
        const Chart = ChartAny as new (c: HTMLCanvasElement, cfg: unknown) => { destroy: () => void }
        const accent = cssVar('--accent', '#C8201D')
        const accent2 = cssVar('--accent-2', '#EDA33B')
        const text2 = cssVar('--text-2', '#9BA3B2')
        const grid = 'rgba(255,255,255,0.06)'
        chartRef.current?.destroy()
        chartRef.current = new Chart(canvasRef.current, {
          data: {
            labels: weeks.map((w) => fmtWeek(w.week_start)),
            datasets: [
              {
                type: 'bar',
                label: labels.meetings,
                data: weeks.map((w) => w.count),
                backgroundColor: accent,
                borderRadius: 6,
                maxBarThickness: 34,
                yAxisID: 'y',
                order: 2,
              },
              {
                type: 'line',
                label: labels.minutes,
                data: weeks.map((w) => w.minutes),
                borderColor: accent2,
                backgroundColor: accent2,
                borderWidth: 2,
                tension: 0.35,
                pointRadius: 3,
                pointHoverRadius: 5,
                yAxisID: 'y1',
                order: 1,
              },
            ],
          },
          options: {
            responsive: true,
            maintainAspectRatio: false,
            interaction: { mode: 'index', intersect: false },
            plugins: {
              legend: { labels: { color: text2, usePointStyle: true, boxWidth: 8, padding: 16 } },
              tooltip: {
                backgroundColor: cssVar('--surface-2', '#12151C'),
                titleColor: cssVar('--text', '#F4F6FA'),
                bodyColor: text2,
                borderColor: grid,
                borderWidth: 1,
                padding: 10,
              },
            },
            scales: {
              x: { ticks: { color: text2 }, grid: { display: false } },
              y: {
                position: 'left',
                beginAtZero: true,
                ticks: { color: text2, precision: 0 },
                grid: { color: grid },
              },
              y1: {
                position: 'right',
                beginAtZero: true,
                ticks: { color: text2 },
                grid: { display: false },
              },
            },
          },
        })
      })
      .catch(() => {
        if (alive) setFailed(true)
      })
    return () => {
      alive = false
      chartRef.current?.destroy()
      chartRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sig])

  if (weeks.length === 0) return <p className="dash-empty">—</p>

  // ---- Fallback CSS4 moderno (barras) — quando o Chart.js não carrega ----
  if (failed) {
    const weekMax = Math.max(1, ...weeks.map((w) => w.count))
    const weekMinMax = Math.max(1, ...weeks.map((w) => w.minutes))
    return (
      <div className="week-chart" role="img" aria-label={labels.meetings}>
        {weeks.map((w) => (
          <div
            key={w.week_start}
            className="week-col"
            title={`${fmtWeek(w.week_start)}: ${w.count} · ${w.minutes} min`}
          >
            <span className="week-count">{w.count}</span>
            <span className="week-inner">
              <span className="week-bar" style={{ height: `${(w.count / weekMax) * 100}%` }} />
              <span
                className="week-bar minutes"
                style={{ height: `${(w.minutes / weekMinMax) * 100}%` }}
              />
            </span>
            <span className="week-label">{fmtWeek(w.week_start)}</span>
          </div>
        ))}
      </div>
    )
  }

  return (
    <div className="chart-canvas-wrap">
      <canvas ref={canvasRef} />
    </div>
  )
}
