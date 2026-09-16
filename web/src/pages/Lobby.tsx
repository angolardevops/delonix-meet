import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, apiErrorMessage, currentUser, isAbort, joinRoom } from '../api'
import { BrandLockup } from '../components/BrandMark'
import { ClientMsg, PeerInfo, Signaling } from '../signaling'
import { Icon } from '../ui/icons'
import { Alert, Avatar, Button, Card, Empty, IconButton, Segmented, Spinner, Tag, Toggle } from '../ui/kit'
import '../ui/room.css'

type Filtro = 'todos' | 'maos' | 'semSom'

/**
 * Consola do anfitrião ANTES de entrar com media: fila da sala de espera, quem
 * já está na reunião e as regras da sala.
 *
 * REGRA (R2): esta página NUNCA cria uma chamada — é só sinalização. A media
 * nasce dentro da sala, depois de `joined`. Quem não é dono da sala é mandado
 * para a sala; e mesmo que ficasse, o servidor recusa as acções de anfitrião.
 */
export default function Lobby({ code }: { code: string }) {
  const { t } = useTranslation()
  const signalRef = useRef<Signaling | null>(null)
  const [erro, setErro] = useState('')
  const [connected, setConnected] = useState(false)
  const [roomName, setRoomName] = useState('')
  const [peers, setPeers] = useState<PeerInfo[]>([])
  const [waiting, setWaiting] = useState<PeerInfo[]>([])
  const [locked, setLocked] = useState(false)
  const [hostShare, setHostShare] = useState(false)
  const [filtro, setFiltro] = useState<Filtro>('todos')

  useEffect(() => {
    let cancelled = false
    let signal: Signaling | null = null
    void (async () => {
      try {
        const { room, room_token } = await joinRoom(code)
        if (cancelled) return
        if (room.owner_id !== currentUser()?.id) {
          location.hash = `/r/${code}`
          return
        }
        setRoomName(room.name)
        signal = new Signaling(room_token, code)
        signalRef.current = signal
        signal.on('joined', (m) => {
          setConnected(true)
          setPeers(m.peers)
        })
        signal.on('peer-joined', (m) => setPeers((p) => [...p.filter((x) => x.peer_id !== m.peer.peer_id), m.peer]))
        signal.on('peer-left', (m) => setPeers((p) => p.filter((x) => x.peer_id !== m.peer_id)))
        signal.on('waiting-join', (m) => setWaiting((q) => [...q.filter((x) => x.peer_id !== m.peer.peer_id), m.peer]))
        signal.on('waiting-left', (m) => setWaiting((q) => q.filter((x) => x.peer_id !== m.peer_id)))
        signal.on('room-settings', (m) => {
          setLocked(m.locked)
          setHostShare(m.host_share_only)
        })
        signal.on('media', (m) => setPeers((p) => p.map((x) => (x.peer_id === m.from ? { ...x, cam: m.cam, mic: m.mic } : x))))
        signal.on('hand', (m) => setPeers((p) => p.map((x) => (x.peer_id === m.from ? { ...x, hand: m.raised } : x))))
        signal.on('error', (m) => setErro(m.message))
      } catch (e) {
        // Um 5xx (ou a rede) não tem frase para a pessoa: «Internal Server Error»
        // não se mostra a ninguém. Só um 4xx traz um motivo do servidor.
        if (cancelled || isAbort(e)) return
        setErro(e instanceof ApiError && e.status < 500 ? apiErrorMessage(e, t('room.lobby.erroLigar')) : t('room.lobby.erroLigar'))
      }
    })()
    return () => {
      cancelled = true
      signal?.close()
    }
  }, [code, t])

  const send = (msg: ClientMsg) => signalRef.current?.send(msg)
  const me = currentUser()
  const admit = (peerId: string, ok: boolean) => {
    send({ type: ok ? 'admit' : 'deny', to: peerId })
    setWaiting((q) => q.filter((x) => x.peer_id !== peerId))
  }
  const maos = peers.filter((p) => p.hand).length
  const semSom = peers.filter((p) => !p.mic).length
  const lista = peers.filter((p) => (filtro === 'maos' ? p.hand : filtro === 'semSom' ? !p.mic : true))

  return (
    <div className="lb-page">
      <header className="lb-bar">
        <a href="#/" className="lb-bar__brand" aria-label={t('room.lobby.inicio')}>
          <BrandLockup size={22} />
        </a>
        <h1 className="lb-bar__title">{roomName ? t('room.lobby.tituloSala', { nome: roomName }) : t('room.lobby.titulo')}</h1>
        <span className="dx-num dx-muted lb-bar__code">{code}</span>
        <span className="dx-spacer" />
        <Button
          variant="secondary"
          icon="micOff"
          aria-label={t('room.lobby.silenciarTodos')}
          disabled={!connected || peers.length === 0}
          onClick={() => send({ type: 'mute-all', allow_unmute: true })}
        >
          <span className="lb-hide-narrow">{t('room.lobby.silenciarTodos')}</span>
        </Button>
        <Button variant="primary" icon="video" onClick={() => (location.hash = `/r/${code}`)}>
          {t('room.lobby.entrar')}
        </Button>
      </header>

      <main className="lb-body">
        {erro && <Alert tone="danger">{erro}</Alert>}
        {!connected && !erro && (
          <p className="lb-connecting dx-muted" role="status">
            <Spinner /> {t('room.lobby.aLigar')}
          </p>
        )}

        <div className="lb-grid">
          <Card
            flush
            title={t('room.lobby.participantes')}
            eyebrow={t('room.lobby.resumo', { naSala: peers.length + (connected ? 1 : 0), espera: waiting.length })}
            actions={
              <Segmented<Filtro>
                label={t('room.lobby.filtro')}
                value={filtro}
                onChange={setFiltro}
                options={[
                  { value: 'todos', label: t('room.lobby.todos') },
                  { value: 'maos', label: t('room.lobby.maosNoAr', { n: maos }) },
                  { value: 'semSom', label: t('room.lobby.semSom', { n: semSom }) },
                ]}
              />
            }
          >
            <div className="dx-table-wrap">
              <table className="dx-table lb-table">
                <thead>
                  <tr>
                    <th scope="col">{t('room.lobby.pessoa')}</th>
                    <th scope="col">{t('room.lobby.papel')}</th>
                    <th scope="col">{t('room.lobby.estado')}</th>
                    <th scope="col">
                      <span className="dx-sr-only">{t('room.lobby.accoes')}</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {connected && filtro === 'todos' && (
                    <tr>
                      <td>
                        <span className="lb-person">
                          <Avatar name={me?.username ?? ''} size={28} />
                          <span>
                            <strong>{me?.username}</strong>
                            <small className="dx-muted">{t('room.lobby.tu')}</small>
                          </span>
                        </span>
                      </td>
                      <td>
                        <Tag tone="accent">{t('room.papel.anfitriao')}</Tag>
                      </td>
                      <td className="dx-muted">{t('room.lobby.naConsola')}</td>
                      <td />
                    </tr>
                  )}
                  {lista.map((p) => (
                    <tr key={p.peer_id}>
                      <td>
                        <span className="lb-person">
                          <Avatar name={p.username} size={28} />
                          <span>
                            <strong>{p.username}</strong>
                            {(p.is_pstn || p.is_bot) && (
                              <small className="dx-muted">{p.is_pstn ? t('room.papel.telefone') : t('room.papel.assistente')}</small>
                            )}
                          </span>
                          {p.hand && (
                            <Tag tone="live">
                              <Icon name="hand" size={10} />
                              {t('room.tile.mao')}
                            </Tag>
                          )}
                        </span>
                      </td>
                      <td>{p.host ? <Tag tone="accent">{t('room.papel.anfitriao')}</Tag> : p.can_admit ? <Tag>{t('room.papel.admiteEntradas')}</Tag> : <Tag plain>{t('room.lobby.participante')}</Tag>}</td>
                      <td>
                        <span className="lb-state">
                          <Icon name={p.mic ? 'mic' : 'micOff'} size={13} aria-hidden="true" />
                          <Icon name={p.cam ? 'video' : 'videoOff'} size={13} aria-hidden="true" />
                          <span className="dx-sr-only">
                            {p.mic ? t('room.lobby.micLigado') : t('room.lobby.micDesligado')}, {p.cam ? t('room.lobby.camLigada') : t('room.lobby.camDesligada')}
                          </span>
                        </span>
                      </td>
                      <td className="lb-actions">
                        {!p.host && (
                          <>
                            <Button size="sm" variant="secondary" disabled={!p.mic} onClick={() => send({ type: 'force-mute', to: p.peer_id })}>
                              {t('room.lobby.silenciar')}
                            </Button>
                            <Button size="sm" variant="outline" onClick={() => send({ type: 'kick', to: p.peer_id })}>
                              {t('room.lobby.remover')}
                            </Button>
                          </>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {connected && peers.length === 0 && (
              <Empty icon="people" title={t('room.lobby.ninguemNaSala')}>
                {t('room.lobby.ninguemNaSalaTexto')}
              </Empty>
            )}
          </Card>

          <div className="lb-side">
            <Card
              title={t('room.avisos.salaDeEspera')}
              eyebrow={t('room.lobby.aAguardar', { count: waiting.length })}
              className={waiting.length > 0 ? 'lb-waiting is-busy' : 'lb-waiting'}
            >
              {waiting.length === 0 ? (
                <p className="dx-muted lb-empty">{t('room.lobby.filaVazia')}</p>
              ) : (
                <div className="lb-queue">
                  {waiting.map((w) => (
                    <div key={w.peer_id} className="lb-queue__row">
                      <Avatar name={w.username} size={28} />
                      <span className="lb-queue__who">
                        <strong>{w.username}</strong>
                        <small className="dx-muted">{w.is_pstn ? t('room.papel.telefone') : t('room.avisos.querEntrar')}</small>
                      </span>
                      <Button size="sm" variant="primary" onClick={() => admit(w.peer_id, true)}>
                        {t('room.avisos.admitir')}
                      </Button>
                      <IconButton icon="x" label={t('room.lobby.negarNome', { nome: w.username })} onClick={() => admit(w.peer_id, false)} />
                    </div>
                  ))}
                  {waiting.length > 1 && (
                    <Button block variant="secondary" onClick={() => waiting.forEach((w) => admit(w.peer_id, true))}>
                      {t('room.avisos.admitirTodos', { count: waiting.length })}
                    </Button>
                  )}
                </div>
              )}
            </Card>

            <Card title={t('room.lobby.regras')}>
              <div className="lb-rules">
                <Toggle
                  label={t('room.pessoas.bloquear')}
                  hint={t('room.pessoas.bloquearDica')}
                  checked={locked}
                  disabled={!connected}
                  onChange={(e) => {
                    setLocked(e.target.checked)
                    send({ type: 'room-lock', locked: e.target.checked })
                  }}
                />
                <Toggle
                  label={t('room.pessoas.soAnfitriaoPartilha')}
                  hint={t('room.pessoas.soAnfitriaoPartilhaDica')}
                  checked={hostShare}
                  disabled={!connected}
                  onChange={(e) => {
                    setHostShare(e.target.checked)
                    send({ type: 'host-share-only', on: e.target.checked })
                  }}
                />
              </div>
            </Card>
          </div>
        </div>
      </main>
    </div>
  )
}
