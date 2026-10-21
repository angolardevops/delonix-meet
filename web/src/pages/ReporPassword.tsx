/**
 * Escolher a palavra-passe nova a partir do link do email (E3) —
 * `#/repor-password?token=…`. PÚBLICA: quem a usa está fora da conta.
 *
 * Serve também o token que um administrador entrega à mão (o mesmo `accept`):
 * a página não sabe, nem precisa de saber, de que canal veio o link.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, acceptPasswordReset, apiErrorCode } from '../api'
import { Alert, Button, Card, Field } from '../ui/kit'
import CampoPalavraPasse from './auth/CampoPalavraPasse'
import Moldura from './publico/Moldura'

function chaveDoErro(e: unknown): string {
  switch (apiErrorCode(e)) {
    case 'password_reset.expired':
      return 'auth.reposicao.erroExpirado'
    case 'password_reset.not_found':
      return 'auth.reposicao.erroInvalido'
    case 'account.managed_by_odoo':
      return 'auth.reposicao.erroOdoo'
    default:
      if (e instanceof ApiError && e.status === 400) return 'auth.reposicao.erroPolitica'
      if (e instanceof ApiError && e.status === 429) return 'auth.reposicao.erroMuitos'
      return 'ui.erroGenerico'
  }
}

export default function ReporPassword({ token }: { token: string }) {
  const { t } = useTranslation()
  const [password, setPassword] = useState('')
  const [confirmar, setConfirmar] = useState('')
  const [busy, setBusy] = useState(false)
  const [feito, setFeito] = useState(false)
  const [erro, setErro] = useState('')

  async function submeter(e: FormEvent) {
    e.preventDefault()
    if (password !== confirmar) {
      setErro(t('auth.reposicao.naoCoincidem'))
      return
    }
    setBusy(true)
    setErro('')
    try {
      await acceptPasswordReset(token, password)
      setFeito(true)
    } catch (err) {
      setErro(t(chaveDoErro(err)))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Moldura pagina="repor-password" estreita>
      <Card className="pub-cartao">
        <div className="org-form">
          <h1>{t('auth.reposicao.novaTitulo')}</h1>
          {feito ? (
            <>
              <Alert tone="success">{t('auth.reposicao.feito')}</Alert>
              <Button variant="primary" size="lg" block onClick={() => (location.hash = '/login')}>
                {t('auth.reposicao.entrar')}
              </Button>
            </>
          ) : (
            <form onSubmit={submeter} className="org-form">
              <Field label={t('auth.reposicao.nova')} htmlFor="repor-nova" hint={t('auth.registo.dicaPalavraPasse')}>
                <CampoPalavraPasse
                  id="repor-nova"
                  autoComplete="new-password"
                  required
                  minLength={8}
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                />
              </Field>
              <Field label={t('auth.reposicao.confirmar')} htmlFor="repor-confirmar">
                <CampoPalavraPasse
                  id="repor-confirmar"
                  autoComplete="new-password"
                  required
                  value={confirmar}
                  onChange={(e) => setConfirmar(e.target.value)}
                />
              </Field>
              {erro && <Alert tone="danger">{erro}</Alert>}
              <Button type="submit" variant="primary" size="lg" block busy={busy}>
                {t('auth.reposicao.guardar')}
              </Button>
            </form>
          )}
        </div>
      </Card>
    </Moldura>
  )
}
