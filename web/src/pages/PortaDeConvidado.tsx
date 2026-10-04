/**
 * A porta de uma reunião para quem NÃO tem sessão.
 *
 * Um link de sala aberto sem conta já não cai no login: mostra a entrada de
 * convidado (um nome, e o anfitrião admite). O login continua a um botão de
 * distância, com a sala como destino. Com um bilhete de convidado guardado —
 * depois de entrar, ou num F5 a meio da reunião — vai-se directo à sala.
 *
 * A sala é a MESMA dos membros. O que muda está à volta dela: não há presença
 * nem chamadas directas (são da conta), e sair devolve a pessoa a este ecrã em
 * vez de a mandar para uma consola que ela não tem.
 */
import { lazy, Suspense, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { User } from '../api'
import { PresencaAusente } from '../components/PresenceProvider'
import { convidadoDe, esquecerConvidado, type SessaoDeConvidado } from '../convidado'
import { Signaling } from '../signaling'
import { Button, Spinner } from '../ui/kit'
import '../ui/auth.css'
import EntradaDeConvidado from './auth/EntradaDeConvidado'
import PainelValor from './auth/PainelValor'
import SeletorLingua from './auth/SeletorLingua'
import Login from './Login'

const Room = lazy(() => import('./Room'))

type Estado = { fase: 'entrada' } | { fase: 'sala'; sessao: SessaoDeConvidado } | { fase: 'saiu' } | { fase: 'conta' }

export default function PortaDeConvidado({ code, voice, onLogin }: { code: string; voice: boolean; onLogin: (u: User) => void }) {
  const { t } = useTranslation()
  const [estado, setEstado] = useState<Estado>(() => {
    const guardada = convidadoDe(code)
    return guardada ? { fase: 'sala', sessao: guardada } : { fase: 'entrada' }
  })

  if (estado.fase === 'conta') return <Login pendingRoom={code} onLogin={onLogin} />

  if (estado.fase === 'sala') {
    return (
      <PresencaAusente>
        <Suspense
          fallback={
            <div className="wait-screen" role="status">
              <Spinner />
            </div>
          }
        >
          <Room
            key={code}
            code={code}
            voiceOnly={voice}
            onLeave={() => {
              // Saiu de vez: o bilhete e o lugar reservado deixam de servir.
              esquecerConvidado(code)
              Signaling.esquecerSegredo(code)
              setEstado({ fase: 'saiu' })
            }}
          />
        </Suspense>
      </PresencaAusente>
    )
  }

  return (
    <div className="auth">
      <PainelValor />
      <main className="auth-main">
        <div className="auth-card">
          <div className="auth-topo">
            <SeletorLingua />
          </div>
          {estado.fase === 'saiu' ? (
            <div className="auth-form" data-testid="convidado-saiu">
              <header className="auth-form__head">
                <h1>{t('auth.convidado.saiuTitulo')}</h1>
                <p className="dx-muted">{t('auth.convidado.saiuTexto')}</p>
              </header>
              <Button variant="secondary" size="lg" block onClick={() => setEstado({ fase: 'entrada' })}>
                {t('auth.convidado.voltarAEntrar')}
              </Button>
              <p className="auth-form__troca">
                {t('auth.convidado.temConta')}{' '}
                <button type="button" className="auth-link" onClick={() => setEstado({ fase: 'conta' })}>
                  {t('auth.convidado.iniciarSessao')}
                </button>
              </p>
            </div>
          ) : (
            <EntradaDeConvidado
              code={code}
              onEntrou={(sessao) => setEstado({ fase: 'sala', sessao })}
              onTenhoConta={() => setEstado({ fase: 'conta' })}
            />
          )}
          <p className="auth-termos">
            {t('auth.entrar.termos')} <a href="#/legal">{t('auth.entrar.termosLink')}</a>.
          </p>
        </div>
      </main>
    </div>
  )
}
