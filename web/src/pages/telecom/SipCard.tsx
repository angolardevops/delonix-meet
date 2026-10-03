/**
 * «Registo SIP»: o que as definições e o estado do registo dizem. A password
 * nunca vem num GET — só se sabe se existe. Revelar credenciais e reiniciar o
 * registo não existem neste ecrã.
 */
import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { SipRegistration, SipSettings, TelephonyComponent } from '../../api'
import { Card } from '../../ui/kit'
import { formatAgo, useLocaleTag } from '../admin/orgShared'
import { useTelecomText } from './shared'

export default function SipCard({ settings: s, registration: r }: { settings: SipSettings; registration: SipRegistration }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const { reason, label } = useTelecomText()
  const naoDefinido = <span className="dx-muted">{t('telecom.sip.naoDefinido')}</span>
  const semMedicao = <span className="dx-muted">{t('telecom.semMedicao')}</span>
  const text = (v: string | null | undefined): ReactNode => (v ? v : naoDefinido)
  /** Um componente que não respondeu diz porquê; nunca uma versão inventada. */
  const component = (c: TelephonyComponent | null | undefined, error: string | null | undefined): ReactNode =>
    c ? [c.software, c.version].filter(Boolean).join(' ') : error ? <span className="dx-muted">{reason(error)}</span> : semMedicao
  const updated = formatAgo(s.updated_at, locale)

  const rows: [string, ReactNode][] = [
    [t('telecom.sip.dominio'), text(s.domain)],
    [t('telecom.sip.sbc'), text(s.sbc_host)],
    [t('telecom.sip.transporte'), s.transport ? s.transport.toUpperCase() : naoDefinido],
    [t('telecom.sip.srtp'), s.srtp ? label('srtp', s.srtp) : naoDefinido],
    [t('telecom.sip.codecs'), s.codecs.length > 0 ? s.codecs.join(' · ') : naoDefinido],
    [t('telecom.sip.codecsOferecidos'), r.codecs_offered.length > 0 ? r.codecs_offered.join(' · ') : semMedicao],
    [t('telecom.sip.utilizador'), text(s.username)],
    [t('telecom.sip.password'), s.password_configured ? t('telecom.sip.passwordSim') : t('telecom.sip.passwordNao')],
    [t('telecom.sip.softwareSbc'), component(r.sbc, r.sbc_error)],
    [t('telecom.sip.softwareMedia'), component(r.media, r.media_error)],
  ]

  return (
    <Card title={t('telecom.sip.titulo')} eyebrow={updated ? t('telecom.sip.alterado', { quando: updated }) : undefined} className="tel-card">
      {!s.configured && <p className="dx-muted tel-small tel-sip__warn">{t('telecom.sip.porConfigurar')}</p>}
      <dl className="tel-kv" data-testid="tel-sip">
        {rows.map(([k, v]) => (
          <div key={k} className="tel-kv__row">
            <dt className="dx-muted">{k}</dt>
            <dd className="dx-num tel-break">{v}</dd>
          </div>
        ))}
      </dl>
    </Card>
  )
}
