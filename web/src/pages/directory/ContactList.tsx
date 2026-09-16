/**
 * Coluna de contactos: pesquisa, filtros, separadores Pessoas · Grupos ·
 * Perdidas, e a lista com presença. Ligar de uma linha é o atalho; o detalhe
 * tem as duas opções (vídeo e voz).
 */
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { Branch, Employee, Group } from '../../api'
import type { MissedCall } from '../../presence'
import { Icon } from '../../ui/icons'
import { Avatar, cx, IconButton, Select, Tabs } from '../../ui/kit'
import { formatAgo, useLocaleTag } from '../admin/orgShared'

export type DirTab = 'people' | 'groups' | 'missed' | 'phone'
export type Selection = { kind: 'person'; id: string } | { kind: 'group'; id: string } | { kind: 'org' } | null

export default function ContactList({
  tab,
  onTab,
  q,
  onQ,
  branchFilter,
  onBranchFilter,
  branches,
  people,
  groups,
  missed,
  meId,
  isOnline,
  onlineCount,
  selection,
  onSelect,
  onCallPerson,
  onCallGroup,
  onCallBack,
  onAckMissed,
  onNewGroup,
  pending,
  phoneHistory,
}: {
  tab: DirTab
  onTab: (t: DirTab) => void
  q: string
  onQ: (q: string) => void
  branchFilter: string
  onBranchFilter: (id: string) => void
  branches: Branch[]
  people: Employee[] | null
  groups: Group[] | null
  missed: MissedCall[]
  meId: string
  isOnline: (id: string) => boolean
  onlineCount: number
  selection: Selection
  onSelect: (s: Selection) => void
  onCallPerson: (p: Employee, kind: 'video' | 'voice') => void
  onCallGroup: (g: Group, kind: 'video' | 'voice') => void
  onCallBack: (m: MissedCall) => void
  onAckMissed: () => void
  onNewGroup: () => void
  /** O que mostrar no lugar da lista enquanto carrega ou quando falhou. */
  pending: ReactNode
  /** Histórico PSTN (só para quem administra; sem ele o separador não aparece). */
  phoneHistory?: ReactNode
}) {
  const { t } = useTranslation()
  const locale = useLocaleTag()

  return (
    <aside className="org-dir__list" aria-label={t('org.dir.lista')}>
      <div className="org-dir__head">
        {(tab === 'people' || tab === 'groups') && (
          <div className="org-search">
            <Icon name="search" />
            <input
              type="search"
              value={q}
              onChange={(e) => onQ(e.target.value)}
              placeholder={tab === 'groups' ? t('org.dir.pesquisarGrupos') : t('org.dir.pesquisar')}
              aria-label={tab === 'groups' ? t('org.dir.pesquisarGrupos') : t('org.dir.pesquisar')}
            />
          </div>
        )}
        {tab === 'people' && branches.length > 0 && (
          <Select value={branchFilter} onChange={(e) => onBranchFilter(e.target.value)} aria-label={t('org.dir.filtrarFilial')}>
            <option value="">{t('org.dir.todasFiliais')}</option>
            {branches.map((b) => (
              <option key={b.id} value={b.id}>
                {b.name}
              </option>
            ))}
          </Select>
        )}
        <Tabs
          label={t('org.dir.separadores')}
          value={tab}
          onChange={onTab}
          tabs={[
            { value: 'people', label: t('org.dir.pessoas') },
            { value: 'groups', label: t('org.dir.grupos') },
            { value: 'missed', label: t('org.dir.perdidas'), count: missed.length },
            ...(phoneHistory ? [{ value: 'phone' as const, label: t('consola.contactos.telefone') }] : []),
          ]}
        />
      </div>

      <div className="org-dir__scroll">
        {tab === 'people' && (
          <>
            <div className="org-dir__sub dx-eyebrow">
              <span>{people ? t('org.dir.pessoasContagem', { count: people.length }) : ''}</span>
              <span className="dx-spacer" />
              <span>
                <span className="org-dot org-dot--on" aria-hidden="true" /> {t('org.dir.onlineContagem', { count: onlineCount })}
              </span>
            </div>
            {people === null && pending}
            {people && people.length === 0 && <p className="org-dir__empty dx-muted">{t('ui.semResultados')}</p>}
            <ul className="org-rows">
              {people?.map((p) => {
                const on = isOnline(p.user_id)
                const me = p.user_id === meId
                const active = selection?.kind === 'person' && selection.id === p.user_id
                return (
                  <li key={p.user_id} className={cx('org-row', active && 'org-row--active')}>
                    <button
                      type="button"
                      className="org-row__main"
                      aria-current={active || undefined}
                      onClick={() => onSelect({ kind: 'person', id: p.user_id })}
                    >
                      <span className="org-av">
                        <Avatar name={p.username} size={32} />
                        <span className={cx('org-dot', on && 'org-dot--on')} aria-hidden="true" />
                      </span>
                      <span className="org-row__text">
                        <strong>
                          {p.username}
                          {me && <span className="dx-muted"> {t('org.dir.tu')}</span>}
                        </strong>
                        <span className="org-row__meta dx-num">
                          {[p.title, on ? t('org.presenca.online') : t('org.presenca.offline')].filter(Boolean).join(' · ')}
                        </span>
                      </span>
                    </button>
                    {!me && (
                      <IconButton
                        icon="phone"
                        bare
                        label={t('org.dir.ligarVozA', { nome: p.username })}
                        onClick={() => onCallPerson(p, 'voice')}
                      />
                    )}
                  </li>
                )
              })}
            </ul>
          </>
        )}

        {tab === 'groups' && (
          <>
            <div className="org-dir__sub">
              <span className="dx-eyebrow">{groups ? t('org.dir.gruposContagem', { count: groups.length }) : ''}</span>
              <span className="dx-spacer" />
              <button type="button" className="dx-btn dx-btn--ghost dx-btn--sm" onClick={onNewGroup}>
                <Icon name="plus" />
                {t('org.grupo.novo')}
              </button>
            </div>
            {groups === null && pending}
            {groups && groups.length === 0 && <p className="org-dir__empty dx-muted">{t('org.dir.semGrupos')}</p>}
            <ul className="org-rows">
              {groups?.map((g) => {
                const active = selection?.kind === 'group' && selection.id === g.id
                return (
                  <li key={g.id} className={cx('org-row', active && 'org-row--active')}>
                    <button
                      type="button"
                      className="org-row__main"
                      aria-current={active || undefined}
                      onClick={() => onSelect({ kind: 'group', id: g.id })}
                    >
                      <span className="org-av org-av--group" aria-hidden="true">
                        <Icon name="people" />
                      </span>
                      <span className="org-row__text">
                        <strong>{g.name}</strong>
                        <span className="org-row__meta dx-num">{t('org.membrosContagem', { count: g.member_count })}</span>
                      </span>
                    </button>
                    <IconButton icon="video" bare label={t('org.dir.ligarGrupoVideo', { nome: g.name })} onClick={() => onCallGroup(g, 'video')} />
                  </li>
                )
              })}
            </ul>
          </>
        )}

        {tab === 'missed' && (
          <>
            <div className="org-dir__sub">
              <span className="dx-eyebrow">{t('org.dir.perdidasTitulo')}</span>
              <span className="dx-spacer" />
              {missed.length > 0 && (
                <button type="button" className="dx-btn dx-btn--ghost dx-btn--sm" onClick={onAckMissed}>
                  <Icon name="check" />
                  {t('org.dir.marcarVistas')}
                </button>
              )}
            </div>
            {missed.length === 0 && <p className="org-dir__empty dx-muted">{t('org.dir.semPerdidas')}</p>}
            <ul className="org-rows">
              {missed.map((m) => (
                <li key={m.id} className="org-row">
                  <div className="org-row__main org-row__main--static">
                    <span className="org-av org-av--missed" aria-hidden="true">
                      <Icon name={m.kind === 'voice' ? 'phone' : 'video'} />
                    </span>
                    <span className="org-row__text">
                      <strong>{m.caller_name}</strong>
                      <span className="org-row__meta dx-num">
                        {[m.kind === 'voice' ? t('org.dir.perdidaVoz') : t('org.dir.perdidaVideo'), formatAgo(m.created_at, locale)]
                          .filter(Boolean)
                          .join(' · ')}
                      </span>
                    </span>
                  </div>
                  <IconButton icon="refresh" bare label={t('org.dir.devolverA', { nome: m.caller_name })} onClick={() => onCallBack(m)} />
                </li>
              ))}
            </ul>
          </>
        )}
        {tab === 'phone' && phoneHistory}
      </div>

      <div className="org-dir__foot">
        <button
          type="button"
          className={cx('org-dir__orgbtn', selection?.kind === 'org' && 'org-dir__orgbtn--active')}
          onClick={() => onSelect({ kind: 'org' })}
        >
          <Icon name="building" />
          <span>{t('org.dir.filiaisESalas')}</span>
          <Icon name="chevronRight" />
        </button>
      </div>
    </aside>
  )
}
