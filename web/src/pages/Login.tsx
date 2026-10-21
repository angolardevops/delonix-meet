/**
 * Entrar — o primeiro ecrã sem sessão, e também o que substitui a landing.
 *
 * Painel escuro com a proposta de valor à esquerda; à direita o formulário,
 * que é uma de duas coisas: email/palavra-passe (com SSO por domínio e criação
 * de organização) ou o desafio do segundo factor. Com `pendingRoom` — um link
 * de sala aberto sem sessão — diz-se à pessoa para onde vai a seguir.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { User } from '../api'
import { Icon } from '../ui/icons'
import '../ui/auth.css'
import DesafioMfa from './auth/DesafioMfa'
import EntrarComCodigo from './auth/EntrarComCodigo'
import FormularioEntrada from './auth/FormularioEntrada'
import PedirReposicao from './auth/PedirReposicao'
import PainelValor from './auth/PainelValor'
import SeletorLingua from './auth/SeletorLingua'

export default function Login({
  pendingRoom,
  onLogin,
  initialMfaToken = null,
}: {
  pendingRoom: string | null
  onLogin: (u: User) => void
  /** Desafio já devolvido pelo SSO (A4) — ver `App.tsx`, rota `#/sso-mfa`. */
  initialMfaToken?: string | null
}) {
  const { t } = useTranslation()
  /** Desafio de segundo factor devolvido pelo login (JWT `typ: "mfa"`, 5 min). */
  const [mfaToken, setMfaToken] = useState<string | null>(initialMfaToken)
  /** «Esqueci-me da palavra-passe»: o formulário dá lugar ao pedido de email. */
  const [esqueci, setEsqueci] = useState(false)

  return (
    <div className="auth">
      <PainelValor />
      <main className="auth-main">
        <div className="auth-card">
          {pendingRoom && (
            <div className="auth-pendente" role="status" data-testid="auth-pendente">
              <Icon name="door" />
              <div className="auth-pendente__texto">
                <span>
                  {t('auth.sala.pendente')} <code className="dx-num">{pendingRoom}</code>
                </span>
                <button type="button" className="auth-link auth-link--muted" onClick={() => (location.hash = '/login')}>
                  {t('auth.sala.naoEntrar')}
                </button>
              </div>
            </div>
          )}

          <div className="auth-topo">
            <SeletorLingua />
          </div>

          {mfaToken ? (
            <DesafioMfa mfaToken={mfaToken} onLogin={onLogin} onRecomecar={() => setMfaToken(null)} />
          ) : esqueci ? (
            <PedirReposicao onVoltar={() => setEsqueci(false)} />
          ) : (
            <FormularioEntrada onLogin={onLogin} onDesafio={setMfaToken} onEsqueci={() => setEsqueci(true)} />
          )}

          {!mfaToken && !esqueci && !pendingRoom && <EntrarComCodigo />}

          <p className="auth-termos">
            {t('auth.entrar.termos')} <a href="#/legal">{t('auth.entrar.termosLink')}</a>.
          </p>
        </div>
      </main>
    </div>
  )
}
