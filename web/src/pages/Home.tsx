import { FormEvent, useState } from 'react'
import { createRoom, getRoom, User } from '../api'
import { VideoIcon } from '../icons'

// Ícone de vídeo local (evita conflito de nomes com CamIcon).
function VideoBadge() {
  return (
    <svg width={26} height={26} viewBox="0 0 24 24" fill="currentColor">
      <path d="M4 6h11a1 1 0 0 1 1 1v3.5l4-3.5v10l-4-3.5V17a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1Z" />
    </svg>
  )
}

export default function Home({
  user,
  onEnterRoom,
}: {
  user: User
  onEnterRoom: (code: string, voice?: boolean) => void
}) {
  const [joinCode, setJoinCode] = useState('')
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState('')

  async function newMeeting(waitingRoom = false, e2ee = false) {
    setError('')
    setCreating(true)
    try {
      const room = await createRoom(`Reunião de ${user.username}`, 'sfu', waitingRoom, e2ee)
      onEnterRoom(room.code)
    } catch (err) {
      setError((err as Error).message)
      setCreating(false)
    }
  }

  async function join(e: FormEvent) {
    e.preventDefault()
    setError('')
    const code = joinCode.trim().toLowerCase().replace(/^.*\/r\//, '')
    try {
      const room = await getRoom(code)
      onEnterRoom(room.code)
    } catch {
      setError('Sala não encontrada — verifica o código')
    }
  }

  return (
    <div className="home">
      <div className="home-hero">
        <div className="home-copy">
          <h1>
            Videochamadas e reuniões <span>para toda a equipa</span>
          </h1>
          <p className="home-sub">
            Reúne, colabora e grava a partir de qualquer lugar com o Delonix Meet — encriptado,
            com fundos inteligentes e gravações partilháveis.
          </p>

          <div className="home-actions">
            <button className="btn-new" disabled={creating} onClick={() => void newMeeting(false)}>
              <VideoBadge />
              {creating ? 'A criar…' : 'Nova reunião'}
            </button>

            <form className="join-box" onSubmit={join}>
              <span className="join-icon">⌨</span>
              <input
                placeholder="Introduz um código ou link"
                value={joinCode}
                onChange={(e) => setJoinCode(e.target.value)}
              />
              <button className="join-btn" disabled={!joinCode.trim()}>
                Participar
              </button>
            </form>
          </div>

          <div className="home-extra">
            <button className="link" onClick={() => void newMeeting(true)}>
              Criar com sala de espera (admitir convidados manualmente)
            </button>
            <button className="link" onClick={() => void newMeeting(false, true)}>
              🔒 Criar reunião E2EE (encriptada de ponta a ponta, com frase-chave)
            </button>
          </div>
          {error && <div className="error">{error}</div>}
        </div>

        <div className="home-illus" aria-hidden>
          <div className="illus-ring">
            <div className="illus-card c1">
              <span className="avatar-circle">AL</span>
            </div>
            <div className="illus-card c2">
              <VideoIcon />
            </div>
            <div className="illus-card c3">
              <span className="avatar-circle">BO</span>
            </div>
            <div className="illus-center">
              <span className="brand-mark big">◆</span>
            </div>
          </div>
          <p className="illus-caption">Uma ligação segura, um código para partilhar.</p>
        </div>
      </div>
    </div>
  )
}
