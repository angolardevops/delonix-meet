/**
 * Centro e coluna direita das Chamadas, na grelha do DelonixCall: barra de
 * 50 px, palco com o contacto em foco e a barra de controlos por baixo, e à
 * direita (280 px) «Acções de chamada», «Histórico · 7 dias» e o dial-in.
 *
 * O palco é o do contacto, não o de uma chamada: a chamada acontece na sala.
 * Por isso não há cronómetro, «A GRAVAR», resolução nem latência aqui — só o
 * que o servidor sabe da pessoa (cargo, filial, email, presença, última
 * actividade). «Retenção», «Transferir» e «Teclado DTMF» não existem no
 * servidor e não aparecem; as quatro acções são as que existem.
 */
import type { CSSProperties, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Branch, Employee, Group, listVoiceDids, MeetingRoom } from '../../api'
import { Async, AsyncSection, useAsync } from '../../components/AsyncSection'
import type { MissedCall } from '../../presence'
import { Icon, IconName } from '../../ui/icons'
import { avatarTone, cx, IconButton, initials } from '../../ui/kit'
import { formatAgo, refusalAware, useLocaleTag } from '../admin/orgShared'
import { calendarHash } from '../calendar/dates'
import CallHistory from './CallHistory'
import OrgOverview from './OrgOverview'

type Side = {
  orgId: string
  isAdmin: boolean
  missed: MissedCall[]
  onCallBack: (m: MissedCall) => void
  onNewGroup: () => void
  /** Presente quando o foco foi escolhido: em ecrã estreito volta à lista. */
  onBack?: () => void
}

function TopBar({ title, chip, action, onBack }: { title: ReactNode; chip?: ReactNode; action?: ReactNode; onBack?: () => void }) {
  const { t } = useTranslation()
  return (
    <div className="call-top">
      {onBack && <IconButton icon="chevronLeft" bare className="call-top__back" label={t('ui.voltar')} onClick={onBack} />}
      <h2 className="call-top__title">{title}</h2>
      {chip && <span className="call-chip dx-num">{chip}</span>}
      <span className="dx-spacer" />
      {action}
    </div>
  )
}

function ActionTile({ icon, label, onClick }: { icon: IconName; label: string; onClick: () => void }) {
  return (
    <button type="button" className="call-act" onClick={onClick}>
      <Icon name={icon} size={12} />
      {label}
    </button>
  )
}

function DialIn({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const dids = useAsync((signal) => refusalAware(listVoiceDids(orgId, signal), t), [orgId])
  return (
    <section className="call-card call-card--foot" aria-labelledby="call-dialin">
      <h3 id="call-dialin">{t('consola.chamadas.dialIn')}</h3>
      <AsyncSection state={dids.state} onRetry={dids.reload}>
        {(rows) => {
          const active = rows.filter((d) => d.active)
          return (
            <div className="call-mono">
              {active.length === 0 ? (
                <span>{t('consola.chamadas.semNumeros')}</span>
              ) : (
                active.slice(0, 2).map((d) => <span key={d.id}>{d.e164}</span>)
              )}
              <span>{t('consola.chamadas.soEntrada')}</span>
            </div>
          )
        }}
      </AsyncSection>
    </section>
  )
}

function SideColumn({ side, actions, target }: { side: Side; actions: ReactNode; target?: ReactNode }) {
  const { t } = useTranslation()
  return (
    <div className="call-side">
      <section className="call-card" aria-labelledby="call-accoes">
        <h3 id="call-accoes">{t('consola.chamadas.accoes')}</h3>
        <div className="call-acts">{actions}</div>
        {target && <div className="call-target">{target}</div>}
      </section>
      <section className="call-card call-card--grow" aria-labelledby="call-historico">
        <div className="call-card__head">
          <h3 id="call-historico">{t('consola.contactos.historico')}</h3>
          <span className="call-eyebrow dx-num">{t('consola.chamadas.seteDias')}</span>
        </div>
        <CallHistory orgId={side.orgId} isAdmin={side.isAdmin} missed={side.missed} onCallBack={side.onCallBack} days={7} limit={6} compact />
      </section>
      {side.isAdmin && <DialIn orgId={side.orgId} />}
    </div>
  )
}

function Controls({ onCall, video, voice }: { onCall: (k: 'video' | 'voice') => void; video: string; voice: string }) {
  return (
    <div className="call-controls" role="group">
      <button type="button" className="call-ctl call-ctl--primary" onClick={() => onCall('video')}>
        <span className="call-ctl__btn" aria-hidden="true">
          <Icon name="video" size={17} />
        </span>
        <span>{video}</span>
      </button>
      <button type="button" className="call-ctl" onClick={() => onCall('voice')}>
        <span className="call-ctl__btn" aria-hidden="true">
          <Icon name="phone" size={17} />
        </span>
        <span>{voice}</span>
      </button>
    </div>
  )
}

function commonActions(side: Side, t: (k: string) => string) {
  return (
    <>
      <ActionTile icon="people" label={t('org.grupo.novo')} onClick={side.onNewGroup} />
      <ActionTile icon="calendar" label={t('consola.chamadas.agendar')} onClick={() => (location.hash = calendarHash.schedule())} />
    </>
  )
}

export default function CallStage({
  person,
  me,
  online,
  onCall,
  ...side
}: Side & { person: Employee; me: boolean; online: boolean; onCall: (k: 'video' | 'voice') => void }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const ago = formatAgo(person.last_active, locale)
  const tone = { '--call-tone': avatarTone(person.username) } as CSSProperties
  const presence = online ? t('consola.chamadas.disponivel') : t('org.presenca.offline')
  return (
    <>
      <TopBar
        title={person.username}
        chip={presence}
        onBack={side.onBack}
        action={
          !me && (
            <button type="button" className="call-escalate" onClick={() => onCall('video')}>
              {t('org.dir.videochamada')}
            </button>
          )
        }
      />
      <div className="call-grid">
        <div className="call-center">
          <div className={cx('call-stage', online && 'call-stage--on')} style={tone} data-testid="call-stage">
            <div className="call-stage__face" aria-hidden="true">
              {initials(person.username)}
            </div>
            <div className="call-stage__chips call-stage__chips--top">
              <span className="call-pill call-pill--strong">
                {person.username}
                <span className={cx('call-dot call-dot--inline', online && 'call-dot--on')} aria-hidden="true" />
              </span>
              {(person.title || person.branch_name) && (
                <span className="call-pill dx-num">{[person.title, person.branch_name].filter(Boolean).join(' · ')}</span>
              )}
            </div>
            <div className="call-stage__chips call-stage__chips--bottom">
              <span className="call-pill dx-num">{person.email}</span>
              {ago && <span className={cx('call-pill dx-num', online && 'call-pill--ok')}>{t('consola.chamadas.activo', { quando: ago })}</span>}
            </div>
          </div>
          {me ? (
            <p className="call-note">{t('org.dir.tuMesmo')}</p>
          ) : (
            <>
              <Controls onCall={onCall} video={t('org.dir.videochamada')} voice={t('org.dir.chamadaVoz')} />
              {!online && <p className="call-note">{t('org.dir.offlineNota')}</p>}
            </>
          )}
        </div>
        <SideColumn
          side={side}
          actions={
            <>
              {!me && <ActionTile icon="video" label={t('org.dir.videochamada')} onClick={() => onCall('video')} />}
              {!me && <ActionTile icon="phone" label={t('org.dir.chamadaVoz')} onClick={() => onCall('voice')} />}
              {commonActions(side, t)}
            </>
          }
          target={
            <>
              <span className="call-eyebrow">{t('org.campo.filial')}</span>
              <strong>{person.branch_name || t('consola.chamadas.semFilial')}</strong>
            </>
          }
        />
      </div>
    </>
  )
}

export function GroupStage({ group, onCall, ...side }: Side & { group: Group; onCall: (k: 'video' | 'voice') => void }) {
  const { t } = useTranslation()
  const tone = { '--call-tone': avatarTone(group.name) } as CSSProperties
  return (
    <>
      <TopBar
        title={group.name}
        chip={t('org.membrosContagem', { count: group.member_count })}
        onBack={side.onBack}
        action={
          <button type="button" className="call-escalate" onClick={() => onCall('video')}>
            {t('org.dir.videochamadaGrupo')}
          </button>
        }
      />
      <div className="call-grid">
        <div className="call-center">
          <div className="call-stage" style={tone} data-testid="call-stage">
            <div className="call-stage__face" aria-hidden="true">
              <Icon name="people" size={44} />
            </div>
            <div className="call-stage__chips call-stage__chips--top">
              <span className="call-pill call-pill--strong">{group.name}</span>
              <span className="call-pill dx-num">{t('org.membrosContagem', { count: group.member_count })}</span>
            </div>
          </div>
          <Controls onCall={onCall} video={t('org.dir.videochamadaGrupo')} voice={t('org.dir.chamadaVozGrupo')} />
          <p className="call-note">{t('org.dir.grupoNota')}</p>
        </div>
        <SideColumn
          side={side}
          actions={
            <>
              <ActionTile icon="video" label={t('org.dir.videochamada')} onClick={() => onCall('video')} />
              <ActionTile icon="phone" label={t('org.dir.chamadaVoz')} onClick={() => onCall('voice')} />
              {commonActions(side, t)}
            </>
          }
        />
      </div>
    </>
  )
}

export function OrgStage({
  orgName,
  places,
  onRetry,
  people,
  onManage,
  ...side
}: Side & {
  orgName: string
  places: Async<[Branch[], MeetingRoom[]]>
  onRetry: () => void
  people: Employee[]
  onManage?: () => void
}) {
  const { t } = useTranslation()
  return (
    <>
      <TopBar
        title={orgName}
        chip={t('org.dir.filiaisESalas')}
        onBack={side.onBack}
        action={
          onManage && (
            <button type="button" className="call-escalate" onClick={onManage}>
              {t('org.dir.gerir')}
            </button>
          )
        }
      />
      <div className="call-grid">
        <div className="call-center call-center--scroll">
          <AsyncSection state={places} onRetry={onRetry}>
            {([b, r]) => <OrgOverview branches={b} rooms={r} people={people} />}
          </AsyncSection>
        </div>
        <SideColumn side={side} actions={commonActions(side, t)} />
      </div>
    </>
  )
}
