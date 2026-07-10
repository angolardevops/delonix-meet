import { useEffect, useRef, useState } from 'react'
import {
  createRecordingLink,
  downloadRecording,
  getRecordingLink,
  listRecordingShares,
  recordingObjectUrl,
  recordingsLibrary,
  RecordingItem,
  revokeRecordingLink,
  roomNotes,
  RoomNotes,
  saveMinutesByRoom,
  searchUsers,
  ShareLink,
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
                  {r.can_download && (
                    <button
                      className="icon-btn"
                      title="Descarregar"
                      onClick={() => void downloadRecording(r).catch((e) => setError((e as Error).message))}
                    >
                      <DownloadIcon />
                    </button>
                  )}
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
                {r.can_download && (
                  <button className="btn-sm ghost" onClick={() => void downloadRecording(r).catch((e) => setError((e as Error).message))}>
                    <DownloadIcon /> Descarregar
                  </button>
                )}
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

/** Leitor: vídeo da gravação + abas Transcrição / Ata (MoM) / Tarefas. */
function ViewerModal({ rec, onClose }: { rec: RecordingItem; onClose: () => void }) {
  const viewerVideoRef = useRef<HTMLVideoElement>(null)
  const [videoUrl, setVideoUrl] = useState('')
  const [notes, setNotes] = useState<RoomNotes | null>(null)
  const [tab, setTab] = useState<'trans' | 'mom' | 'tasks'>('trans')
  const [videoErr, setVideoErr] = useState('')
  const [savingTask, setSavingTask] = useState(false)

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
  // Tarefas = linhas "- [ ]"/"- [x]" da ata (secção Decisões e ações).
  const momLines = notes?.minutes ? notes.minutes.split('\n') : []
  const tasks = momLines
    .map((l, idx) => ({ idx, m: /^- \[([ x])\] (.*)$/.exec(l) }))
    .filter((t): t is { idx: number; m: RegExpExecArray } => !!t.m)
    .map((t) => ({ idx: t.idx, done: t.m[1] === 'x', text: t.m[2] }))

  /** Marca/desmarca uma tarefa na própria ata e persiste. */
  async function toggleTask(lineIdx: number, done: boolean) {
    if (!notes || savingTask) return
    setSavingTask(true)
    const updated = [...momLines]
    updated[lineIdx] = updated[lineIdx].replace(/^- \[[ x]\]/, done ? '- [x]' : '- [ ]')
    const minutes = updated.join('\n')
    try {
      await saveMinutesByRoom(rec.room_code, minutes, notes.transcript)
      setNotes({ ...notes, minutes })
    } catch {
      /* mantém como estava */
    } finally {
      setSavingTask(false)
    }
  }

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
        {videoUrl && (
          <div className="viewer-video-wrap">
            <video ref={viewerVideoRef} className="viewer-video" src={videoUrl} controls autoPlay />
            <button
              className="pip-btn"
              title="Destacar em janela flutuante (Picture-in-Picture)"
              onClick={() => {
                const v = viewerVideoRef.current as (HTMLVideoElement & { requestPictureInPicture?: () => Promise<unknown> }) | null
                v?.requestPictureInPicture?.().catch(() => setVideoErr('O browser não suporta Picture-in-Picture'))
              }}
            >
              ⧉ Destacar
            </button>
          </div>
        )}

        <div className="viewer-tabs">
          <button className={tab === 'trans' ? 'auth-tab active' : 'auth-tab'} onClick={() => setTab('trans')}>
            Transcrição
          </button>
          <button className={tab === 'mom' ? 'auth-tab active' : 'auth-tab'} onClick={() => setTab('mom')}>
            Ata (MoM)
          </button>
          <button className={tab === 'tasks' ? 'auth-tab active' : 'auth-tab'} onClick={() => setTab('tasks')}>
            Tarefas{tasks.length > 0 ? ` (${tasks.filter((t) => !t.done).length})` : ''}
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
        {tab === 'tasks' && tasks.length === 0 && (
          <p className="muted small">
            Sem tarefas nesta reunião — a ata gera-as a partir de frases de ação («fica responsável»,
            «temos de», «próximo passo»…).
          </p>
        )}
        {tab === 'tasks' && tasks.length > 0 && (
          <div className="task-list">
            {tasks.map((t) => (
              <label key={t.idx} className={t.done ? 'task-row done' : 'task-row'}>
                <input
                  type="checkbox"
                  checked={t.done}
                  disabled={savingTask}
                  onChange={(e) => void toggleTask(t.idx, e.target.checked)}
                />
                <span>{t.text}</span>
              </label>
            ))}
          </div>
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

  // --- Link público ---
  const [link, setLink] = useState<ShareLink | null | undefined>(undefined) // undefined = loading
  const [linkPassword, setLinkPassword] = useState('')
  const [linkExpiry, setLinkExpiry] = useState('')
  const [linkBusy, setLinkBusy] = useState(false)
  const [linkCopied, setLinkCopied] = useState(false)

  useEffect(() => {
    void getRecordingLink(rec.id).then(setLink).catch(() => setLink(null))
  }, [rec.id])

  function linkUrl(token: string) {
    return `${location.origin}/#/share/${token}`
  }

  async function generateLink() {
    setLinkBusy(true)
    try {
      const l = await createRecordingLink(rec.id, {
        password: linkPassword || undefined,
        expires_at: linkExpiry ? new Date(linkExpiry).toISOString() : null,
      })
      setLink(l)
      setLinkPassword('')
    } finally {
      setLinkBusy(false)
    }
  }

  async function revokeLink() {
    setLinkBusy(true)
    try {
      await revokeRecordingLink(rec.id)
      setLink(null)
    } finally {
      setLinkBusy(false)
    }
  }

  function copyLink(token: string) {
    void navigator.clipboard.writeText(linkUrl(token)).then(() => {
      setLinkCopied(true)
      setTimeout(() => setLinkCopied(false), 2000)
    })
  }

  // --- Partilha por utilizador ---
  async function loadShares() {
    setShared(await listRecordingShares(rec.id).catch(() => []))
  }
  useEffect(() => {
    void loadShares()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    if (query.trim().length < 2) { setResults([]); return }
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
      <div className="modal share-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>Partilhar gravação</h3>
          <button className="panel-close" onClick={onClose}><CloseIcon /></button>
        </div>

        {/* ---- Link público ---- */}
        <div className="share-link-section">
          <h4>Link público</h4>
          {link === undefined && <p className="muted small">A carregar…</p>}
          {link === null && (
            <>
              <div className="share-link-options">
                <input
                  type="password"
                  placeholder="Password (opcional)"
                  value={linkPassword}
                  onChange={(e) => setLinkPassword(e.target.value)}
                />
                <label className="share-link-label">
                  Expira em
                  <input
                    type="datetime-local"
                    value={linkExpiry}
                    onChange={(e) => setLinkExpiry(e.target.value)}
                    min={new Date().toISOString().slice(0, 16)}
                  />
                </label>
              </div>
              <button className="btn-sm" disabled={linkBusy} onClick={() => void generateLink()}>
                {linkBusy ? 'A gerar…' : 'Gerar link'}
              </button>
            </>
          )}
          {link && (
            <div className="share-link-box">
              <div className="share-link-url">
                <span className="mono small">{linkUrl(link.token)}</span>
                <button
                  className="share-link-copy-btn"
                  onClick={() => copyLink(link.token)}
                  title="Copiar link"
                >
                  {linkCopied ? '✓' : '⎘'}
                </button>
              </div>
              {link.expires_at && (
                <p className="muted small share-link-password-hint">
                  Expira em {new Date(link.expires_at).toLocaleString('pt-PT')}
                </p>
              )}
              <div className="share-link-actions">
                <button className="btn-sm ghost danger" disabled={linkBusy} onClick={() => void revokeLink()}>
                  {linkBusy ? 'A revogar…' : 'Revogar link'}
                </button>
                <button className="btn-sm ghost" disabled={linkBusy} onClick={() => void revokeLink().then(() => generateLink())}>
                  Renovar
                </button>
              </div>
            </div>
          )}
        </div>

        <hr className="share-divider" />

        {/* ---- Partilha com utilizadores ---- */}
        <p className="muted small">
          Partilha direta: quem adicionares poderá <strong>ver e descarregar</strong>.
        </p>
        <input
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
