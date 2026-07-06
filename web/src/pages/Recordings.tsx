import { useEffect, useState } from 'react'
import {
  downloadRecording,
  listRecordingShares,
  recordingObjectUrl,
  recordingsLibrary,
  RecordingItem,
  roomNotes,
  RoomNotes,
  searchUsers,
  shareRecording,
  unshareRecording,
  User,
} from '../api'
import { CloseIcon, DownloadIcon, FilmIcon, NoteIcon, ShareLinkIcon, TrashIcon } from '../icons'

export default function Recordings() {
  const [items, setItems] = useState<RecordingItem[]>([])
  const [loading, setLoading] = useState(true)
  const [shareTarget, setShareTarget] = useState<RecordingItem | null>(null)
  const [viewTarget, setViewTarget] = useState<RecordingItem | null>(null)
  const [error, setError] = useState('')
  const [view, setView] = useState<'cards' | 'table'>(
    (localStorage.getItem('dx_rec_view') as 'cards' | 'table') || 'cards',
  )
  function switchView(v: 'cards' | 'table') {
    setView(v)
    localStorage.setItem('dx_rec_view', v)
  }

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
        <div className="seg view-toggle">
          <button className={view === 'cards' ? 'seg-btn active' : 'seg-btn'} onClick={() => switchView('cards')}>
            ▦ Cartões
          </button>
          <button className={view === 'table' ? 'seg-btn active' : 'seg-btn'} onClick={() => switchView('table')}>
            ☰ Tabela
          </button>
        </div>
      </header>

      {loading && <p className="muted">A carregar…</p>}
      {error && <div className="error">{error}</div>}
      {!loading && items.length === 0 && (
        <div className="empty-state">
          <FilmIcon />
          <p>Ainda não há gravações. Grava uma reunião e ela aparecerá aqui.</p>
        </div>
      )}

      {view === 'table' && items.length > 0 && (
        <table className="members-table rec-table">
          <thead>
            <tr>
              <th>Nome</th>
              <th>Sala</th>
              <th>Autor</th>
              <th>Data</th>
              <th>Tamanho</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {items.map((r) => (
              <tr key={r.id}>
                <td>
                  <button className="link rec-name-link" onClick={() => setViewTarget(r)}>
                    {r.filename.replace(/\.webm$/, '')}
                  </button>
                  {!r.owned && <span className="rec-badge shared inline">Partilhada</span>}
                </td>
                <td className="mono">{r.room_code}</td>
                <td>{r.uploader_name}</td>
                <td>{new Date(r.created_at).toLocaleString('pt-PT')}</td>
                <td>{(r.size_bytes / 1_048_576).toFixed(1)} MB</td>
                <td className="rec-row-actions">
                  <button className="icon-btn" title="Abrir" onClick={() => setViewTarget(r)}>▶</button>
                  <button
                    className="icon-btn"
                    title="Descarregar"
                    onClick={() => void downloadRecording(r).catch(() => setError('Falha ao descarregar'))}
                  >
                    <DownloadIcon />
                  </button>
                  {r.owned && (
                    <button className="icon-btn" title="Partilhar" onClick={() => setShareTarget(r)}>
                      <ShareLinkIcon />
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {view === 'cards' && (
      <div className="rec-cards">
        {items.map((r) => (
          <div key={r.id} className="rec-card">
            <button className="rec-thumb" onClick={() => setViewTarget(r)} title="Reproduzir e ver ata">
              <FilmIcon />
              <span className="rec-play">▶</span>
              {!r.owned && <span className="rec-badge shared">Partilhada</span>}
            </button>
            <div className="rec-body">
              <strong className="rec-title">{r.filename}</strong>
              <div className="rec-meta">
                Sala {r.room_code} · {r.uploader_name}
                <br />
                {new Date(r.created_at).toLocaleString('pt-PT')} · {(r.size_bytes / 1_048_576).toFixed(1)} MB
              </div>
              <div className="rec-actions">
                <button className="btn-sm" onClick={() => setViewTarget(r)}>
                  <NoteIcon /> Abrir
                </button>
                <button className="btn-sm ghost" onClick={() => void downloadRecording(r).catch(() => setError('Falha ao descarregar'))}>
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
      )}

      {shareTarget && (
        <ShareModal
          rec={shareTarget}
          onClose={() => {
            setShareTarget(null)
            void refresh()
          }}
        />
      )}
      {viewTarget && <ViewerModal rec={viewTarget} onClose={() => setViewTarget(null)} />}
    </div>
  )
}

/** Leitor: vídeo da gravação + abas Transcrição / Ata (MoM) da reunião associada. */
function ViewerModal({ rec, onClose }: { rec: RecordingItem; onClose: () => void }) {
  const [videoUrl, setVideoUrl] = useState('')
  const [notes, setNotes] = useState<RoomNotes | null>(null)
  const [tab, setTab] = useState<'trans' | 'mom'>('trans')
  const [videoErr, setVideoErr] = useState('')

  useEffect(() => {
    let url = ''
    void recordingObjectUrl(rec)
      .then((u) => { url = u; setVideoUrl(u) })
      .catch(() => setVideoErr('Não foi possível carregar o vídeo'))
    void roomNotes(rec.room_code).then(setNotes).catch(() => setNotes({ title: '', minutes: '', transcript: '' }))
    return () => { if (url) URL.revokeObjectURL(url) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rec.id])

  const hasNotes = !!(notes && (notes.minutes || notes.transcript))
  // Transcrição guardada como linhas "[hh:mm] Nome: texto".
  const lines = notes?.transcript ? notes.transcript.split('\n').filter(Boolean) : []

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal viewer" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>{notes?.title || rec.filename.replace(/\.webm$/, '')}</h3>
          <button className="panel-close" onClick={onClose}>
            <CloseIcon />
          </button>
        </div>

        {videoErr && <div className="error">{videoErr}</div>}
        {!videoUrl && !videoErr && <p className="muted">A carregar vídeo…</p>}
        {videoUrl && <video className="viewer-video" src={videoUrl} controls autoPlay />}

        <div className="viewer-tabs">
          <button className={tab === 'trans' ? 'auth-tab active' : 'auth-tab'} onClick={() => setTab('trans')}>
            Transcrição
          </button>
          <button className={tab === 'mom' ? 'auth-tab active' : 'auth-tab'} onClick={() => setTab('mom')}>
            Ata (MoM)
          </button>
        </div>

        {!hasNotes && (
          <p className="muted small">
            Esta gravação não tem notas associadas — ativa as «Notas AI» durante a reunião para gerar
            transcrição e ata.
          </p>
        )}
        {tab === 'trans' && lines.length > 0 && (
          <div className="viewer-transcript">
            {lines.map((l, i) => {
              const m = /^\[(\d{2}:\d{2})\]\s*([^:]+):\s*(.*)$/.exec(l)
              return m ? (
                <p key={i} className="tr-line">
                  <span className="tr-time mono">{m[1]}</span>
                  <strong>{m[2]}</strong>
                  <span>{m[3]}</span>
                </p>
              ) : (
                <p key={i} className="tr-line plain">{l}</p>
              )
            })}
          </div>
        )}
        {tab === 'mom' && notes?.minutes && <pre className="viewer-mom">{notes.minutes}</pre>}
        {tab === 'mom' && hasNotes && !notes?.minutes && (
          <p className="muted small">Sem ata guardada — só transcrição.</p>
        )}
      </div>
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
