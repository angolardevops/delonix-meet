import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, apiErrorMessage, currentUser, isAbort, joinRoom } from '../api'
import PageBar from '../components/PageBar'
import { BreakoutRoom, ClientMsg, ClientMsgB1, Origin, onB1, PeerInfo, Role, sendB1, Signaling } from '../signaling'
import { Icon } from '../ui/icons'
import { Alert, Avatar, Button, Empty, Spinner, StatusBadge, cx } from '../ui/kit'
import { BreakoutsCard, type BreakoutsApi } from '../room/BreakoutsCard'
import { MeetingElapsed } from '../room/Clocks'
import { papelDe } from '../room/useParticipants'
import '../ui/room.css'
import '../ui/moderation.css'

type Filtro = 'todos' | 'maos' | 'semSom'

/** Uma linha da tabela: quem está na sala, à porta, ou numa sala paralela. */
type Linha =
  | { kind: 'eu'; nome: string; peer: PeerInfo | null }
  | { kind: 'sala'; peer: PeerInfo }
  | { kind: 'espera'; peer: PeerInfo }
  | { kind: 'paralela'; nome: string; sala: BreakoutRoom }

const ORIGEM: Record<Origin, string> = {
  sso: 'room.lobby.origemSso',
  password: 'room.lobby.origemPassword',
  guest: 'room.lobby.origemGuest',
  pstn: 'room.lobby.origemPstn',
  bot: 'room.lobby.origemBot',
}

/**
 * Consola do anfitrião (template DelonixModeration): quem está na reunião, de
 * onde veio e com que papel, onde está, a sala de espera e as salas paralelas.
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
  const [training, setTraining] = useState(false)
  const [meuPeer, setMeuPeer] = useState('')
  const [peers, setPeers] = useState<PeerInfo[]>([])
  const [waiting, setWaiting] = useState<PeerInfo[]>([])
  /** Sala de espera ligada agora (`room-settings.waiting_room`). */
  const [salaDeEspera, setSalaDeEspera] = useState<boolean | null>(null)
  const [destaque, setDestaque] = useState<string | null>(null)
  const [inicio, setInicio] = useState(0)
  const [filtro, setFiltro] = useState<Filtro>('todos')
  /** Quem grava: servidor (`server-recording`) e gravações locais de cada participante (`recording`). */
  const [recServidor, setRecServidor] = useState<string | null>(null)
  const [recLocais, setRecLocais] = useState<Record<string, string>>({})
  const [rooms, setRooms] = useState<BreakoutRoom[]>([])
  const [endsAt, setEndsAt] = useState<number | null>(null)
  const [minutes, setMinutes] = useState(0)
  const [assign, setAssign] = useState<'auto' | 'manual'>('auto')

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
        setTraining(room.format === 'training')
        setSalaDeEspera(room.waiting_room)
        signal = new Signaling(room_token, code)
        signalRef.current = signal
        const s = signal
        onB1(s, 'joined', (m) => {
          setConnected(true)
          setMeuPeer(m.peer_id)
          setPeers(m.peers)
          if (m.started_at && m.started_at > 0) setInicio(m.started_at)
        })
        s.on('peer-joined', (m) => setPeers((p) => [...p.filter((x) => x.peer_id !== m.peer.peer_id), m.peer]))
        s.on('peer-left', (m) => {
          setPeers((p) => p.filter((x) => x.peer_id !== m.peer_id))
          setRecLocais((r) => {
            if (!(m.peer_id in r)) return r
            const n = { ...r }
            delete n[m.peer_id]
            return n
          })
        })
        s.on('waiting-join', (m) => setWaiting((q) => [...q.filter((x) => x.peer_id !== m.peer.peer_id), m.peer]))
        s.on('waiting-left', (m) => setWaiting((q) => q.filter((x) => x.peer_id !== m.peer_id)))
        onB1(s, 'room-settings', (m) => {
          if (m.waiting_room !== undefined) setSalaDeEspera(m.waiting_room)
        })
        s.on('media', (m) => setPeers((p) => p.map((x) => (x.peer_id === m.from ? { ...x, cam: m.cam, mic: m.mic } : x))))
        s.on('hand', (m) => setPeers((p) => p.map((x) => (x.peer_id === m.from ? { ...x, hand: m.raised } : x))))
        onB1(s, 'peer-role', (m) =>
          setPeers((p) => p.map((x) => (x.peer_id === m.peer_id ? { ...x, can_admit: m.can_admit, role: m.role ?? x.role } : x))),
        )
        s.on('host-changed', (m) =>
          setPeers((p) =>
            p.map((x) => (x.peer_id === m.to ? { ...x, host: true, role: 'host' } : x.peer_id === m.from ? { ...x, host: false, role: 'attendee' } : x)),
          ),
        )
        onB1(s, 'spotlight', (m) => setDestaque(m.peer))
        s.on('server-recording', (m) => setRecServidor(m.active ? m.by : null))
        s.on('recording', (m) =>
          setRecLocais((r) => {
            const n = { ...r }
            if (m.active) n[m.from] = m.username
            else delete n[m.from]
            return n
          }),
        )
        s.on('breakouts-created', (m) => {
          setRooms(m.rooms)
          setEndsAt(m.ends_at)
        })
        s.on('error', (m) => setErro(m.message))
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
  const send1 = (msg: ClientMsgB1) => signalRef.current && sendB1(signalRef.current, msg)
  const me = currentUser()
  const admit = (peerId: string, ok: boolean) => {
    send({ type: ok ? 'admit' : 'deny', to: peerId })
    setWaiting((q) => q.filter((x) => x.peer_id !== peerId))
  }
  const admitirTodos = () => {
    send1({ type: 'admit-all' })
    setWaiting([])
  }

  // A minha sessão NA SALA (o anfitrião também lá está) é a mesma pessoa que
  // esta consola: uma linha só, não duas com o mesmo nome.
  const minhaNaSala = peers.find((p) => p.host && p.username === me?.username) ?? null
  const outros = useMemo(() => peers.filter((p) => p !== minhaNaSala), [peers, minhaNaSala])
  const naSala = outros.length + 1
  const maos = peers.filter((p) => p.hand).length
  const semSom = peers.filter((p) => !p.mic).length
  const gravacao = recServidor
    ? t('room.gravacao.noServidorPor', { nome: recServidor })
    : Object.values(recLocais)[0]
      ? t('room.gravacao.aGravarPor', { nome: Object.values(recLocais)[0] })
      : null

  const breakouts: BreakoutsApi = {
    rooms,
    endsAt,
    minutes,
    setMinutes,
    assign,
    setAssign,
    create: (count) => send1({ type: 'breakouts-create', count, minutes: minutes || null, assign }),
    rename: (roomCode, label) => send({ type: 'breakout-rename', code: roomCode, label }),
    add: () => send({ type: 'breakout-add' }),
    moveUser: (name, roomCode) => send({ type: 'breakout-move-user', name, code: roomCode }),
    closeAll: () => send({ type: 'breakouts-close' }),
    broadcast: (text) => send1({ type: 'breakouts-broadcast', text }),
    // Visitar um grupo como na sala: o caminho de volta fica guardado.
    visit: (roomCode) => {
      sessionStorage.setItem(`dx_return_${roomCode}`, code)
      location.hash = `/r/${roomCode}`
    },
  }

  const linhas = useMemo<Linha[]>(() => {
    const out: Linha[] = []
    if (filtro === 'todos' && connected) out.push({ kind: 'eu', nome: me?.username ?? '', peer: minhaNaSala })
    for (const p of outros) {
      if (filtro === 'maos' && !p.hand) continue
      if (filtro === 'semSom' && p.mic) continue
      out.push({ kind: 'sala', peer: p })
    }
    if (filtro === 'todos') {
      for (const r of rooms) for (const nome of r.people) out.push({ kind: 'paralela', nome, sala: r })
      for (const w of waiting) out.push({ kind: 'espera', peer: w })
    }
    return out
  }, [filtro, connected, me?.username, minhaNaSala, outros, rooms, waiting])

  const origem = (p: PeerInfo | null) => {
    const o = p?.origin ? t(ORIGEM[p.origin]) : p?.is_pstn ? t(ORIGEM.pstn) : p?.is_bot ? t(ORIGEM.bot) : ''
    return p?.title ? (o ? t('room.lobby.origemCargo', { origem: o, cargo: p.title }) : p.title) : o
  }

  const papel = (role: Role) => {
    const [classe, rotulo] =
      role === 'host'
        ? ['is-host', t('room.papel.anfitriao')]
        : role === 'cohost'
          ? ['is-cohost', t('room.papel.coAnfitriao')]
          : role === 'speaker'
            ? ['is-speaker', t('room.papel.orador')]
            : role === 'broadcast'
              ? ['is-speaker', t('room.papel.emissao')]
              : ['', t('room.lobby.assistente')]
    return <span className={cx('lb-role', classe)}>{rotulo}</span>
  }

  /** «principal», ou — com salas abertas — a sala de cada pessoa (atribuição manual). */
  const celulaSala = (nome: string, atual: string | null) =>
    rooms.length === 0 ? (
      <span className="dx-num">{t('room.lobby.principal')}</span>
    ) : (
      <select
        className="lb-roomsel dx-num"
        value={atual ?? code}
        aria-label={t('room.lobby.moverPara', { nome })}
        onChange={(e) => breakouts.moveUser(nome, e.target.value)}
      >
        <option value={code}>{t('room.lobby.principal')}</option>
        {rooms.map((o) => (
          <option key={o.code} value={o.code}>
            {o.label}
          </option>
        ))}
      </select>
    )

  /** A acção de papel da linha: Fixar (anfitrião), Promover, Passar palco ou Retirar palco. */
  const accaoPapel = (p: PeerInfo) => {
    const role = papelDe(p)
    if (role === 'host') {
      const fixado = destaque === p.peer_id
      return (
        <button
          type="button"
          className={cx('lb-act', fixado && 'is-on')}
          aria-pressed={fixado}
          title={t('room.lobby.fixarDica')}
          onClick={() => send1({ type: 'spotlight', peer: fixado ? null : p.peer_id })}
        >
          {fixado ? t('room.lobby.desafixar') : t('room.lobby.fixar')}
        </button>
      )
    }
    const [alvo, rotulo, dica]: [Exclude<Role, 'host'>, string, string] =
      role === 'attendee'
        ? ['cohost', t('room.lobby.promover'), t('room.lobby.promoverDica')]
        : role === 'cohost'
          ? ['speaker', t('room.lobby.passarPalco'), t('room.lobby.passarPalcoDica')]
          : ['attendee', t('room.lobby.retirarPalco'), t('room.lobby.retirarPalcoDica')]
    return (
      <button type="button" className="lb-act" title={dica} onClick={() => send1({ type: 'set-role', to: p.peer_id, role: alvo })}>
        {rotulo}
      </button>
    )
  }

  const pessoa = (nome: string, sub: string, hand?: boolean) => (
    <span className="lb-person">
      <Avatar name={nome} size={27} />
      <span>
        <strong>{nome}</strong>
        {sub && <small className="dx-num dx-muted">{sub}</small>}
      </span>
      {hand && (
        <span className="lb-hand">
          <Icon name="hand" size={10} />
          {t('room.tile.mao')}
        </span>
      )}
    </span>
  )

  return (
    <div className="lb-page">
      <PageBar
        title={roomName ? t('room.lobby.tituloSala', { nome: roomName }) : t('room.lobby.titulo')}
        meta={
          <span className="lb-meta">
            {inicio > 0 && <MeetingElapsed startedAt={inicio} className="lb-elapsed dx-num" />}
            {gravacao && (
              <span title={gravacao} role="status">
                <StatusBadge tone="record">{t('room.topo.rec')}</StatusBadge>
                <span className="dx-sr-only">{gravacao}</span>
              </span>
            )}
          </span>
        }
      >
        <Button variant="outline" aria-label={t('room.lobby.silenciarTodos')} disabled={!connected || peers.length === 0} onClick={() => send({ type: 'mute-all', allow_unmute: true })}>
          <span className="lb-hide-narrow">{t('room.lobby.silenciarTodos')}</span>
          <Icon name="micOff" className="dx-icon lb-only-narrow" />
        </Button>
        <Button variant="primary" onClick={() => (location.hash = `/r/${code}`)}>
          {t('room.lobby.voltarSessao')}
        </Button>
      </PageBar>

      <main className="lb-body">
        {erro && <Alert tone="danger">{erro}</Alert>}
        {!connected && !erro && (
          <p className="lb-connecting dx-muted" role="status">
            <Spinner /> {t('room.lobby.aLigar')}
          </p>
        )}

        <div className="lb-grid">
          <section className="lb-main" aria-labelledby="lb-part-h">
            <div className="lb-main__head">
              <h2 id="lb-part-h">{t('room.lobby.participantes')}</h2>
              <span className="dx-num dx-muted">{t('room.lobby.resumo', { naSala: connected ? naSala : 0, espera: waiting.length })}</span>
              <span className="dx-spacer" />
              <div className="lb-filters" role="group" aria-label={t('room.lobby.filtro')}>
                {(
                  [
                    ['todos', t('room.lobby.todos')],
                    ['maos', t('room.lobby.maosNoAr', { n: maos })],
                    ['semSom', t('room.lobby.semSom', { n: semSom })],
                  ] as const
                ).map(([v, label]) => (
                  <button key={v} type="button" className={cx('lb-filter', filtro === v && 'is-on')} aria-pressed={filtro === v} onClick={() => setFiltro(v)}>
                    {label}
                  </button>
                ))}
              </div>
            </div>

            <div className="lb-tablecard">
              <div className="dx-table-wrap">
                <table className="lb-table">
                  <thead>
                    <tr>
                      <th scope="col">{t('room.lobby.pessoa')}</th>
                      <th scope="col">{t('room.lobby.papel')}</th>
                      <th scope="col">{t('room.lobby.sala')}</th>
                      <th scope="col">{t('room.lobby.accoes')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {linhas.map((l) => {
                      if (l.kind === 'eu') {
                        const eu = l.peer
                        return (
                          <tr key="eu">
                            <td>{pessoa(l.nome, eu ? origem(eu) : t('room.lobby.naConsola'), eu?.hand)}</td>
                            <td>{papel('host')}</td>
                            <td className="dx-num dx-muted">{t('room.lobby.principal')}</td>
                            <td>
                              <div className="lb-actions">
                                {eu && (
                                  <button type="button" className="lb-act" disabled={!eu.mic} onClick={() => send({ type: 'force-mute', to: eu.peer_id })}>
                                    {t('room.lobby.silenciar')}
                                  </button>
                                )}
                                {eu
                                  ? accaoPapel(eu)
                                  : meuPeer && accaoPapel({ peer_id: meuPeer, username: l.nome, host: true, hand: false, cam: false, mic: false })}
                              </div>
                            </td>
                          </tr>
                        )
                      }
                      if (l.kind === 'paralela')
                        return (
                          <tr key={`bo-${l.sala.code}-${l.nome}`}>
                            <td>{pessoa(l.nome, '')}</td>
                            <td>{papel('attendee')}</td>
                            <td className="dx-muted">{celulaSala(l.nome, l.sala.code)}</td>
                            <td />
                          </tr>
                        )
                      const p = l.peer
                      if (l.kind === 'espera')
                        return (
                          <tr key={`w-${p.peer_id}`}>
                            <td>{pessoa(p.username, origem(p))}</td>
                            <td>{papel(papelDe(p))}</td>
                            <td className="dx-num lb-waiting-cell">{t('room.lobby.emEspera')}</td>
                            <td>
                              <div className="lb-actions">
                                <button type="button" className="lb-act" onClick={() => admit(p.peer_id, true)}>
                                  {t('room.avisos.admitir')}
                                </button>
                                <button type="button" className="lb-act is-danger" onClick={() => admit(p.peer_id, false)}>
                                  {t('room.avisos.negar')}
                                </button>
                              </div>
                            </td>
                          </tr>
                        )
                      return (
                        <tr key={p.peer_id}>
                          <td>{pessoa(p.username, origem(p), p.hand)}</td>
                          <td>{papel(papelDe(p))}</td>
                          <td className="dx-muted">{celulaSala(p.username, null)}</td>
                          <td>
                            <div className="lb-actions">
                              <button type="button" className="lb-act" disabled={!p.mic} onClick={() => send({ type: 'force-mute', to: p.peer_id })}>
                                {t('room.lobby.silenciar')}
                              </button>
                              {accaoPapel(p)}
                              {!p.host && (
                                <button type="button" className="lb-act is-danger" onClick={() => send({ type: 'kick', to: p.peer_id })}>
                                  {t('room.lobby.remover')}
                                </button>
                              )}
                            </div>
                          </td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              </div>
              {connected && linhas.length <= 1 && filtro === 'todos' && (
                <Empty icon="people" title={t('room.lobby.ninguemNaSala')}>
                  {t('room.lobby.ninguemNaSalaTexto')}
                </Empty>
              )}
            </div>
          </section>

          <div className="lb-side">
            <section className={cx('lb-card lb-waitcard', waiting.length > 0 && 'is-busy')} aria-labelledby="lb-wait-h">
              <div className="lb-card__head">
                <h2 id="lb-wait-h">{t('room.avisos.salaDeEspera')}</h2>
                <span className="dx-num">{t('room.lobby.aAguardarAprovacao', { count: waiting.length })}</span>
              </div>
              {waiting.length === 0 ? (
                <p className="dx-muted lb-empty">{salaDeEspera === false ? t('room.lobby.esperaDesligada') : t('room.lobby.filaVazia')}</p>
              ) : (
                waiting.map((w) => (
                  <div key={w.peer_id} className="lb-queue__row">
                    {w.origin === 'pstn' || w.is_pstn ? (
                      <span className="lb-queue__phone" aria-hidden="true">
                        <Icon name="phone" size={12} />
                      </span>
                    ) : (
                      <Avatar name={w.username} size={27} />
                    )}
                    <span className="lb-queue__who">
                      <strong>{w.username}</strong>
                      <small className="dx-num dx-muted">{origem(w) || t('room.avisos.querEntrar')}</small>
                    </span>
                    <Button size="sm" variant="primary" onClick={() => admit(w.peer_id, true)}>
                      {t('room.avisos.admitir')}
                    </Button>
                    <Button size="sm" variant="outline" aria-label={t('room.lobby.negarNome', { nome: w.username })} onClick={() => admit(w.peer_id, false)}>
                      {t('room.avisos.negar')}
                    </Button>
                  </div>
                ))
              )}
              <div className="lb-waitcard__actions">
                <button type="button" className="lb-wide" disabled={!connected || waiting.length === 0} onClick={admitirTodos}>
                  {t('room.lobby.admitirTodos')}
                </button>
                <button
                  type="button"
                  className="lb-wide is-muted"
                  disabled={!connected || salaDeEspera === null}
                  onClick={() => send1({ type: 'waiting-room', on: !salaDeEspera })}
                >
                  {salaDeEspera === false ? t('room.lobby.abrirEspera') : t('room.lobby.fecharEspera')}
                </button>
              </div>
            </section>

            {training && <BreakoutsCard code={code} api={breakouts} className="lb-card lb-bocard" />}
          </div>
        </div>
      </main>
    </div>
  )
}
