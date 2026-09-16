/**
 * Entrar ou criar organização.
 *
 * O SSO decide-se pelo DOMÍNIO do email e decide-o o servidor
 * (`/api/auth/sso/check`): o botão só aparece quando o domínio tem um
 * fornecedor de identidade, e com `enforce_sso` a palavra-passe sai do ecrã —
 * o servidor recusa-a na mesma (`auth.rs` `login`), e mostrar um campo que não
 * serve era convidar a pessoa a falhar.
 */
import { FormEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, getPlatformSettings, login, registerOrg, ssoCheck, ssoRedirect, User } from '../../api'
import { getAppName } from '../../branding'
import { Alert, Button, Field, TextInput } from '../../ui/kit'
import CampoPalavraPasse from './CampoPalavraPasse'
import { dominioDoEmail, motivoDaRecusa } from './logica'

type Modo = 'entrar' | 'registo'

interface EstadoSso {
  dominio: string | null
  activo: boolean
  obrigatorio: boolean
  aVerificar: boolean
}

const SEM_SSO: EstadoSso = { dominio: null, activo: false, obrigatorio: false, aVerificar: false }

export default function FormularioEntrada({
  onLogin,
  onDesafio,
}: {
  onLogin: (u: User) => void
  onDesafio: (mfaToken: string) => void
}) {
  const { t } = useTranslation()
  const [modo, setModo] = useState<Modo>('entrar')
  const [orgName, setOrgName] = useState('')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [erro, setErro] = useState('')
  const [busy, setBusy] = useState(false)
  const [sso, setSso] = useState<EstadoSso>(SEM_SSO)
  // Flags da plataforma (`/api/public/settings`): uma integração pode esconder
  // a criação de organização e o botão de SSO.
  const [esconderRegisto, setEsconderRegisto] = useState(false)
  const [esconderSso, setEsconderSso] = useState(false)

  useEffect(() => {
    let vivo = true
    getPlatformSettings()
      .then((s) => {
        if (!vivo) return
        setEsconderRegisto(!!s.hide_org_creation)
        setEsconderSso(!!s.hide_sso_button)
        if (s.hide_org_creation) setModo('entrar')
      })
      .catch(() => {
        // Sem as flags públicas mostra-se tudo: são para esconder, não para
        // autorizar — o servidor recusa o que não deixar fazer.
      })
    return () => {
      vivo = false
    }
  }, [])

  // Pergunta de SSO 500 ms depois de o email parar de mudar. A resposta de um
  // domínio antigo é descartada: escrever depressa não pode deixar no ecrã o
  // SSO de um domínio que já não está no campo.
  const dominio = modo === 'entrar' ? dominioDoEmail(email) : null
  useEffect(() => {
    if (!dominio) {
      setSso(SEM_SSO)
      return
    }
    let vivo = true
    setSso((s) => (s.dominio === dominio ? s : { ...SEM_SSO, dominio, aVerificar: true }))
    const timer = setTimeout(() => {
      ssoCheck(dominio)
        .then((r) => {
          if (vivo) setSso({ dominio, activo: r.sso_enabled, obrigatorio: r.enforce_sso, aVerificar: false })
        })
        .catch(() => {
          if (vivo) setSso({ ...SEM_SSO, dominio })
        })
    }, 500)
    return () => {
      vivo = false
      clearTimeout(timer)
    }
  }, [dominio])

  function mostrarErro(e: unknown) {
    const chave = motivoDaRecusa(e)
    setErro(chave ? t(chave) : apiErrorMessage(e, t('auth.erro.generico')))
  }

  async function submeter(e: FormEvent) {
    e.preventDefault()
    setErro('')
    setBusy(true)
    try {
      if (modo === 'entrar') {
        const r = await login(email, password)
        // Com MFA activo a palavra-passe NÃO produz sessão: o servidor devolve
        // um desafio de 5 minutos e só o código a troca por tokens.
        if (r.kind === 'mfa') {
          onDesafio(r.mfa_token)
          return
        }
        onLogin(r.user)
      } else {
        onLogin(await registerOrg(orgName, email, password))
      }
    } catch (err) {
      mostrarErro(err)
    } finally {
      setBusy(false)
    }
  }

  function trocarModo(m: Modo) {
    setModo(m)
    setErro('')
  }

  const ssoVisivel = modo === 'entrar' && sso.activo && !!sso.dominio && (sso.obrigatorio || !esconderSso)
  const registo = modo === 'registo'

  return (
    <div className="auth-form">
      <header className="auth-form__head">
        <h1>{registo ? t('auth.registo.titulo') : t('auth.entrar.titulo')}</h1>
        <p className="dx-muted">{t('auth.entrar.instancia', { host: location.host })}</p>
      </header>

      {registo && <p className="auth-form__nota">{t('auth.registo.explicacao', { app: getAppName() })}</p>}

      <form className="auth-form__campos" onSubmit={submeter} data-testid="auth-form">
        {registo && (
          <Field label={t('auth.registo.nomeOrg')} htmlFor="auth-org">
            <TextInput
              id="auth-org"
              name="organization"
              large
              required
              minLength={2}
              autoComplete="organization"
              value={orgName}
              onChange={(e) => setOrgName(e.target.value)}
            />
          </Field>
        )}

        <Field
          label={registo ? t('auth.registo.emailEmpresa') : t('auth.entrar.email')}
          htmlFor="auth-email"
          hint={sso.aVerificar ? t('auth.sso.aVerificar') : undefined}
        >
          <TextInput
            id="auth-email"
            name="email"
            type="email"
            large
            required
            autoComplete={registo ? 'email' : 'username'}
            data-testid="auth-email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
        </Field>

        {ssoVisivel && (
          <div className="auth-sso" data-testid="auth-sso">
            {sso.obrigatorio ? (
              <Alert tone="warning" icon="shieldCheck">
                {t('auth.sso.obrigatorio', { dominio: sso.dominio })}
              </Alert>
            ) : (
              <p className="dx-muted auth-sso__nota">{t('auth.sso.disponivel', { dominio: sso.dominio })}</p>
            )}
            <Button variant="primary" size="lg" block icon="key" onClick={() => sso.dominio && ssoRedirect(sso.dominio)}>
              {t('auth.sso.botao')}
            </Button>
            {!sso.obrigatorio && <div className="dx-divider-or">{t('auth.entrar.ou')}</div>}
          </div>
        )}

        {!(ssoVisivel && sso.obrigatorio) && (
          <>
            <Field
              label={t('auth.entrar.palavraPasse')}
              htmlFor="auth-password"
              hint={registo ? t('auth.registo.dicaPalavraPasse') : undefined}
            >
              <CampoPalavraPasse
                id="auth-password"
                name="password"
                required
                minLength={registo ? 8 : undefined}
                autoComplete={registo ? 'new-password' : 'current-password'}
                data-testid="auth-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </Field>

            {erro && (
              <div className="auth-error">
                <Alert tone="danger">{erro}</Alert>
              </div>
            )}

            <Button
              type="submit"
              variant="secondary"
              size="lg"
              block
              busy={busy}
              className="auth-submit"
              data-testid="auth-submit"
            >
              {registo ? t('auth.registo.submeter') : t('auth.entrar.continuar')}
            </Button>
          </>
        )}
      </form>

      {!esconderRegisto && (
        <p className="auth-form__troca">
          {registo ? t('auth.registo.jaTens') : t('auth.entrar.semOrganizacao')}{' '}
          <button type="button" className="auth-link" onClick={() => trocarModo(registo ? 'entrar' : 'registo')} data-testid="auth-modo">
            {registo ? t('auth.registo.entrar') : t('auth.entrar.criar')}
          </button>
        </p>
      )}
    </div>
  )
}
