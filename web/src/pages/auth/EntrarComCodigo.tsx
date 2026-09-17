/**
 * Entrar numa sala com o código (ou o link colado). Não há entrada anónima no
 * servidor: o código leva a `#/r/<código>`, e é a própria entrada que depois
 * diz à pessoa que segue para essa sala mal se autentique.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { parseRoomCode } from '../../roomCode'
import { Button, Field, TextInput } from '../../ui/kit'

export default function EntrarComCodigo() {
  const { t } = useTranslation()
  const [raw, setRaw] = useState('')
  const [erro, setErro] = useState('')

  function submeter(e: FormEvent) {
    e.preventDefault()
    const code = parseRoomCode(raw)
    if (!code) {
      setErro(t('auth.sala.invalido'))
      return
    }
    setErro('')
    location.hash = `/r/${code}`
  }

  return (
    <form className="auth-join" onSubmit={submeter} data-testid="auth-join">
      <div className="auth-join__cabeca">
        <h2 className="auth-join__titulo">{t('consola.entrar.codigoTitulo')}</h2>
        <span className="dx-muted">{t('auth.sala.dica')}</span>
      </div>
      <Field label={<span className="dx-sr-only">{t('consola.entrar.codigoTitulo')}</span>} htmlFor="auth-join-code" error={erro || undefined} hint={t('auth.sala.rotulo')}>
        <div className="auth-join__linha">
          <TextInput
            id="auth-join-code"
            name="room"
            code
            large
            spellCheck={false}
            autoCapitalize="none"
            placeholder="abc-defg-hij"
            aria-invalid={erro ? true : undefined}
            value={raw}
            onChange={(e) => setRaw(e.target.value)}
          />
          <Button type="submit" variant="outline">
            {t('auth.sala.entrar')}
          </Button>
        </div>
      </Field>
    </form>
  )
}
