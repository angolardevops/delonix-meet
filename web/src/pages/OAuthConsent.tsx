/**
 * Ecrã de consentimento de um agente (`#/oauth/consent?…`, ADR-0028 §5).
 *
 * EXIGE sessão: é a pessoa que decide, e o que o agente fizer fica na trilha
 * de auditoria com o nome dela. Sem sessão o `App` manda ao login e o endereço
 * fica — o fluxo retoma aqui.
 *
 * Três coisas que este ecrã tem de dizer, e diz:
 * - **quem** pede (e se se registou a si mesmo, RFC 7591 — ninguém o aprovou);
 * - **em que organização** o agente vai agir;
 * - **cada escopo** com o que abre, separando o que só lê do que mexe.
 *
 * Não desenha os escopos: o texto de cada um vem do servidor
 * (`McpScope::describes`), para que acrescentar um escopo não deixe uma frase
 * por traduzir num ecrã onde a pessoa está a dar permissões.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  oauthConsentInfo,
  oauthDecide,
  type OAuthConsentInfo,
  type OAuthRequestParams,
} from '../api'
import { Alert, Button, Card, Spinner } from '../ui/kit'
import { Icon } from '../ui/icons'
import Moldura from './publico/Moldura'

/** Lê os parâmetros do fluxo do próprio endereço (`#/oauth/consent?…`). */
function paramsDoEndereco(): OAuthRequestParams | null {
  const i = location.hash.indexOf('?')
  if (i < 0) return null
  const q = new URLSearchParams(location.hash.slice(i + 1))
  const obrigatorios = [
    'response_type',
    'client_id',
    'redirect_uri',
    'scope',
    'code_challenge',
    'code_challenge_method',
    'resource',
  ] as const
  if (obrigatorios.some((k) => !q.get(k))) return null
  const p: OAuthRequestParams = {
    response_type: q.get('response_type')!,
    client_id: q.get('client_id')!,
    redirect_uri: q.get('redirect_uri')!,
    scope: q.get('scope')!,
    code_challenge: q.get('code_challenge')!,
    code_challenge_method: q.get('code_challenge_method')!,
    resource: q.get('resource')!,
  }
  const state = q.get('state')
  if (state) p.state = state
  return p
}

/** O host do `redirect_uri`, que é o que a pessoa pode reconhecer. */
function destino(uri: string): string {
  try {
    return new URL(uri).host
  } catch {
    return uri
  }
}

export default function OAuthConsent() {
  const { t } = useTranslation()
  const [params] = useState<OAuthRequestParams | null>(paramsDoEndereco)
  const [info, setInfo] = useState<OAuthConsentInfo | null>(null)
  const [erro, setErro] = useState<string | null>(null)
  const [busy, setBusy] = useState<'nenhum' | 'aprovar' | 'negar'>('nenhum')

  useEffect(() => {
    if (!params) {
      setErro(t('oauth.consent.erroPedido'))
      return
    }
    const ac = new AbortController()
    oauthConsentInfo(params, ac.signal)
      .then(setInfo)
      .catch((e: unknown) => {
        if (!ac.signal.aborted) setErro(e instanceof Error ? e.message : t('ui.erroGenerico'))
      })
    return () => ac.abort()
  }, [params, t])

  async function decidir(aprovar: boolean) {
    if (!params) return
    setBusy(aprovar ? 'aprovar' : 'negar')
    try {
      const r = await oauthDecide(params, aprovar)
      // Sai da aplicação para o cliente: é ele que troca o código pelo token.
      location.href = r.redirect_to
    } catch (e) {
      setErro(e instanceof Error ? e.message : t('ui.erroGenerico'))
      setBusy('nenhum')
    }
  }

  if (erro) {
    return (
      <Moldura pagina="oauth-consent" estreita>
        <Card className="pub-cartao">
          <div className="org-form">
            <h1>{t('oauth.consent.titulo')}</h1>
            <Alert tone="danger">{erro}</Alert>
            <Button variant="secondary" block onClick={() => (location.hash = '/')}>
              {t('oauth.consent.voltar')}
            </Button>
          </div>
        </Card>
      </Moldura>
    )
  }

  if (!info || !params) {
    return (
      <Moldura pagina="oauth-consent" estreita>
        <Card className="pub-cartao">
          <Spinner label={t('oauth.consent.aCarregar')} />
        </Card>
      </Moldura>
    )
  }

  const leitura = info.scopes.filter((s) => s.read_only)
  const escrita = info.scopes.filter((s) => !s.read_only)

  return (
    <Moldura pagina="oauth-consent" estreita>
      <Card className="pub-cartao">
        <div className="org-form">
          <h1>{t('oauth.consent.titulo')}</h1>
          <p>
            {t('oauth.consent.pedido', {
              cliente: info.client_name,
              organizacao: info.organization_name,
            })}
          </p>

          {info.registered_dynamically && (
            <Alert tone="warning" icon="alert">
              {t('oauth.consent.registoProprio', { destino: destino(params.redirect_uri) })}
            </Alert>
          )}

          {escrita.length > 0 && (
            <section className="org-form__section">
              <h2>{t('oauth.consent.vaiPoderMexer')}</h2>
              <ul className="oauth-consent__escopos">
                {escrita.map((s) => (
                  <li key={s.scope}>
                    <Icon name="alert" /> <strong>{s.describes}</strong>{' '}
                    <code className="dx-muted">{s.scope}</code>
                  </li>
                ))}
              </ul>
            </section>
          )}

          {leitura.length > 0 && (
            <section className="org-form__section">
              <h2>{t('oauth.consent.vaiPoderVer')}</h2>
              <ul className="oauth-consent__escopos">
                {leitura.map((s) => (
                  <li key={s.scope}>
                    {s.describes} <code className="dx-muted">{s.scope}</code>
                  </li>
                ))}
              </ul>
            </section>
          )}

          <Alert tone="warning" icon="info">
            {t('oauth.consent.agePorTi')}
          </Alert>

          {info.already_granted && <p className="dx-muted">{t('oauth.consent.jaConsentido')}</p>}

          <div className="org-form__foot">
            <Button
              variant="primary"
              size="lg"
              busy={busy === 'aprovar'}
              disabled={busy !== 'nenhum'}
              onClick={() => void decidir(true)}
            >
              {t('oauth.consent.autorizar')}
            </Button>
            <Button
              variant="secondary"
              size="lg"
              busy={busy === 'negar'}
              disabled={busy !== 'nenhum'}
              onClick={() => void decidir(false)}
            >
              {t('oauth.consent.negar')}
            </Button>
          </div>
          <p className="dx-muted">{t('oauth.consent.revogarDepois')}</p>
        </div>
      </Card>
    </Moldura>
  )
}
