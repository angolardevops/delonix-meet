/**
 * «Esqueci-me da palavra-passe» (E3). Pede o email de reposição e diz SEMPRE a
 * mesma frase, exista a conta ou não — é o servidor que decide se sai um email
 * (só para contas com o endereço provado), e a página não o pode saber nem o
 * deve adivinhar: dizer «não há conta com este email» era dizer quem tem conta.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, apiErrorCode, requestPasswordRecovery } from '../../api'
import { Alert, Button, Field, TextInput } from '../../ui/kit'

function chaveDoErro(e: unknown): string {
  const codigo = apiErrorCode(e)
  if (codigo === 'mail.disabled' || codigo === 'mail.public_url_missing') return 'auth.reposicao.semCorreio'
  if (e instanceof ApiError && e.status === 429) return 'auth.reposicao.erroMuitos'
  return 'ui.erroGenerico'
}

export default function PedirReposicao({ onVoltar }: { onVoltar: () => void }) {
  const { t } = useTranslation()
  const [email, setEmail] = useState('')
  const [busy, setBusy] = useState(false)
  const [enviado, setEnviado] = useState(false)
  const [erro, setErro] = useState('')

  async function submeter(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErro('')
    try {
      await requestPasswordRecovery(email)
      setEnviado(true)
    } catch (err) {
      setErro(t(chaveDoErro(err)))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="auth-form" data-testid="auth-reposicao">
      <header className="auth-form__head">
        <h1>{t('auth.reposicao.titulo')}</h1>
      </header>
      {enviado ? (
        <Alert tone="success">{t('auth.reposicao.enviado')}</Alert>
      ) : (
        <form className="auth-form__campos" onSubmit={submeter}>
          <p className="dx-muted">{t('auth.reposicao.explica')}</p>
          <Field label={t('auth.entrar.email')} htmlFor="auth-reposicao-email">
            <TextInput
              id="auth-reposicao-email"
              type="email"
              autoComplete="email"
              required
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </Field>
          {erro && <Alert tone="danger">{erro}</Alert>}
          <Button type="submit" variant="secondary" size="lg" block busy={busy}>
            {t('auth.reposicao.enviar')}
          </Button>
        </form>
      )}
      <p className="auth-form__troca">
        <button type="button" className="auth-link" onClick={onVoltar}>
          {t('auth.reposicao.voltar')}
        </button>
      </p>
    </div>
  )
}
