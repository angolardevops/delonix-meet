import { FormEvent, useState } from 'react'
import { createRoom, getRoom, User } from '../api'

export default function Lobby({
  user,
  onEnterRoom,
  onLogout,
}: {
  user: User
  onEnterRoom: (code: string) => void
  onLogout: () => void
}) {
  const [roomName, setRoomName] = useState('')
  const [topology, setTopology] = useState<'sfu' | 'mesh'>('sfu')
  const [waitingRoom, setWaitingRoom] = useState(false)
  const [joinCode, setJoinCode] = useState('')
  const [error, setError] = useState('')

  async function create(e: FormEvent) {
    e.preventDefault()
    setError('')
    try {
      const room = await createRoom(roomName || `Reunião de ${user.username}`, topology, waitingRoom)
      onEnterRoom(room.code)
    } catch (err) {
      setError((err as Error).message)
    }
  }

  async function join(e: FormEvent) {
    e.preventDefault()
    setError('')
    try {
      const room = await getRoom(joinCode.trim().toLowerCase())
      onEnterRoom(room.code)
    } catch {
      setError('Sala não encontrada — verifica o código')
    }
  }

  return (
    <div className="lobby-page">
      <header className="topbar">
        <h1 className="logo">
          Delonix <span>Meet</span>
        </h1>
        <div className="topbar-right">
          <span className="me">{user.username}</span>
          <button className="link" onClick={onLogout}>
            Sair
          </button>
        </div>
      </header>
      <main className="lobby-main">
        <h2>Videochamadas seguras para todos</h2>
        <p className="muted">Cria uma sala e partilha o código, ou entra numa sala existente.</p>
        <div className="lobby-actions">
          <form onSubmit={create} className="lobby-card">
            <h3>Nova reunião</h3>
            <input
              placeholder="Nome da reunião (opcional)"
              value={roomName}
              onChange={(e) => setRoomName(e.target.value)}
            />
            <label className="topo-select">
              Topologia
              <select value={topology} onChange={(e) => setTopology(e.target.value as 'sfu' | 'mesh')}>
                <option value="sfu">SFU — escala para salas grandes (recomendado)</option>
                <option value="mesh">Mesh — P2P direto, salas até ~6</option>
              </select>
            </label>
            <label className="check-row">
              <input
                type="checkbox"
                checked={waitingRoom}
                onChange={(e) => setWaitingRoom(e.target.checked)}
              />
              Sala de espera — os convidados só entram quando o anfitrião os admitir
            </label>
            <button className="primary">Criar sala</button>
          </form>
          <form onSubmit={join} className="lobby-card">
            <h3>Entrar com código</h3>
            <input
              placeholder="ex.: abc-defg-hij"
              value={joinCode}
              onChange={(e) => setJoinCode(e.target.value)}
              required
            />
            <button className="primary">Entrar</button>
          </form>
        </div>
        {error && <div className="error">{error}</div>}
      </main>
    </div>
  )
}
