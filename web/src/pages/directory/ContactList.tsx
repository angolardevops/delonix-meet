/**
 * Coluna de contactos das Chamadas (296 px, como no DelonixCall): marca e
 * título, pesquisa, separadores Contactos · Grupos · Histórico e a lista com
 * presença. Cada linha tem os atalhos voz, vídeo e — quando o servidor diz que
 * a pessoa recebe e a org deixa enviar — SMS; o centro tem as mesmas três.
 *
 * O separador «Teclado» e a marcação PSTN do template não existem aqui: não há
 * chamadas de saída no servidor. No lugar do teclado ficam os grupos, que
 * existem e se ligam (a quem estiver online).
 */
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { Branch, Employee, Group } from '../../api'
import type { MissedCall } from '../../presence'
import { DelonixSymbol, Icon } from '../../ui/icons'
import { Avatar, cx, IconButton, Select } from '../../ui/kit'
import CallHistory from './CallHistory'

export type DirTab = 'people' | 'groups' | 'history'
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
  focus,
  onSelect,
  onCallPerson,
  smsFor,
  onCallGroup,
  onCallBack,
  onAckMissed,
  onNewGroup,
  orgId,
  isAdmin,
  orgPicker,
  pending,
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
  /** O que está em foco no centro (escolhido ou o primeiro da lista). */
  focus: Selection
  onSelect: (s: Selection) => void
  onCallPerson: (p: Employee, kind: 'video' | 'voice') => void
  /** Abre o SMS a esta pessoa; `undefined` quando não se pode (sem número, recusou, política). */
  smsFor: (p: Employee) => (() => void) | undefined
  onCallGroup: (g: Group, kind: 'video' | 'voice') => void
  onCallBack: (m: MissedCall) => void
  onAckMissed: () => void
  onNewGroup: () => void
  orgId: string
  isAdmin: boolean
  orgPicker: ReactNode
  /** O que mostrar no lugar da lista enquanto carrega ou quando falhou. */
  pending: ReactNode
}) {
  const { t } = useTranslation()
  const tabs: { value: DirTab; label: string; count?: number }[] = [
    { value: 'people', label: t('consola.chamadas.contactos') },
    { value: 'groups', label: t('org.dir.grupos') },
    { value: 'history', label: t('consola.contactos.historico'), count: missed.length },
  ]

  return (
    <aside className="call-list" aria-label={t('org.dir.lista')}>
      <div className="call-list__head">
        <div className="call-brand">
          <span className="call-brand__mark" aria-hidden="true">
            <DelonixSymbol size={18} />
          </span>
          <h2>{t('consola.chamadas.titulo')}</h2>
          <span className="dx-spacer" />
          <button
            type="button"
            className={cx('call-sq', focus?.kind === 'org' && 'call-sq--on')}
            aria-label={t('org.dir.filiaisESalas')}
            title={t('org.dir.filiaisESalas')}
            aria-pressed={focus?.kind === 'org'}
            onClick={() => onSelect({ kind: 'org' })}
          >
            <Icon name="building" size={13} />
          </button>
        </div>
        {orgPicker}
        {tab !== 'history' && (
          <label className="call-search">
            <Icon name="search" size={12} />
            <input
              type="search"
              value={q}
              onChange={(e) => onQ(e.target.value)}
              placeholder={tab === 'groups' ? t('org.dir.pesquisarGrupos') : t('consola.contactos.pesquisar')}
              aria-label={tab === 'groups' ? t('org.dir.pesquisarGrupos') : t('org.dir.pesquisar')}
            />
          </label>
        )}
        <div className="call-tabs" role="tablist" aria-label={t('org.dir.separadores')}>
          {tabs.map((x) => (
            <button
              key={x.value}
              type="button"
              role="tab"
              aria-selected={tab === x.value}
              className={cx('call-tab', tab === x.value && 'call-tab--on')}
              onClick={() => onTab(x.value)}
            >
              {x.label}
              {!!x.count && <span className="call-tab__count dx-num">{x.count}</span>}
            </button>
          ))}
        </div>
        {tab === 'people' && branches.length > 1 && (
          <Select value={branchFilter} onChange={(e) => onBranchFilter(e.target.value)} aria-label={t('org.dir.filtrarFilial')}>
            <option value="">{t('org.dir.todasFiliais')}</option>
            {branches.map((b) => (
              <option key={b.id} value={b.id}>
                {b.name}
              </option>
            ))}
          </Select>
        )}
      </div>

      <div className="call-list__scroll">
        {tab === 'people' && (
          <>
            {people === null && pending}
            {people && people.length === 0 && <p className="call-empty">{t('ui.semResultados')}</p>}
            <ul className="call-rows" role="list">
              {people?.map((p) => {
                const on = isOnline(p.user_id)
                const me = p.user_id === meId
                const active = focus?.kind === 'person' && focus.id === p.user_id
                const sms = me ? undefined : smsFor(p)
                return (
                  <li key={p.user_id} className={cx('call-row', active && 'call-row--active')}>
                    <button
                      type="button"
                      className="call-row__main"
                      aria-current={active || undefined}
                      onClick={() => onSelect({ kind: 'person', id: p.user_id })}
                    >
                      <span className="call-av">
                        <Avatar name={p.username} size={30} />
                        <span className={cx('call-dot', on && 'call-dot--on')} aria-hidden="true" />
                      </span>
                      <span className="call-row__text">
                        <strong>
                          {p.username}
                          {me && <span className="call-muted"> {t('org.dir.tu')}</span>}
                        </strong>
                        <span className="call-row__meta dx-num">
                          {[p.title, on ? t('consola.chamadas.disponivel') : t('consola.chamadas.offline')].filter(Boolean).join(' · ')}
                        </span>
                      </span>
                    </button>
                    {!me && (
                      <span className="call-rowbtns">
                        <IconButton icon="phone" bare className="call-rowbtn" label={t('org.dir.ligarVozA', { nome: p.username })} onClick={() => onCallPerson(p, 'voice')} />
                        <IconButton icon="video" bare className="call-rowbtn" label={t('org.dir.ligarVideoA', { nome: p.username })} onClick={() => onCallPerson(p, 'video')} />
                        {sms && <IconButton icon="sms" bare className="call-rowbtn" label={t('org.sms.enviarA', { nome: p.username })} onClick={sms} />}
                      </span>
                    )}
                  </li>
                )
              })}
            </ul>
          </>
        )}

        {tab === 'groups' && (
          <>
            {groups === null && pending}
            {groups && groups.length === 0 && <p className="call-empty">{t('org.dir.semGrupos')}</p>}
            <ul className="call-rows" role="list">
              {groups?.map((g) => {
                const active = focus?.kind === 'group' && focus.id === g.id
                return (
                  <li key={g.id} className={cx('call-row', active && 'call-row--active')}>
                    <button
                      type="button"
                      className="call-row__main"
                      aria-current={active || undefined}
                      onClick={() => onSelect({ kind: 'group', id: g.id })}
                    >
                      <span className="call-av call-av--icon" aria-hidden="true">
                        <Icon name="people" size={14} />
                      </span>
                      <span className="call-row__text">
                        <strong>{g.name}</strong>
                        <span className="call-row__meta dx-num">{t('org.membrosContagem', { count: g.member_count })}</span>
                      </span>
                    </button>
                    <IconButton icon="video" bare className="call-rowbtn" label={t('org.dir.ligarGrupoVideo', { nome: g.name })} onClick={() => onCallGroup(g, 'video')} />
                  </li>
                )
              })}
            </ul>
            <button type="button" className="call-addrow" onClick={onNewGroup}>
              <Icon name="plus" size={12} />
              {t('org.grupo.novo')}
            </button>
          </>
        )}

        {tab === 'history' && (
          <>
            <div className="call-sub">
              <span className="call-eyebrow">{t('org.dir.perdidasTitulo')}</span>
              <span className="dx-spacer" />
              {missed.length > 0 && (
                <button type="button" className="call-link" onClick={onAckMissed}>
                  <Icon name="check" size={11} />
                  {t('org.dir.marcarVistas')}
                </button>
              )}
            </div>
            <CallHistory orgId={orgId} isAdmin={isAdmin} missed={missed} onCallBack={onCallBack} />
          </>
        )}
      </div>
    </aside>
  )
}
