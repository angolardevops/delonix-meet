import { useEffect, useRef, useState } from 'react'
import { CloseIcon, LockIcon, MicOffIcon, ShareIcon } from '../icons'
import { useTranslation } from 'react-i18next'
import { currentUser, joinRoom } from '../api'
import { ClientMsg, PeerInfo, Signaling } from '../signaling'
import { BrandMark } from '../components/BrandMark'

/** Sala de espera dedicada do anfitrião (vista do template): gerir a fila,
 *  quem está na reunião e as definições da sala — antes de entrar com media.
 *  REGRA (R2): esta página NUNCA cria SfuCall — é só sinalização; a media
 *  nasce apenas dentro da Room, depois de `joined`. */
export default function Lobby({ code }: { code: string }) {
  const { t } = useTranslation()
  const signalRef = useRef<Signaling | null>(null)
  const [status, setStatus] = useState('')
  const [connected, setConnected] = useState(false)
  const [peers, setPeers] = useState<PeerInfo[]>([])
  const [waiting, setWaiting] = useState<PeerInfo[]>([])
  const [locked, setLocked] = useState(false)
  const [hostShare, setHostShare] = useState(false)

  useEffect(() => {
    let cancelled = false
    let signal: Signaling | null = null
    void (async () => {
      try {
        const { room, room_token } = await joinRoom(code)
        if (cancelled) return
        // Só o anfitrião gere a sala de espera; convidados vão direto à sala.
        if (room.owner_id !== currentUser()?.id) {
          location.hash = `/r/${code}`
          return
        }
        signal = new Signaling(room_token, code)
        signalRef.current = signal
        signal.on('joined', (m) => {
          setConnected(true)
          setPeers(m.peers)
        })
        signal.on('peer-joined', (m) =>
          setPeers((p) => [...p.filter((x) => x.peer_id !== m.peer.peer_id), m.peer]))
        signal.on('peer-left', (m) => setPeers((p) => p.filter((x) => x.peer_id !== m.peer_id)))
        signal.on('waiting-join', (m) =>
          setWaiting((q) => [...q.filter((x) => x.peer_id !== m.peer.peer_id), m.peer]))
        signal.on('waiting-left', (m) => setWaiting((q) => q.filter((x) => x.peer_id !== m.peer_id)))
        signal.on('room-settings', (m) => {
          setLocked(m.locked)
          setHostShare(m.host_share_only)
        })
        signal.on('media', (m) =>
          setPeers((p) => p.map((x) => (x.peer_id === m.from ? { ...x, cam: m.cam, mic: m.mic } : x))))
        signal.on('error', (m) => setStatus(m.message))
      } catch (e) {
        setStatus((e as Error).message)
      }
    })()
    return () => {
      cancelled = true
      signal?.close()
    }
  }, [code])

  const send = (msg: ClientMsg) => signalRef.current?.send(msg)
  const me = currentUser()

  return (
    <div className="lobby-page">
      <header className="lobby-head">
        <a href="#/" className="brand-text">
          <BrandMark /> Delonix <span>Meet</span>
        </a>
        <span className="mono lobby-code">{code}</span>
      </header>

      <main className="lobby-body">
        <div className="lobby-title-row">
          <div>
            <h1>{t('lobby.title')}</h1>
            <p className="muted">{t('lobby.sub')}</p>
          </div>
          <button className="primary" onClick={() => (location.hash = `/r/${code}`)}>
            {t('lobby.enter')}
          </button>
        </div>

        {status && <div className="error">{status}</div>}
        {!connected && !status && <p className="muted">{t('lobby.connecting')}</p>}

        <div className="lobby-toggles">
          <label className="lobby-toggle">
            <span className="lobby-toggle-ic lock"><LockIcon /></span>
            <span>
              <strong>{t('lobby.lock')}</strong>
              <small>{t('lobby.lockDesc')}</small>
            </span>
            <span className="dx-switch">
              <input
                type="checkbox"
                checked={locked}
                onChange={(e) => {
                  setLocked(e.target.checked)
                  send({ type: 'room-lock', locked: e.target.checked })
                }}
              />
              <span className="track" />
            </span>
          </label>
          <label className="lobby-toggle">
            <span className="lobby-toggle-ic screen"><ShareIcon /></span>
            <span>
              <strong>{t('lobby.hostShare')}</strong>
              <small>{t('lobby.hostShareDesc')}</small>
            </span>
            <span className="dx-switch">
              <input
                type="checkbox"
                checked={hostShare}
                onChange={(e) => {
                  setHostShare(e.target.checked)
                  send({ type: 'host-share-only', on: e.target.checked })
                }}
              />
              <span className="track" />
            </span>
          </label>
        </div>

        <div className="lobby-grid">
          <section className="dash-card">
            <header className="dash-card-head">
              <h2>
                {t('lobby.waitingTitle')} <span className="count-badge">{waiting.length}</span>
              </h2>
              {waiting.length > 1 && (
                <button
                  className="lobby-admit all"
                  onClick={() => waiting.forEach((w) => send({ type: 'admit', to: w.peer_id }))}
                >
                  {t('lobby.admitAll')}
                </button>
              )}
            </header>
            {waiting.length === 0 && <p className="dash-empty">{t('lobby.empty')}</p>}
            {waiting.map((w) => (
              <div key={w.peer_id} className="lobby-row">
                <span className="avatar-circle small">{w.username.slice(0, 2).toUpperCase()}</span>
                <span className="lobby-row-name">
                  <strong>{w.username}</strong>
                  <small>{t('lobby.wants')}</small>
                </span>
                <button
                  className="lobby-deny"
                  title={t('lobby.deny')}
                  onClick={() => send({ type: 'deny', to: w.peer_id })}
                >
                  <CloseIcon />
                </button>
                <button className="lobby-admit" onClick={() => send({ type: 'admit', to: w.peer_id })}>
                  {t('lobby.admit')}
                </button>
              </div>
            ))}
          </section>

          <section className="dash-card">
            <header className="dash-card-head">
              <h2>
                {t('lobby.inTitle')} <span className="count-badge">{peers.length + (connected ? 1 : 0)}</span>
              </h2>
            </header>
            {connected && (
              <div className="lobby-row">
                <span className="avatar-circle small">{(me?.username ?? 'eu').slice(0, 2).toUpperCase()}</span>
                <span className="lobby-row-name">
                  <strong>{me?.username ?? 'eu'}</strong>
                  <small>{t('lobby.you')}</small>
                </span>
                <span className="posture-tag on">HOST</span>
              </div>
            )}
            {peers.map((p) => (
              <div key={p.peer_id} className="lobby-row">
                <span className="avatar-circle small">{p.username.slice(0, 2).toUpperCase()}</span>
                <span className="lobby-row-name">
                  <strong>{p.username}</strong>
                  <small>{p.host ? t('lobby.roleHost') : t('lobby.roleMember')}</small>
                </span>
                <button
                  className={p.mic ? 'lobby-deny lobby-mic' : 'lobby-deny lobby-mic off'}
                  title={p.mic ? t('lobby.mute') : t('lobby.micOff')}
                  disabled={!p.mic}
                  onClick={() => send({ type: 'force-mute', to: p.peer_id })}
                >
                  <MicOffIcon />
                </button>
                <button
                  className="lobby-deny"
                  title={t('lobby.remove')}
                  onClick={() => send({ type: 'kick', to: p.peer_id })}
                >
                  <CloseIcon />
                </button>
              </div>
            ))}
          </section>
        </div>
      </main>
    </div>
  )
}
