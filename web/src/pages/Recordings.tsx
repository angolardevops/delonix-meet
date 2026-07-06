import { useEffect, useState } from 'react'
import {
  downloadRecording,
  listRecordingShares,
  recordingsLibrary,
  RecordingItem,
  searchUsers,
  shareRecording,
  unshareRecording,
  User,
} from '../api'
import { CloseIcon, DownloadIcon, FilmIcon, ShareLinkIcon, TrashIcon } from '../icons'

export default function Recordings() {
  const [items, setItems] = useState<RecordingItem[]>([])
  const [loading, setLoading] = useState(true)
  const [shareTarget, setShareTarget] = useState<RecordingItem | null>(null)
  const [error, setError] = useState('')

  async function refresh() {
    try {
      setItems(await recordingsLibrary())
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => {
    void refresh()
  }, [])

  return (
    <div className="page">
      <header className="page-head">
        <h1>
          <FilmIcon /> Gravações
        </h1>
        <p className="muted">Reuniões em que participaste e gravações partilhadas contigo.</p>
      </header>

      {loading && <p className="muted">A carregar…</p>}
      {error && <div className="error">{error}</div>}
      {!loading && items.length === 0 && (
        <div className="empty-state">
          <FilmIcon />
          <p>Ainda não há gravações. Grava uma reunião e ela aparecerá aqui.</p>
        </div>
      )}

      <div className="rec-cards">
        {items.map((r) => (
          <div key={r.id} className="rec-card">
            <div className="rec-thumb">
              <FilmIcon />
              {!r.owned && <span className="rec-badge shared">Partilhada</span>}
            </div>
            <div className="rec-body">
              <strong className="rec-title">{r.filename}</strong>
              <div className="rec-meta">
                Sala {r.room_code} · {r.uploader_name}
                <br />
                {new Date(r.created_at).toLocaleString('pt-PT')} · {(r.size_bytes / 1_048_576).toFixed(1)} MB
              </div>
              <div className="rec-actions">
                <button className="btn-sm" onClick={() => void downloadRecording(r).catch(() => setError('Falha ao descarregar'))}>
                  <DownloadIcon /> Descarregar
                </button>
                {r.owned && (
                  <button className="btn-sm ghost" onClick={() => setShareTarget(r)}>
                    <ShareLinkIcon /> Partilhar{r.share_count > 0 ? ` (${r.share_count})` : ''}
                  </button>
                )}
              </div>
            </div>
          </div>
        ))}
      </div>

      {shareTarget && (
        <ShareModal
          rec={shareTarget}
          onClose={() => {
            setShareTarget(null)
            void refresh()
          }}
        />
      )}
    </div>
  )
}

function ShareModal({ rec, onClose }: { rec: RecordingItem; onClose: () => void }) {
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<User[]>([])
  const [shared, setShared] = useState<User[]>([])
  const [busy, setBusy] = useState(false)

  async function loadShares() {
    setShared(await listRecordingShares(rec.id).catch(() => []))
  }
  useEffect(() => {
    void loadShares()
  }, [])

  useEffect(() => {
    if (query.trim().length < 2) {
      setResults([])
      return
    }
    const t = setTimeout(async () => {
      setResults(await searchUsers(query).catch(() => []))
    }, 250)
    return () => clearTimeout(t)
  }, [query])

  async function add(u: User) {
    setBusy(true)
    try {
      await shareRecording(rec.id, u.id)
      setQuery('')
      setResults([])
      await loadShares()
    } finally {
      setBusy(false)
    }
  }
  async function remove(u: User) {
    await unshareRecording(rec.id, u.id)
    await loadShares()
  }

  const sharedIds = new Set(shared.map((s) => s.id))

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>Partilhar gravação</h3>
          <button className="panel-close" onClick={onClose}>
            <CloseIcon />
          </button>
        </div>
        <p className="muted small">
          Quem adicionares poderá <strong>ver e descarregar</strong> — só de leitura, não pode voltar a partilhar.
        </p>
        <input
          autoFocus
          placeholder="Procurar por nome ou email…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        {results.length > 0 && (
          <div className="user-results">
            {results.map((u) => (
              <button key={u.id} className="user-result" disabled={busy || sharedIds.has(u.id)} onClick={() => void add(u)}>
                <span className="avatar-circle small">{u.username.slice(0, 2).toUpperCase()}</span>
                <span className="user-info">
                  <strong>{u.username}</strong>
                  <small>{u.email}</small>
                </span>
                {sharedIds.has(u.id) ? <span className="muted small">já tem acesso</span> : <span className="add-plus">+</span>}
              </button>
            ))}
          </div>
        )}
        <div className="shared-list">
          <h4>Com acesso ({shared.length})</h4>
          {shared.length === 0 && <p className="muted small">Ainda não partilhaste com ninguém.</p>}
          {shared.map((u) => (
            <div key={u.id} className="shared-row">
              <span className="avatar-circle small">{u.username.slice(0, 2).toUpperCase()}</span>
              <span className="user-info">
                <strong>{u.username}</strong>
                <small>{u.email}</small>
              </span>
              <button className="icon-btn" title="Remover acesso" onClick={() => void remove(u)}>
                <TrashIcon />
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
