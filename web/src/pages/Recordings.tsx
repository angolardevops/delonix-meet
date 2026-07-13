import { useEffect, useRef, useState } from 'react'
import { Trans, useTranslation } from 'react-i18next'
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
import PageHeader from '../components/PageHeader'
import EmptyState from '../components/EmptyState'

/** Thumb duotone estável por nome (mesma técnica dos tiles da sala). */
function recColor(name: string): string {
  let h = 5381
  for (let i = 0; i < name.length; i++) h = ((h << 5) + h + name.charCodeAt(i)) | 0
  const hue = Math.abs(h) % 360
  return `linear-gradient(135deg, hsl(${hue}, 42%, 30%), hsl(${(hue + 35) % 360}, 48%, 18%))`
}

export default function Recordings() {
  const { t } = useTranslation()
  const [items, setItems] = useState<RecordingItem[]>([])
  const [loading, setLoading] = useState(true)
  const [shareTarget, setShareTarget] = useState<RecordingItem | null>(null)
  const [viewTarget, setViewTarget] = useState<RecordingItem | null>(null)
  const [error, setError] = useState('')
  const [view, setView] = useState<'library' | 'cards' | 'table'>(
    (localStorage.getItem('dx_rec_view') as 'library' | 'cards' | 'table') || 'library',
  )
  const [search, setSearch] = useState('')
  // Vista biblioteca (template): item selecionado abre no leitor à direita.
  const [selected, setSelected] = useState<RecordingItem | null>(null)
  function switchView(v: 'library' | 'cards' | 'table') {
    setView(v)
    localStorage.setItem('dx_rec_view', v)
  }

  async function refresh() {
    try {
      const list = await recordingsLibrary()
      setItems(list)
      setSelected((cur) => cur ?? list[0] ?? null)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => {
    void refresh()
  }, [])

  const q = search.trim().toLowerCase()
  const shown = q
    ? items.filter(
        (r) =>
          r.filename.toLowerCase().includes(q) ||
          r.room_code.toLowerCase().includes(q) ||
          r.uploader_name.toLowerCase().includes(q),
      )
    : items

  return (
    <div className="page">
      <PageHeader
        icon={<FilmIcon />}
        title={t('recordings.title')}
        subtitle={t('recordings.subtitle')}
        actions={
          <div className="seg view-toggle">
            <button className={view === 'library' ? 'seg-btn active' : 'seg-btn'} onClick={() => switchView('library')}>
              ▤ {t('recordings.viewLibrary')}
            </button>
            <button className={view === 'cards' ? 'seg-btn active' : 'seg-btn'} onClick={() => switchView('cards')}>
              ▦ {t('recordings.viewCards')}
            </button>
            <button className={view === 'table' ? 'seg-btn active' : 'seg-btn'} onClick={() => switchView('table')}>
              ☰ {t('recordings.viewTable')}
            </button>
          </div>
        }
      />

      {!loading && items.length > 0 && view !== 'library' && (
        <div className="rec-search">
          <input
            type="search"
            placeholder={t('recordings.searchPh')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            autoComplete="off"
          />
        </div>
      )}

      {loading && <p className="muted">{t('recordings.loading')}</p>}
      {error && <div className="error">{error}</div>}
      {!loading && items.length === 0 && (
        <EmptyState icon={<FilmIcon />} title={t('recordings.empty')} />
      )}
      {!loading && items.length > 0 && shown.length === 0 && view !== 'library' && (
        <p className="muted">{t('recordings.noResults')}</p>
      )}

      {view === 'library' && items.length > 0 && (
        <div className="rec-split">
          <aside className="rec-split-list">
            {/* Template: pesquisa no topo da lista, não da página. */}
            <div className="rec-search in-list">
              <input
                type="search"
                placeholder={t('recordings.searchPh')}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                autoComplete="off"
              />
            </div>
            {shown.length === 0 && <p className="muted small">{t('recordings.noResults')}</p>}
            {shown.map((r) => (
              <button
                key={r.id}
                className={selected?.id === r.id ? 'rec-item active' : 'rec-item'}
                onClick={() => setSelected(r)}
              >
                <span className="rec-item-thumb" style={{ background: recColor(r.filename) }}><FilmIcon /></span>
                <span className="rec-item-info">
                  <strong>{r.filename.replace(/\.webm$/, '')}</strong>
                  <small>
                    {new Date(r.created_at).toLocaleDateString('pt-PT', { day: 'numeric', month: 'short', year: 'numeric' })}
                    {' · '}{(r.size_bytes / 1_048_576).toFixed(1)} MB
                    {!r.owned && <> · {t('recordings.shared')}</>}
                  </small>
                </span>
              </button>
            ))}
          </aside>
          <section className="rec-split-viewer">
            {selected ? (
              <ViewerBody
                key={selected.id}
                rec={selected}
                showHeader
                onShare={() => setShareTarget(selected)}
                onError={setError}
              />
            ) : (
              <EmptyState icon={<FilmIcon />} title={t('recordings.empty')} />
            )}
          </section>
        </div>
      )}

      {view === 'table' && shown.length > 0 && (
        <table className="members-table rec-table">
          <thead>
            <tr>
              <th>{t('recordings.colName')}</th>
              <th>{t('recordings.colRoom')}</th>
              <th>{t('recordings.colAuthor')}</th>
              <th>{t('recordings.colDate')}</th>
              <th>{t('recordings.colSize')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {shown.map((r) => (
              <tr key={r.id}>
                <td>
                  <button className="link rec-name-link" onClick={() => setViewTarget(r)}>
                    {r.filename.replace(/\.webm$/, '')}
                  </button>
                  {!r.owned && <span className="rec-badge shared inline">{t('recordings.shared')}</span>}
                </td>
                <td className="mono">{r.room_code}</td>
                <td>{r.uploader_name}</td>
                <td>{new Date(r.created_at).toLocaleString('pt-PT')}</td>
                <td>{(r.size_bytes / 1_048_576).toFixed(1)} MB</td>
                <td className="rec-row-actions">
                  <button className="icon-btn" title={t('recordings.open')} onClick={() => setViewTarget(r)}>▶</button>
                  {r.can_download && (
                    <button
                      className="icon-btn"
                      title={t('recordings.download')}
                      onClick={() => void downloadRecording(r).catch((e) => setError((e as Error).message))}
                    >
                      <DownloadIcon />
                    </button>
                  )}
                  {r.owned && (
                    <button className="icon-btn" title={t('recordings.share')} onClick={() => setShareTarget(r)}>
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
        {shown.map((r) => (
          <div key={r.id} className="rec-card">
            <button className="rec-thumb" style={{ background: recColor(r.filename) }} onClick={() => setViewTarget(r)} title={t('recordings.play')}>
              <FilmIcon />
              <span className="rec-play">▶</span>
              {!r.owned && <span className="rec-badge shared">{t('recordings.shared')}</span>}
            </button>
            <div className="rec-body">
              <strong className="rec-title">{r.filename}</strong>
              <div className="rec-meta">
                {t('recordings.room', { code: r.room_code })} · {r.uploader_name}
                <br />
                {new Date(r.created_at).toLocaleString('pt-PT')} · {(r.size_bytes / 1_048_576).toFixed(1)} MB
              </div>
              <div className="rec-actions">
                <button className="btn-sm" onClick={() => setViewTarget(r)}>
                  <NoteIcon /> {t('recordings.open')}
                </button>
                {r.can_download && (
                  <button className="btn-sm ghost" onClick={() => void downloadRecording(r).catch((e) => setError((e as Error).message))}>
                    <DownloadIcon /> {t('recordings.download')}
                  </button>
                )}
                {r.owned && (
                  <button className="btn-sm ghost" onClick={() => setShareTarget(r)}>
                    <ShareLinkIcon /> {t('recordings.share')}{r.share_count > 0 ? ` (${r.share_count})` : ''}
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

/** Corpo do leitor: vídeo + (opcional) cabeçalho com meta/ações + abas
 *  Transcrição / Ata (MoM) / Tarefas. Usado inline na biblioteca e no modal. */
function ViewerBody({
  rec,
  showHeader = false,
  onShare,
  onError,
}: {
  rec: RecordingItem
  showHeader?: boolean
  onShare?: () => void
  onError?: (msg: string) => void
}) {
  const { t } = useTranslation()
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
      .catch(() => setVideoErr(t('recordings.loadVideoFail')))
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
    <div className="viewer-body">
        {videoErr && <div className="error">{videoErr}</div>}
        {!videoUrl && !videoErr && <p className="muted">{t('recordings.loadingVideo')}</p>}
        {videoUrl && (
          <div className="viewer-video-wrap">
            <video ref={viewerVideoRef} className="viewer-video" src={videoUrl} controls autoPlay />
            <button
              className="pip-btn"
              title={t('recordings.pipTitle')}
              onClick={() => {
                const v = viewerVideoRef.current as (HTMLVideoElement & { requestPictureInPicture?: () => Promise<unknown> }) | null
                v?.requestPictureInPicture?.().catch(() => setVideoErr(t('recordings.pipUnsupported')))
              }}
            >
              ⧉ {t('recordings.pip')}
            </button>
          </div>
        )}

        {showHeader && (
          <div className="viewer-head">
            <div className="viewer-head-info">
              <h3>{notes?.title || rec.filename.replace(/\.webm$/, '')}</h3>
              <small className="muted">
                {new Date(rec.created_at).toLocaleString('pt-PT')} · {(rec.size_bytes / 1_048_576).toFixed(1)} MB
                {' · '}{t('recordings.room', { code: rec.room_code })} · {rec.uploader_name}
              </small>
            </div>
            <div className="viewer-head-actions">
              {rec.can_download && (
                <button
                  className="btn-sm ghost"
                  onClick={() => void downloadRecording(rec).catch((e) => onError?.((e as Error).message))}
                >
                  <DownloadIcon /> {t('recordings.download')}
                </button>
              )}
              {rec.owned && onShare && (
                <button className="btn-sm" onClick={onShare}>
                  <ShareLinkIcon /> {t('recordings.share')}{rec.share_count > 0 ? ` (${rec.share_count})` : ''}
                </button>
              )}
            </div>
          </div>
        )}

        <div className="viewer-tabs">
          <button className={tab === 'trans' ? 'auth-tab active' : 'auth-tab'} onClick={() => setTab('trans')}>
            {t('recordings.tabTranscript')}
          </button>
          <button className={tab === 'mom' ? 'auth-tab active' : 'auth-tab'} onClick={() => setTab('mom')}>
            {t('recordings.tabMom')}
          </button>
          <button className={tab === 'tasks' ? 'auth-tab active' : 'auth-tab'} onClick={() => setTab('tasks')}>
            {t('recordings.tabTasks')}{tasks.length > 0 ? ` (${tasks.filter((t) => !t.done).length})` : ''}
          </button>
        </div>

        {!hasNotes && (
          <p className="muted small">
            {t('recordings.noNotes')}
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
          <p className="muted small">{t('recordings.noMom')}</p>
        )}
        {tab === 'tasks' && tasks.length === 0 && (
          <p className="muted small">
            {t('recordings.noTasks')}
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
  )
}

/** Leitor em modal (vistas Cartões/Tabela). */
function ViewerModal({ rec, onClose }: { rec: RecordingItem; onClose: () => void }) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal viewer" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>{rec.filename.replace(/\.webm$/, '')}</h3>
          <button className="panel-close" onClick={onClose}>
            <CloseIcon />
          </button>
        </div>
        <ViewerBody rec={rec} />
      </div>
    </div>
  )
}

function ShareModal({ rec, onClose }: { rec: RecordingItem; onClose: () => void }) {
  const { t } = useTranslation()
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

  // skipConfirm: usado pelo "Renovar" (que já é revogar+recriar de propósito).
  async function revokeLink(skipConfirm = false) {
    // Confirmação: sem ela, um duplo-clique em "Revogar" acerta no botão
    // "Gerar link" que aparece NA MESMA posição → revoga e recria de imediato
    // (parecia que "revogar recria outro link").
    if (!skipConfirm && !confirm(t('recordings.confirmRevoke'))) return
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
          <h3>{t('recordings.shareRecording')}</h3>
          <button className="panel-close" onClick={onClose}><CloseIcon /></button>
        </div>

        {/* ---- Link público ---- */}
        <div className="share-link-section">
          <h4>{t('recordings.publicLink')}</h4>
          {link === undefined && <p className="muted small">{t('recordings.loading')}</p>}
          {link === null && (
            <>
              <div className="share-link-options">
                <input
                  type="password"
                  placeholder={t('recordings.passwordOptional')}
                  value={linkPassword}
                  onChange={(e) => setLinkPassword(e.target.value)}
                />
                <label className="share-link-label">
                  {t('recordings.expiresIn')}
                  <input
                    type="datetime-local"
                    value={linkExpiry}
                    onChange={(e) => setLinkExpiry(e.target.value)}
                    min={new Date().toISOString().slice(0, 16)}
                  />
                </label>
              </div>
              <button className="btn-sm" disabled={linkBusy} onClick={() => void generateLink()}>
                {linkBusy ? t('recordings.generating') : t('recordings.generateLink')}
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
                  title={t('recordings.copyLink')}
                >
                  {linkCopied ? '✓' : '⎘'}
                </button>
              </div>
              {link.expires_at && (
                <p className="muted small share-link-password-hint">
                  {t('recordings.expiresOn', { date: new Date(link.expires_at).toLocaleString('pt-PT') })}
                </p>
              )}
              <div className="share-link-actions">
                <button className="btn-sm ghost danger" disabled={linkBusy} onClick={() => void revokeLink()}>
                  {linkBusy ? t('recordings.revoking') : t('recordings.revokeLink')}
                </button>
                <button className="btn-sm ghost" disabled={linkBusy} onClick={() => void revokeLink(true).then(() => generateLink())}>
                  {t('recordings.renew')}
                </button>
              </div>
            </div>
          )}
        </div>

        <hr className="share-divider" />

        {/* ---- Partilha com utilizadores ---- */}
        <p className="muted small">
          <Trans i18nKey="recordings.directShareHint"><strong>ver e descarregar</strong></Trans>
        </p>
        <input
          placeholder={t('recordings.searchUsers')}
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
                {sharedIds.has(u.id) ? <span className="muted small">{t('recordings.alreadyHasAccess')}</span> : <span className="add-plus">+</span>}
              </button>
            ))}
          </div>
        )}
        <div className="shared-list">
          <h4>{t('recordings.withAccess', { count: shared.length })}</h4>
          {shared.length === 0 && <p className="muted small">{t('recordings.noneShared')}</p>}
          {shared.map((u) => (
            <div key={u.id} className="shared-row">
              <span className="avatar-circle small">{u.username.slice(0, 2).toUpperCase()}</span>
              <span className="user-info">
                <strong>{u.username}</strong>
                <small>{u.email}</small>
              </span>
              <button className="icon-btn" title={t('recordings.removeAccess')} onClick={() => void remove(u)}>
                <TrashIcon />
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
