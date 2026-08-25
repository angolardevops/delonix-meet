import { useEffect, useState } from 'react'
import { mfaActivar, mfaDesactivar, mfaEstado, mfaInscrever, MfaEstado } from '../api'

/**
 * Segundo factor (TOTP) nas definições da conta.
 *
 * A inscrição é um caminho de três passos com uma propriedade que a interface
 * tem de respeitar: **o segredo e os códigos de recuperação são mostrados UMA
 * vez**. Depois disso só existe o hash deles no servidor. Por isso não há
 * «voltar atrás» a meio nem se fecha o painel sem um aviso — perder os códigos
 * de recuperação significa perder a conta se o telemóvel se perder.
 */
type Passo = 'estado' | 'inscricao' | 'codigos'

export default function MfaPanel() {
  const [estado, setEstado] = useState<MfaEstado | null>(null)
  const [passo, setPasso] = useState<Passo>('estado')
  const [segredo, setSegredo] = useState('')
  const [qr, setQr] = useState('')
  const [codigo, setCodigo] = useState('')
  const [recuperacao, setRecuperacao] = useState<string[]>([])
  const [guardados, setGuardados] = useState(false)
  const [erro, setErro] = useState('')
  const [ocupado, setOcupado] = useState(false)

  const recarregar = () => mfaEstado().then(setEstado).catch(() => {})
  useEffect(() => { void recarregar() }, [])

  async function inscrever() {
    setErro(''); setOcupado(true)
    try {
      const r = await mfaInscrever()
      setSegredo(r.secret)
      // O gerador de QR entra por import dinâmico: só é descarregado por quem
      // abre mesmo a inscrição, e não pesa no bundle de quem nunca lá vai.
      const { toString } = await import('qrcode')
      setQr(await toString(r.otpauth_uri, { type: 'svg', margin: 1, width: 220 }))
      setPasso('inscricao')
    } catch (e) {
      setErro((e as Error).message)
    } finally { setOcupado(false) }
  }

  async function activar() {
    setErro(''); setOcupado(true)
    try {
      const r = await mfaActivar(codigo.trim())
      setRecuperacao(r.backup_codes)
      setCodigo('')
      setPasso('codigos')
      await recarregar()
    } catch {
      setErro('Código inválido. Confirma a hora do telemóvel e tenta o código seguinte.')
    } finally { setOcupado(false) }
  }

  async function desactivar() {
    setErro(''); setOcupado(true)
    try {
      await mfaDesactivar(codigo.trim())
      setCodigo('')
      await recarregar()
    } catch {
      setErro('Código inválido. Usa o código do autenticador ou um de recuperação.')
    } finally { setOcupado(false) }
  }

  if (!estado) return <p className="muted">A carregar…</p>

  // --- Passo 3: os códigos de recuperação, vistos uma única vez ---
  if (passo === 'codigos') {
    return (
      <div className="mfa-panel">
        <h3>Guarda os códigos de recuperação</h3>
        <p className="mfa-warn" role="alert">
          Só os vês <strong>agora</strong>. Cada um serve <strong>uma vez</strong> e é a única
          forma de entrar se perderes o telemóvel.
        </p>
        <ul className="mfa-codes">
          {recuperacao.map((c) => <li key={c}><code>{c}</code></li>)}
        </ul>
        <div className="mfa-actions">
          <button
            className="btn-sm"
            onClick={() => void navigator.clipboard?.writeText(recuperacao.join('\n'))}
          >
            Copiar
          </button>
          <button
            className="btn-sm"
            onClick={() => {
              const blob = new Blob([recuperacao.join('\n')], { type: 'text/plain' })
              const a = document.createElement('a')
              a.href = URL.createObjectURL(blob)
              a.download = 'delonix-codigos-recuperacao.txt'
              a.click()
              URL.revokeObjectURL(a.href)
            }}
          >
            Descarregar
          </button>
        </div>
        <label className="mfa-confirm">
          <input type="checkbox" checked={guardados} onChange={(e) => setGuardados(e.target.checked)} />
          Guardei os códigos num sítio seguro.
        </label>
        <button className="btn-sm primary" disabled={!guardados} onClick={() => { setRecuperacao([]); setPasso('estado') }}>
          Concluir
        </button>
      </div>
    )
  }

  // --- Passo 2: ler o QR e provar que o autenticador funciona ---
  if (passo === 'inscricao') {
    return (
      <div className="mfa-panel">
        <h3>Liga o teu autenticador</h3>
        <p className="muted">
          Lê o código com o Google Authenticator, Aegis, 1Password ou outro — ou introduz a chave à mão.
        </p>
        {qr && <div className="mfa-qr" aria-label="Código QR de inscrição" dangerouslySetInnerHTML={{ __html: qr }} />}
        <label className="set-label">
          Chave (se não conseguires ler o código)
          <code className="mfa-secret">{segredo.match(/.{1,4}/g)?.join(' ')}</code>
        </label>
        <label className="set-label">
          Código de 6 dígitos do autenticador
          <input
            value={codigo}
            onChange={(e) => setCodigo(e.target.value.replace(/\D/g, '').slice(0, 6))}
            placeholder="000000"
            inputMode="numeric"
            autoComplete="one-time-code"
          />
        </label>
        {erro && <p className="auth-error" role="alert">{erro}</p>}
        <div className="mfa-actions">
          <button className="btn-sm primary" disabled={ocupado || codigo.length !== 6} onClick={() => void activar()}>
            {ocupado ? 'A verificar…' : 'Activar'}
          </button>
          <button className="btn-ghost small" onClick={() => { setPasso('estado'); setCodigo(''); setErro('') }}>
            Cancelar
          </button>
        </div>
      </div>
    )
  }

  // --- Passo 1: estado ---
  return (
    <div className="mfa-panel">
      <h3>Verificação em dois passos</h3>
      {estado.enabled ? (
        <>
          <p className="mfa-on">✓ Activa. O teu autenticador é pedido em cada início de sessão.</p>
          <p className="muted">
            Restam <strong>{estado.backup_codes_left}</strong> códigos de recuperação.
            {estado.backup_codes_left <= 2 && ' Desactiva e volta a activar para gerar códigos novos.'}
          </p>
          <label className="set-label">
            Para desactivar, introduz um código actual
            <input
              value={codigo}
              onChange={(e) => setCodigo(e.target.value.replace(/[^0-9A-Za-z-]/g, '').slice(0, 11))}
              placeholder="000000"
              inputMode="numeric"
              autoComplete="one-time-code"
            />
          </label>
          {erro && <p className="auth-error" role="alert">{erro}</p>}
          {/* Exige-se um código porque, sem ele, uma sessão roubada bastava
              para desligar o segundo factor — que existe precisamente para o
              caso de a sessão estar comprometida. */}
          <button className="btn-sm" disabled={ocupado || codigo.trim().length < 6} onClick={() => void desactivar()}>
            Desactivar
          </button>
        </>
      ) : (
        <>
          <p className="muted">
            Acrescenta um código do telemóvel ao teu início de sessão. Uma password roubada deixa
            de chegar para entrar na tua conta.
          </p>
          {estado.pending && (
            <p className="muted">Há uma inscrição por concluir — recomeça para gerar uma chave nova.</p>
          )}
          {erro && <p className="auth-error" role="alert">{erro}</p>}
          <button className="btn-sm primary" disabled={ocupado} onClick={() => void inscrever()}>
            {ocupado ? 'A preparar…' : estado.pending ? 'Recomeçar inscrição' : 'Activar'}
          </button>
        </>
      )}
    </div>
  )
}
