/**
 * Assistente «Ligar operadora móvel»: escolher a operadora, ver o que é
 * preciso pedir-lhe, e preencher o tronco. Acaba num `createTrunk` normal.
 *
 * Pré-preenche só o que é padrão numa interligação SIP (ver
 * `operatorPresets.ts`); o endereço do SBC, os canais e as credenciais vêm do
 * contrato de interligação e ficam para quem os tem.
 */
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { Trunk } from '../../api'
import { Alert, Button, Dialog } from '../../ui/kit'
import { ASK_OPERATOR, operatorPreset, OPERATORS, presetForm } from './operatorPresets'
import type { LinkSecurity, OperatorId } from './operatorPresets'
import { TrunkEditor } from './TrunkDialog'

type Step = 'pick' | 'ask' | 'form'
const STEPS: Step[] = ['pick', 'ask', 'form']

export default function OperatorWizard({ orgId, onClose, onDone }: { orgId: string; onClose: () => void; onDone: (saved: Trunk) => void }) {
  const { t } = useTranslation()
  const [step, setStep] = useState<Step>('pick')
  const [operator, setOperator] = useState<OperatorId>('unitel')
  const [security, setSecurity] = useState<LinkSecurity>('tls_srtp')
  const heading = useRef<HTMLHeadingElement>(null)
  const first = useRef(true)

  // Ao mudar de passo o foco vai para o título do passo (o do diálogo trata do primeiro).
  useEffect(() => {
    if (first.current) {
      first.current = false
      return
    }
    heading.current?.focus()
  }, [step])

  const preset = operatorPreset(operator)
  const operatorName = (id: OperatorId) => operatorPreset(id).name || t('telecom.assistente.outra')

  return (
    <Dialog title={t('telecom.assistente.titulo')} onClose={onClose} wide>
      <div className="tel-wizard" data-testid="tel-wizard">
        <p className="dx-eyebrow tel-wizard__step">{t('telecom.assistente.passo', { n: STEPS.indexOf(step) + 1, total: STEPS.length })}</p>
        <h3 ref={heading} tabIndex={-1} className="tel-wizard__title">
          {step === 'pick' ? t('telecom.assistente.escolher') : step === 'ask' ? t('telecom.assistente.pedirTitulo', { nome: operatorName(operator) }) : t('telecom.assistente.preencher', { nome: operatorName(operator) })}
        </h3>

        {step === 'pick' && (
          <>
            <fieldset className="tel-choices">
              <legend className="dx-sr-only">{t('telecom.assistente.escolher')}</legend>
              {OPERATORS.map((o) => (
                <label key={o.id} className="tel-choice">
                  <input type="radio" name="tel-operator" value={o.id} checked={operator === o.id} onChange={() => setOperator(o.id)} />
                  <span className="tel-choice__body">
                    <strong>{operatorName(o.id)}</strong>
                    {o.short_code && <span className="dx-num dx-muted">{o.short_code}</span>}
                  </span>
                </label>
              ))}
            </fieldset>
            <p className="dx-muted tel-small">{t('telecom.assistente.nota')}</p>
            <div className="tel-form__foot">
              <Button variant="secondary" onClick={onClose}>
                {t('ui.cancelar')}
              </Button>
              <Button variant="primary" onClick={() => setStep('ask')}>
                {t('ui.seguinte')}
              </Button>
            </div>
          </>
        )}

        {step === 'ask' && (
          <>
            <p className="tel-small">{t('telecom.assistente.pedirIntro')}</p>
            <ul className="tel-ask">
              {ASK_OPERATOR.map((k) => (
                <li key={k}>{t(`telecom.assistente.pedir.${k}`)}</li>
              ))}
            </ul>
            <fieldset className="tel-choices">
              <legend className="tel-wizard__legend">{t('telecom.assistente.seguranca')}</legend>
              <label className="tel-choice">
                <input type="radio" name="tel-security" checked={security === 'tls_srtp'} onChange={() => setSecurity('tls_srtp')} />
                <span className="tel-choice__body">
                  <strong>{t('telecom.assistente.tlsSrtp')}</strong>
                  <span className="dx-muted">{t('telecom.assistente.tlsSrtpNota')}</span>
                </span>
              </label>
              <label className="tel-choice">
                <input type="radio" name="tel-security" checked={security === 'udp_plain'} onChange={() => setSecurity('udp_plain')} />
                <span className="tel-choice__body">
                  <strong>{t('telecom.assistente.udpClaro')}</strong>
                  <span className="dx-muted">{t('telecom.assistente.udpClaroNota')}</span>
                </span>
              </label>
            </fieldset>
            {security === 'udp_plain' && (
              <Alert tone="warning" icon="lock">
                {t('telecom.form.redePrivada')}
              </Alert>
            )}
            <div className="tel-form__foot">
              <Button variant="secondary" onClick={() => setStep('pick')}>
                {t('ui.voltar')}
              </Button>
              <Button variant="primary" onClick={() => setStep('form')}>
                {t('ui.seguinte')}
              </Button>
            </div>
          </>
        )}

        {step === 'form' && (
          <>
            <p className="dx-muted tel-small">{t('telecom.assistente.vazioNota')}</p>
            <TrunkEditor
              key={`${operator}-${security}`}
              orgId={orgId}
              initial={presetForm(operator, security)}
              prefixesSuggested={preset.suggestedPrefixes.length > 0}
              onDone={onDone}
              onCancel={() => setStep('ask')}
              cancelLabel={t('ui.voltar')}
            />
          </>
        )}
      </div>
    </Dialog>
  )
}
