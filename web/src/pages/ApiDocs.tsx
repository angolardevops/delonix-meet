/**
 * Documentação pública da API v1 (`/api/v1`, autenticada por chave de API).
 *
 * Só aparece o que está no router de `server/src/main.rs` e com a forma que
 * `server/src/apikeys.rs` devolve. Os exemplos são montados a partir de
 * objectos (JSON.stringify), não escritos à mão: um exemplo que não é JSON
 * válido é pior do que nenhum.
 */
import { useTranslation } from 'react-i18next'
import { Card, Tag } from '../ui/kit'
import BlocoCodigo from './publico/BlocoCodigo'
import Moldura from './publico/Moldura'

const AUTH = 'Authorization'
const API_KEY = 'X-API-Key'
const CT = 'Content-Type'
const CHAVE = 'dlx_…'
const CODIGO = 'abc-defg-hij'

const json = (v: unknown) => JSON.stringify(v, null, 2)

interface Endpoint {
  metodo: 'GET' | 'POST'
  caminho: string
  desc: string
  campos?: { nome: string; tipo: string; desc: string }[]
  corpo?: unknown
  resposta: unknown
  curl: (base: string) => string
}

function endpoints(base: string): Endpoint[] {
  const sala = {
    code: CODIGO,
    name: 'sync',
    e2ee: false,
    waiting_room: false,
    join_url: `${base}/#/r/${CODIGO}`,
  }
  return [
    {
      metodo: 'POST',
      caminho: '/api/v1/rooms',
      desc: 'publico.api.criaSala',
      campos: [
        { nome: 'name', tipo: 'string', desc: 'publico.api.campoNome' },
        { nome: 'e2ee', tipo: 'boolean', desc: 'publico.api.campoE2ee' },
        { nome: 'waiting_room', tipo: 'boolean', desc: 'publico.api.campoEspera' },
      ],
      corpo: { name: 'sync', e2ee: false, waiting_room: false },
      resposta: sala,
      curl: (b) =>
        [
          `curl -X POST ${b}/api/v1/rooms \\`,
          `  -H "${AUTH}: Bearer ${CHAVE}" \\`,
          `  -H "${CT}: application/json" \\`,
          `  -d '${JSON.stringify({ name: 'sync' })}'`,
        ].join('\n'),
    },
    {
      metodo: 'GET',
      caminho: '/api/v1/rooms/{code}',
      desc: 'publico.api.metadadosSala',
      resposta: sala,
      curl: (b) => [`curl ${b}/api/v1/rooms/${CODIGO} \\`, `  -H "${AUTH}: Bearer ${CHAVE}"`].join('\n'),
    },
    {
      metodo: 'GET',
      caminho: '/api/v1/recordings',
      desc: 'publico.api.listaGravacoes',
      resposta: {
        recordings: [
          {
            id: '…',
            filename: '…',
            size_bytes: 12345678,
            created_at: '2026-07-07T20:00:00Z',
            room_code: CODIGO,
            download_url: '/api/recordings/…',
          },
        ],
      },
      curl: (b) => [`curl ${b}/api/v1/recordings \\`, `  -H "${AUTH}: Bearer ${CHAVE}"`].join('\n'),
    },
    {
      metodo: 'GET',
      caminho: '/api/v1/org',
      desc: 'publico.api.infoOrg',
      resposta: { id: '…', name: 'acme', email_domain: 'acme.example', domain: 'meet.acme.example', members: 42 },
      curl: (b) => [`curl ${b}/api/v1/org \\`, `  -H "${AUTH}: Bearer ${CHAVE}"`].join('\n'),
    },
  ]
}

const EVENTOS: { nome: string; desc: string }[] = [
  { nome: 'meeting.created', desc: 'publico.api.evCriada' },
  { nome: 'meeting.started', desc: 'publico.api.evIniciada' },
  { nome: 'meeting.mom_ready', desc: 'publico.api.evAta' },
  { nome: 'recording.ready', desc: 'publico.api.evGravacao' },
]

const CODIGOS_HTTP = ['200', '401', '404', '409', '429']

export default function ApiDocs() {
  const { t } = useTranslation()
  const base = location.origin

  return (
    <Moldura pagina="api-docs">
      <header className="pub-cabecalho">
        <h1>{t('publico.api.titulo')}</h1>
        <p className="dx-muted">{t('publico.api.intro')}</p>
      </header>

      <div className="pub-grelha">
        <Card title={t('publico.api.autenticacao')} className="pub-cartao">
          <div className="pub-prosa">
            <p>{t('publico.api.autenticacaoTexto')}</p>
            <BlocoCodigo codigo={[`${AUTH}: Bearer ${CHAVE}`, `${API_KEY}: ${CHAVE}`].join('\n')} />
            <p className="dx-muted">{t('publico.api.chaveSegredo')}</p>
          </div>
        </Card>
        <Card title={t('publico.api.baseUrl')} className="pub-cartao">
          <BlocoCodigo codigo={`${base}/api/v1`} />
        </Card>
      </div>

      <h2 className="pub-subtitulo">{t('publico.api.endpoints')}</h2>
      {endpoints(base).map((ep) => (
        <Card
          key={ep.metodo + ep.caminho}
          className="pub-cartao pub-endpoint"
          title={
            <span className="pub-endpoint__titulo">
              <Tag tone={ep.metodo === 'POST' ? 'accent' : 'success'}>{ep.metodo}</Tag>
              <code className="dx-num">{ep.caminho}</code>
            </span>
          }
        >
          <div className="pub-prosa">
            <p>{t(ep.desc)}</p>
            {ep.campos && (
              <div className="dx-table-wrap">
                <table className="dx-table">
                  <thead>
                    <tr>
                      <th>{t('publico.api.campo')}</th>
                      <th>{t('publico.api.descricao')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {ep.campos.map((c) => (
                      <tr key={c.nome}>
                        <td>
                          <code className="dx-num">{c.nome}</code>{' '}
                          <span className="dx-muted dx-num">
                            {c.tipo} · {t('publico.api.opcional')}
                          </span>
                        </td>
                        <td>{t(c.desc)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            <div className="pub-exemplos">
              {ep.corpo !== undefined && <BlocoCodigo rotulo={t('publico.api.corpo')} codigo={json(ep.corpo)} />}
              <BlocoCodigo rotulo={t('publico.api.resposta')} codigo={json(ep.resposta)} />
              <BlocoCodigo rotulo={t('publico.api.exemplo')} codigo={ep.curl(base)} />
            </div>
          </div>
        </Card>
      ))}

      <div className="pub-grelha">
        <Card title={t('publico.api.webhooks')} className="pub-cartao">
          <div className="pub-prosa">
            <p>{t('publico.api.webhooksTexto')}</p>
            <BlocoCodigo codigo={'X-Delonix-Signature: sha256=…'} />
            <p className="dx-muted">{t('publico.api.webhooksValida')}</p>
            <div className="dx-table-wrap">
              <table className="dx-table">
                <thead>
                  <tr>
                    <th>{t('publico.api.evento')}</th>
                    <th>{t('publico.api.descricao')}</th>
                  </tr>
                </thead>
                <tbody>
                  {EVENTOS.map((e) => (
                    <tr key={e.nome}>
                      <td>
                        <code className="dx-num">{e.nome}</code>
                      </td>
                      <td>{t(e.desc)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </Card>

        <Card title={t('publico.api.codigos')} className="pub-cartao">
          <div className="dx-table-wrap">
            <table className="dx-table">
              <thead>
                <tr>
                  <th>{t('publico.api.codigo')}</th>
                  <th>{t('publico.api.descricao')}</th>
                </tr>
              </thead>
              <tbody>
                {CODIGOS_HTTP.map((c) => (
                  <tr key={c}>
                    <td className="dx-num">{c}</td>
                    <td>{t(`publico.api.c${c}`)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>
    </Moldura>
  )
}
