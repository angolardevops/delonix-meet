// Carregador lazy do Chart.js — AUTO-HOSPEDADO (web/public/vendor/chart.umd.min.js),
// não CDN cross-origin: respeita o self-hosted-first / soberania de dados e a CSP
// estrita (COOP/COEP require-corp) do app, que bloquearia um script cross-origin.
// Carrega "estilo-CDN" (script same-origin injetado, fora do bundle principal) só
// quando uma view com gráficos monta. Quem falhar cai no fallback CSS4 do componente.

type ChartCtor = unknown
let promise: Promise<ChartCtor> | null = null

export function loadChart(): Promise<ChartCtor> {
  const w = window as unknown as { Chart?: ChartCtor }
  if (w.Chart) return Promise.resolve(w.Chart)
  if (promise) return promise
  promise = new Promise<ChartCtor>((resolve, reject) => {
    const s = document.createElement('script')
    s.src = '/vendor/chart.umd.min.js'
    s.async = true
    s.onload = () => (w.Chart ? resolve(w.Chart) : reject(new Error('Chart global ausente')))
    s.onerror = () => {
      promise = null // permite re-tentar num próximo mount
      reject(new Error('Chart.js falhou a carregar'))
    }
    document.head.appendChild(s)
  })
  return promise
}
