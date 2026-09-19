/**
 * Segunda metade da entrada com MFA activo.
 *
 * Substitui o formulário INTEIRO: a palavra-passe já foi aceite, e deixar o
 * email e o SSO à vista sugeria que se podia recomeçar sem o código — o oposto
 * do que o segundo factor faz. «Recomeçar» é explícito e larga o desafio.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { loginMfa, User } from '../../api'
import { Alert, Button, Field, TextInput } from '../../ui/kit'
import { codigoCompleto, estadoHttp, motivoDaRecusa, normalizarCodigo, TipoCodigo } from './logica'

export default function DesafioMfa({
  mfaToken,
  onLogin,
  onRecomecar,
}: {
  mfaToken: string
  onLogin: (u: User) => void
  onRecomecar: () => void
}) {
  const { t } = useTranslation()
  const [tipo, setTipo] = useState<TipoCodigo>('totp')
  const [code, setCode] = useState('')
  const [erro, setErro] = useState('')
  const [busy, setBusy] = useState(false)

  async function submeter(e: FormEvent) {
    e.preventDefault()
    if (!codigoCompleto(code, tipo)) return
    setErro('')
    setBusy(true)
    try {
      onLogin(await loginMfa(mfaToken, code))
    } catch (err) {
      // Código errado e desafio expirado dão a MESMA mensagem: distinguir os
      // dois diria a quem tenta adivinhar se vale a pena insistir no desafio.
      const status = estadoHttp(err)
      const recusado = status === 401 || status === 400
      setErro(recusado ? t('auth.desafio.invalido') : t(motivoDaRecusa(err) ?? 'auth.erro.generico'))
      setCode('')
    } finally {
      setBusy(false)
    }
  }

  function trocarTipo() {
    setTipo((v) => (v === 'totp' ? 'recuperacao' : 'totp'))
    setCode('')
    setErro('')
  }

  const totp = tipo === 'totp'

  return (
    <form className="auth-form auth-mfa" onSubmit={submeter} data-testid="auth-mfa">
      <header className="auth-form__head">
        <h1>{t('auth.desafio.titulo')}</h1>
        <p className="dx-muted">{totp ? t('auth.desafio.totp') : t('auth.desafio.recuperacao')}</p>
      </header>

      <Field label={t('auth.desafio.codigo')} htmlFor="auth-mfa-code" hint={t('auth.desafio.expira')}>
        <TextInput
          // `key` força um campo novo ao trocar de tipo: o teclado do telemóvel
          // só relê o `inputMode` quando o campo ganha foco de novo.
          key={tipo}
          id="auth-mfa-code"
          name="code"
          className="mfa-code-input"
          large
          code
          autoFocus
          autoComplete="one-time-code"
          inputMode={totp ? 'numeric' : 'text'}
          autoCapitalize="characters"
          spellCheck={false}
          placeholder={totp ? '000000' : 'XXXXX-XXXXX'}
          value={code}
          onChange={(e) => setCode(normalizarCodigo(e.target.value, tipo))}
        />
      </Field>

      {erro && (
        <div className="auth-error">
          <Alert tone="danger">{erro}</Alert>
        </div>
      )}

      <Button type="submit" variant="primary" size="lg" block busy={busy} disabled={!codigoCompleto(code, tipo)} data-testid="auth-mfa-submit">
        {t('auth.desafio.verificar')}
      </Button>

      <div className="auth-mfa__alternativas">
        <button type="button" className="auth-link" onClick={trocarTipo}>
          {totp ? t('auth.desafio.usarRecuperacao') : t('auth.desafio.usarTotp')}
        </button>
        <button type="button" className="auth-link auth-link--muted" onClick={onRecomecar}>
          {t('auth.desafio.recomecar')}
        </button>
      </div>
    </form>
  )
}
