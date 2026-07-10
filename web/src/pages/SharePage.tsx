import { useEffect, useState } from 'react'
import { getPublicShare, PublicShareInfo } from '../api'
import { FilmIcon } from '../icons'

export default function SharePage({ token }: { token: string }) {
  const [info, setInfo] = useState<PublicShareInfo | null>(null)
  const [needsPassword, setNeedsPassword] = useState(false)
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)

  async function load(pw?: string) {
    setError('')
    setLoading(true)
    try {
      const data = await getPublicShare(token, pw)
      setInfo(data)
      setNeedsPassword(false)
    } catch (e: unknown) {
      const status = (e as { status?: number }).status
      if (status === 401) {
        setNeedsPassword(true)
      } else if (status === 404) {
        setError('Link inválido ou expirado.')
      } else {
        setError((e as Error).message ?? 'Erro ao carregar gravação.')
      }
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token])

  if (loading) {
    return (
      <div className="share-page">
        <p className="muted">A verificar link…</p>
      </div>
    )
  }

  if (error) {
    return (
      <div className="share-page">
        <div className="share-page-card">
          <FilmIcon />
          <h2>Gravação indisponível</h2>
          <p className="muted">{error}</p>
          <a href="#/" className="btn-sm">Ir para o início</a>
        </div>
      </div>
    )
  }

  if (needsPassword && !info) {
    return (
      <div className="share-page">
        <div className="share-page-card">
          <FilmIcon />
          <h2>Gravação protegida</h2>
          <p className="muted">Este link requer uma password para aceder.</p>
          <form
            onSubmit={(e) => { e.preventDefault(); void load(password) }}
            className="share-password-form"
          >
            <input
              type="password"
              autoFocus
              placeholder="Password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
            {error && <p className="error small">{error}</p>}
            <button className="btn-sm" type="submit">Aceder</button>
          </form>
        </div>
      </div>
    )
  }

  if (!info) return null

  const sizeMb = (info.size_bytes / 1_048_576).toFixed(1)
  const date = new Date(info.created_at).toLocaleString('pt-PT')
  const downloadUrl = `/api/share/${token}/download${password ? `?password=${encodeURIComponent(password)}` : ''}`

  return (
    <div className="share-page">
      <div className="share-page-card">
        <FilmIcon />
        <h2>{info.filename.replace(/\.webm$/, '')}</h2>
        <p className="muted">{date} · {sizeMb} MB</p>
        <a className="btn-sm" href={downloadUrl} download={info.filename}>
          ⬇ Descarregar
        </a>
        <p className="muted small share-link-password-hint">
          Powered by <strong>Delonix Meet</strong>
        </p>
      </div>
    </div>
  )
}
