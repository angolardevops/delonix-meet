/**
 * Gateway de SMS da organização (ADR-0005, `server/src/sms.rs`) — só admins;
 * o servidor decide e o cartão mostra a recusa.
 *
 * O servidor não vê USB: o telefone está ligado a outra máquina, onde corre o
 * agente `delonix-sms-gateway`. Este cartão é a vista da organização sobre
 * isso — os gateways (e o token, mostrado UMA vez), o inventário que os
 * agentes reportam, os operadores que a plataforma tem contratados, o envio e
 * a fila.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getSmsRoute } from '../../api'
import { Card } from '../../ui/kit'
import { SmsDevices, SmsOperators } from './SmsDevices'
import { SmsGateways } from './SmsGateways'
import { SmsMessages, SmsSend } from './SmsSend'
import { usePolled } from './smsShared'

export default function SmsCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  // A rota não se repete: muda por PUT, e a resposta do PUT substitui-a.
  const route = usePolled((signal) => getSmsRoute(orgId, signal), [orgId], null)
  const [nonce, setNonce] = useState(0)

  return (
    <Card title={t('consola.sms.titulo')} eyebrow={t('consola.sms.eyebrow')} flush className="org-sms" as="section">
      <p className="dx-muted org-card-note">{t('consola.sms.intro')}</p>
      <SmsGateways orgId={orgId} />
      <SmsDevices orgId={orgId} onRoute={route.put} />
      <SmsOperators route={route.state} onRetry={route.reload} />
      <SmsSend orgId={orgId} onSent={() => setNonce((n) => n + 1)} />
      <SmsMessages orgId={orgId} nonce={nonce} />
    </Card>
  )
}
