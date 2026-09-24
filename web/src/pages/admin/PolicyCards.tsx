/**
 * Coluna direita do DelonixAdmin: «Armazenamento» e «Políticas de retenção»,
 * com o que o servidor sabe.
 *  - Armazenamento: o volume das gravações da organização (orgStats). O
 *    template soma também transcrições e processamento contra um tecto de
 *    2,5 TB num MinIO — nenhum desses três números existe; a barra desenha só
 *    o segmento real e sem tecto ocupa a largura toda.
 *  - Retenção: só as gravações de reuniões têm política (organizations.
 *    retention_days, varrida em recorder.rs). Videoaulas, chat e sondagens e
 *    auditoria não têm retenção própria no servidor, e por isso não têm linha.
 */
import { useTranslation } from 'react-i18next'
import type { OrgStats, OrgSummary } from '../../api'
import { Async } from '../../components/AsyncSection'
import { formatBytes, useLocaleTag } from './orgShared'

export function StorageCard({ stats }: { stats: Async<OrgStats> }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const s = stats.s === 'ready' ? stats.d : null
  return (
    <section className="org-pcard" aria-labelledby="org-armazenamento">
      <h2 id="org-armazenamento">{t('consola.admin.armazenamento')}</h2>
      <div className="org-pbar" aria-hidden="true">
        <span style={{ width: s && s.recordings_bytes > 0 ? '100%' : 0 }} />
      </div>
      <div className="org-plegend">
        <span>
          <i aria-hidden="true" />
          {s ? t('consola.admin.gravacoesBytes', { tamanho: formatBytes(s.recordings_bytes, locale) }) : '—'}
        </span>
      </div>
      <div className="dx-num dx-muted org-pnote">
        {s ? t('home.armazenamento.gravacoes', { count: s.recordings_total }) : stats.s === 'error' ? stats.msg : '…'}
      </div>
    </section>
  )
}

export function RetentionCard({ org }: { org: OrgSummary }) {
  const { t } = useTranslation()
  const days = org.retention_days ?? 0
  return (
    <section className="org-pcard" aria-labelledby="org-retencao">
      <h2 id="org-retencao">{t('consola.admin.retencao')}</h2>
      <ul className="org-plist" role="list">
        <li>
          <span>{t('consola.admin.retencaoGravacoes')}</span>
          <strong className="dx-num">{days > 0 ? t('consola.orgs.dias', { count: days }) : t('consola.orgs.semLimite')}</strong>
        </li>
      </ul>
    </section>
  )
}
