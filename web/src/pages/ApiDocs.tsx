import { useState } from 'react'

/** Documentação pública da API REST do Delonix Meet (#/api-docs). */
export default function ApiDocs() {
  const base = `${location.origin}`
  return (
    <div className="apidoc-page">
      <div className="apidoc-wrap">
        <header className="apidoc-head">
          <img src="/logo.svg" alt="" className="brand-logo big" />
          <h1>
            Delonix <span>Meet</span> · API REST
          </h1>
          <p className="muted">
            Integra o Delonix Meet noutras plataformas: cria salas, obtém links de reunião e lista
            gravações — tudo por HTTP, autenticado com uma chave de API da tua organização.
          </p>
        </header>

        <Section title="Autenticação">
          <p>
            Todas as chamadas <code>/api/v1</code> exigem uma <strong>chave de API</strong> da organização
            (gera-a em <em>Análises → Chaves de API</em>, como administrador). Envia-a num destes headers:
          </p>
          <Code>{`Authorization: Bearer dlx_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
# ou
X-API-Key: dlx_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx`}</Code>
          <p className="muted small">
            A chave só é mostrada uma vez, na criação. Guarda-a em segredo — quem a tiver age em nome da
            tua organização. Podes revogá-la a qualquer momento.
          </p>
        </Section>

        <Section title="Base URL">
          <Code>{`${base}/api/v1`}</Code>
        </Section>

        <Endpoint
          method="POST"
          path="/api/v1/rooms"
          desc="Cria uma sala de reunião e devolve o código + link de entrada."
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
          desc="Metadados de uma sala existente."
          resp={`{ "code": "abc-defg-hij", "name": "...", "e2ee": false, "waiting_room": false, "join_url": "..." }`}
          curl={`curl ${base}/api/v1/rooms/abc-defg-hij -H "Authorization: Bearer dlx_..."`}
        />

        <Endpoint
          method="GET"
          path="/api/v1/recordings"
          desc="Lista as gravações da organização (até 200, mais recentes primeiro)."
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
          desc="Informação da organização a que a chave pertence."
          resp={`{ "id": "…", "name": "Acme", "email_domain": "acme.com", "domain": "meet.acme.com", "members": 42 }`}
          curl={`curl ${base}/api/v1/org -H "Authorization: Bearer dlx_..."`}
        />

        <Section title="Webhooks (eventos)">
          <p>
            Além da API, a organização pode receber <strong>webhooks</strong> em Slack, Teams, Mattermost ou
            num endpoint genérico (configura em <em>Análises → Webhooks</em>). Eventos:
          </p>
          <ul className="apidoc-list">
            <li><code>meeting.created</code> — reunião agendada</li>
            <li><code>meeting.started</code> — reunião iniciada (com <code>join_url</code>)</li>
            <li><code>recording.ready</code> — gravação disponível</li>
          </ul>
          <p>
            No destino <strong>genérico</strong>, o payload JSON vem assinado com HMAC-SHA256 (chave = o
            segredo do webhook) no header <code>X-Delonix-Signature: sha256=…</code> — valida-o para
            garantir a autenticidade.
          </p>
          <Code>{`{
  "event": "meeting.started",
  "title": "Delonix Meet",
  "text": "Reunião «Sync» começou · https://meet.acme.com/#/r/abc-defg-hij",
  "data": { "meeting_id": "…", "room_code": "abc-defg-hij", "link": "…", "kind": "video" }
}`}</Code>
        </Section>

        <Section title="Códigos de estado">
          <ul className="apidoc-list">
            <li><code>200</code> — sucesso</li>
            <li><code>401</code> — chave de API em falta ou inválida</li>
            <li><code>404</code> — recurso não encontrado</li>
            <li><code>409</code> — conflito (ex.: domínio já registado)</li>
          </ul>
        </Section>

        <a className="link" href="#/">← Voltar ao Delonix Meet</a>
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
  return (
    <section className="apidoc-section apidoc-endpoint">
      <div className="apidoc-ep-head">
        <span className={`apidoc-method ${method.toLowerCase()}`}>{method}</span>
        <code className="apidoc-path">{path}</code>
      </div>
      <p>{desc}</p>
      {body && (<><h4>Corpo</h4><Code>{body}</Code></>)}
      <h4>Resposta</h4>
      <Code>{resp}</Code>
      <h4>Exemplo (curl)</h4>
      <Code>{curl}</Code>
    </section>
  )
}
