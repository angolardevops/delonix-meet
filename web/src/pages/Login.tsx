import { FormEvent, useState } from 'react'
import { login, register, User } from '../api'

export default function Login({ onLogin }: { onLogin: (u: User) => void }) {
  const [mode, setMode] = useState<'login' | 'register'>('login')
  const [email, setEmail] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setError('')
    setBusy(true)
    try {
      const user =
        mode === 'login' ? await login(email, password) : await register(email, username, password)
      onLogin(user)
    } catch (err) {
      setError((err as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="auth-page">
      <div className="auth-card">
        <h1 className="logo">
          Delonix <span>Meet</span>
        </h1>
        <p className="tagline">Videochamadas seguras e encriptadas</p>
        <form onSubmit={submit}>
          {mode === 'register' && (
            <input
              placeholder="Nome de utilizador"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
              minLength={2}
            />
          )}
          <input
            type="email"
            placeholder="Email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
          />
          <input
            type="password"
            placeholder="Password (mín. 8 caracteres)"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            minLength={8}
          />
          {error && <div className="error">{error}</div>}
          <button className="primary" disabled={busy}>
            {busy ? '…' : mode === 'login' ? 'Entrar' : 'Criar conta'}
          </button>
        </form>
        <button className="link" onClick={() => setMode(mode === 'login' ? 'register' : 'login')}>
          {mode === 'login' ? 'Não tens conta? Regista-te' : 'Já tens conta? Entra'}
        </button>
      </div>
    </div>
  )
}
