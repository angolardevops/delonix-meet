import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { BrandLockup, BrandMark } from '../components/BrandMark'

interface StatusInfo {
  status: string
  api: boolean
  db: boolean
  uptime_secs: number
  version: string
}

function fmtUptime(s: number): string {
  const d = Math.floor(s / 86400)
  const h = Math.floor((s % 86400) / 3600)
  const m = Math.floor((s % 3600) / 60)
  return d > 0 ? `${d}d ${h}h ${m}m` : h > 0 ? `${h}h ${m}m` : `${m}m`
}

/** Status page pública (roadmap) — saúde dos componentes, sem autenticação. */
export default function Status() {
  const { t } = useTranslation()
  const [info, setInfo] = useState<StatusInfo | null>(null)
  const [err, setErr] = useState(false)
  const [checkedAt, setCheckedAt] = useState<Date>(new Date())

  useEffect(() => {
    const load = () =>
      fetch('/api/status')
        .then((r) => r.json())
        .then((j) => {
          setInfo(j)
          setErr(false)
          setCheckedAt(new Date())
        })
        .catch(() => setErr(true))
    void load()
    const t = setInterval(load, 15000)
    return () => clearInterval(t)
  }, [])

  const ok = !err && info?.status === 'ok'
  const rows = [
    { name: 'API / Signaling', up: !err && !!info?.api },
    { name: 'Base de dados', up: !err && !!info?.db },
    { name: 'Frontend (este site)', up: true },
  ]

  return (
    <div className="status-page">
      <div className="status-card">
        <BrandMark big />
        <h1>
          <BrandLockup suffix="— Estado do serviço" />
        </h1>
        <div className={ok ? 'status-banner ok' : 'status-banner down'}>
          {err ? '● Serviço indisponível' : ok ? '● Todos os sistemas operacionais' : '● Serviço degradado'}
        </div>
        {rows.map((r) => (
          <div key={r.name} className="status-row">
            <span>{r.name}</span>
            <span className={r.up ? 'status-pill up' : 'status-pill dn'}>{r.up ? 'operacional' : 'em baixo'}</span>
          </div>
        ))}
        {info && (
          <p className="muted small status-meta">{t('status.uptime')}<strong className="mono">{fmtUptime(info.uptime_secs)}</strong> · versão{' '}
            <span className="mono">{info.version}</span> · verificado às{' '}
            {checkedAt.toLocaleTimeString('pt-PT')} (atualiza a cada 15 s)
          </p>
        )}
        <a className="link" href="#/">
          ← Voltar ao Delonix Meet
        </a>
      </div>
    </div>
  )
}
