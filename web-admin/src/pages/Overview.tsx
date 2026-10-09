/**
 * Visão geral — `GET /api/operator/v1/nodes`. Esta rota serve dois papéis ao
 * mesmo tempo: é a lista de nós de media da plataforma E a detecção de "sou
 * administrador de plataforma" (403 se não for, 404 se a instalação nem tem
 * superfície de operador). As duas recusas distinguem-se — dizem coisas
 * diferentes a quem as vê — e por isso não passam pelo `guarded()` genérico
 * (que só sabe 403).
 */
import { useTranslation } from 'react-i18next'
import { ApiError, listNodes, MediaNode, MediaNodeList } from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import { Card, Empty, StatusBadge } from '../ui/kit'
import type { BadgeTone } from '../ui/kit'

type NodesView = { kind: 'forbidden' } | { kind: 'no_surface' } | { kind: 'ok'; data: MediaNodeList }

function statusTone(status: string): BadgeTone {
  if (status === 'serving') return 'success'
  if (status === 'draining') return 'warning'
  return 'neutral'
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <Card>
      <div className="dx-muted" style={{ fontSize: 10.5 }}>
        {label}
      </div>
      <div className="dx-num" style={{ fontSize: 22, fontWeight: 700 }}>
        {value}
      </div>
    </Card>
  )
}

export default function Overview() {
  const { t } = useTranslation()
  const { state, reload } = useAsync<NodesView>(
    (signal) =>
      listNodes(signal)
        .then((data) => ({ kind: 'ok' as const, data }))
        .catch((e) => {
          if (e instanceof ApiError && e.status === 403) return { kind: 'forbidden' as const }
          if (e instanceof ApiError && e.status === 404) return { kind: 'no_surface' as const }
          throw e
        }),
    [],
  )
  return (
    <div className="page">
      <AsyncSection state={state} onRetry={reload}>
        {(v) => {
          if (v.kind === 'forbidden') {
            return (
              <Empty icon="lock" title={t('nodes.semAcesso.titulo')}>
                {t('nodes.semAcesso.texto')}
              </Empty>
            )
          }
          if (v.kind === 'no_surface') {
            return (
              <Empty icon="server" title={t('nodes.semSuperficie.titulo')}>
                {t('nodes.semSuperficie.texto')}
              </Empty>
            )
          }
          return <NodesBody data={v.data} />
        }}
      </AsyncSection>
    </div>
  )
}

function NodesBody({ data }: { data: MediaNodeList }) {
  const { t } = useTranslation()
  return (
    <>
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        <Stat label={t('nodes.resumo.serving')} value={data.serving} />
        <Stat label={t('nodes.resumo.draining')} value={data.draining} />
        <Stat label={t('nodes.resumo.unreachable')} value={data.unreachable} />
        <Stat label={t('nodes.resumo.peers')} value={data.peers} />
      </div>
      <Card title={t('nodes.titulo')} flush>
        {data.items.length === 0 ? (
          <div className="dx-card__body">
            <p className="dx-muted">{t('nodes.vazio')}</p>
          </div>
        ) : (
          <div className="dx-table-wrap">
            <table className="dx-table">
              <thead>
                <tr>
                  <th>{t('nodes.tabela.hostname')}</th>
                  <th>{t('nodes.tabela.estado')}</th>
                  <th>{t('nodes.tabela.versao')}</th>
                  <th>{t('nodes.tabela.carga')}</th>
                  <th>{t('nodes.tabela.salas')}</th>
                  <th>{t('nodes.tabela.participantes')}</th>
                  <th>{t('nodes.tabela.visto')}</th>
                </tr>
              </thead>
              <tbody>
                {data.items.map((n) => (
                  <NodeRow key={n.node_id} node={n} />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </>
  )
}

function NodeRow({ node }: { node: MediaNode }) {
  const { t } = useTranslation()
  return (
    <tr>
      <td>
        <strong className="dx-num">{node.hostname}</strong>
        <div className="dx-muted" style={{ fontSize: 10.5 }}>
          {node.edition}
        </div>
      </td>
      <td>
        <StatusBadge tone={statusTone(node.status)}>{t(`nodes.estado.${node.status}`, { defaultValue: node.status })}</StatusBadge>
      </td>
      <td className="dx-num">{node.version}</td>
      <td className="dx-num">{node.load === null ? '—' : `${Math.round(node.load * 100)}%`}</td>
      <td className="dx-num">{node.rooms}</td>
      <td className="dx-num">{node.peers}</td>
      <td className="dx-num">{new Date(node.last_seen_at).toLocaleString()}</td>
    </tr>
  )
}
