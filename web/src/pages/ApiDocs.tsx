import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { BrandLockup, BrandMark } from '../components/BrandMark'

/** Documentação pública da API REST do Delonix Meet (#/api-docs). */
export default function ApiDocs() {
  const { t } = useTranslation()
  const base = `${location.origin}`
  return (
    <div className="apidoc-page">
      <div className="apidoc-wrap">
        <header className="apidoc-head">
          <BrandMark big />
          <h1>
            <BrandLockup suffix="· API REST" />
          </h1>
          <p className="muted">
            
            {t('api.integraODelonixMeet')}
          </p>
        </header>

        <Section title={t('api.autenticacao')}>
          <p>{t('api.todasAsChamadas')}<code>/api/v1</code>  {t('api.exigemUma')} <strong>{t('api.chaveDeApi')}</strong>  {t('api.daOrganizacaoGeraA')} <em>{t('api.caminhoChaves')}</em>{t('api.comoAdministradorEnviaA')}
          </p>
          <Code>{`Authorization: Bearer dlx_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
# ou
X-API-Key: dlx_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx`}</Code>
          <p className="muted small">
            
            {t('api.aChaveSoE')}
          </p>
        </Section>

        <Section title={t('api.baseUrl')}>
          <Code>{`${base}/api/v1`}</Code>
        </Section>

        <Endpoint
          method="POST"
          path="/api/v1/rooms"
          desc={t('api.criaSala')}
          body={`{
  "name": "Sync semanal",   // opcional
  "e2ee": false,             // opcional — encriptação ponta-a-ponta
  "waiting_room": false      // opcional — admitir convidados manualmente
}`}
          resp={`{
  "code": "abc-defg-hij",
  "name": "Sync semanal",
  "e2ee": false,
  "waiting_room": false,
  "join_url": "https://meet.acme.com/#/r/abc-defg-hij"
}`}
          curl={`curl -X POST ${base}/api/v1/rooms \\
  -H "Authorization: Bearer dlx_..." \\
  -H "Content-Type: application/json" \\
  -d '{"name":"Sync semanal"}'`}
        />

        <Endpoint
          method="GET"
          path="/api/v1/rooms/{code}"
          desc={t('api.metadadosSala')}
          resp={`{ "code": "abc-defg-hij", "name": "...", "e2ee": false, "waiting_room": false, "join_url": "..." }`}
          curl={`curl ${base}/api/v1/rooms/abc-defg-hij -H "Authorization: Bearer dlx_..."`}
        />

        <Endpoint
          method="GET"
          path="/api/v1/recordings"
          desc={t('api.listaGravacoes')}
          resp={`{
  "recordings": [
    { "id": "…", "filename": "…", "size_bytes": 12345678,
      "created_at": "2026-07-07T20:00:00Z", "room_code": "abc-defg-hij",
      "download_url": "/api/recordings/…" }
  ]
}`}
          curl={`curl ${base}/api/v1/recordings -H "Authorization: Bearer dlx_..."`}
        />

        <Endpoint
          method="GET"
          path="/api/v1/org"
          desc={t('api.infoOrg')}
          resp={`{ "id": "…", "name": "Acme", "email_domain": "acme.com", "domain": "meet.acme.com", "members": 42 }`}
          curl={`curl ${base}/api/v1/org -H "Authorization: Bearer dlx_..."`}
        />

        <Section title={t('api.webhooks')}>
          <p>{t('api.alemDaApi')}<strong>webhooks</strong>  {t('api.emSlackTeamsMattermost')} <em>{t('api.caminhoWebhooks')}</em>{t('api.eventos')}
          </p>
          <ul className="apidoc-list">
            <li><code>meeting.created</code>  {t('api.reuniaoAgendada')}</li>
            <li><code>meeting.started</code>  {t('api.reuniaoIniciadaCom')} <code>join_url</code>)</li>
            <li><code>recording.ready</code>  {t('api.gravacaoDisponivel')}</li>
          </ul>
          <p>{t('api.noDestino')}<strong>genérico</strong>{t('api.oPayloadJsonVem')} <code>X-Delonix-Signature: sha256=…</code>  {t('api.validaOParaGarantir')}
          </p>
          <Code>{`{
  "event": "meeting.started",
  "title": "Delonix Meet",
  "text": "Reunião «Sync» começou · https://meet.acme.com/#/r/abc-defg-hij",
  "data": { "meeting_id": "…", "room_code": "abc-defg-hij", "link": "…", "kind": "video" }
}`}</Code>
        </Section>

        <Section title={t('api.codigosDeEstado')}>
          <ul className="apidoc-list">
            <li><code>200</code>  {t('api.sucesso')}</li>
            <li><code>401</code>  {t('api.chaveDeApiEm')}</li>
            <li><code>404</code>  {t('api.recursoNaoEncontrado')}</li>
            <li><code>409</code>  {t('api.conflitoExDominioJa')}</li>
          </ul>
        </Section>

        <a className="link" href="#/">{t('api.voltarAoDelonixMeet')}</a>
      </div>
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="apidoc-section">
      <h2>{title}</h2>
      {children}
    </section>
  )
}

function Code({ children }: { children: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <pre className="apidoc-code" onClick={() => { void navigator.clipboard.writeText(children); setCopied(true); setTimeout(() => setCopied(false), 1200) }}>
      <code>{children}</code>
      <span className="apidoc-copy">{copied ? '✓ copiado' : '⧉'}</span>
    </pre>
  )
}

function Endpoint({
  method, path, desc, body, resp, curl,
}: {
  method: string; path: string; desc: string; body?: string; resp: string; curl: string
}) {
  const { t } = useTranslation()
  return (
    <section className="apidoc-section apidoc-endpoint">
      <div className="apidoc-ep-head">
        <span className={`apidoc-method ${method.toLowerCase()}`}>{method}</span>
        <code className="apidoc-path">{path}</code>
      </div>
      <p>{desc}</p>
      {body && (<><h4>{t('api.corpo')}</h4><Code>{body}</Code></>)}
      <h4>{t('api.resposta')}</h4>
      <Code>{resp}</Code>
      <h4>{t('api.exemploCurl')}</h4>
      <Code>{curl}</Code>
    </section>
  )
}
