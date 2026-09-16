import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser, Recording } from '../api'
import { Icon } from '../ui/icons'
import { Avatar, Button, IconButton, Select, Tag, TextInput, Toggle, cx } from '../ui/kit'
import { Countdown } from './Clocks'
import { SpeakingBars } from './ParticipantTile'
import { ligacaoFraca } from './qosAmostra'
import type { Breakouts } from './useBreakouts'
import type { Participants } from './useParticipants'
import type { RemotePeer } from './useRoomCore'
import type { QosReport } from '../webrtc'

const BREAKOUT_MINUTES = [0, 5, 10, 15, 20, 30, 45, 60]

/** Salas paralelas (formação): criar, renomear, mover, visitar, encerrar. */
function BreakoutsBlock({ code, breakouts }: { code: string; breakouts: Breakouts }) {
  const { t } = useTranslation()
  return (
    <section className="rm-block" aria-labelledby="rm-bo-h">
      <h3 id="rm-bo-h" className="rm-block__title">
        <Icon name="grid" size={13} />
        {t('room.paralelas.titulo')}
        <span className="dx-spacer" />
        {breakouts.rooms.length > 0 && <span className="dx-num dx-muted">{t('room.paralelas.salas', { count: breakouts.rooms.length })}</span>}
      </h3>
      {breakouts.rooms.length === 0 ? (
        <>
          <p className="dx-muted">{t('room.paralelas.dividir')}</p>
          <div className="rm-block__row">
            {[2, 3, 4].map((n) => (
              <Button key={n} size="sm" variant="outline" onClick={() => breakouts.create(n)}>
                {t('room.paralelas.grupos', { count: n })}
              </Button>
            ))}
          </div>
          <label className="rm-block__row">
            <span className="dx-muted">{t('room.paralelas.duracao')}</span>
            <Select value={breakouts.minutes} onChange={(e) => breakouts.setMinutes(Number(e.target.value))}>
              {BREAKOUT_MINUTES.map((m) => (
                <option key={m} value={m}>
                  {m === 0 ? t('room.paralelas.semLimite') : t('room.temporizador.minutos', { n: m })}
                </option>
              ))}
            </Select>
          </label>
          <p className="dx-muted">{t('room.paralelas.noFimVoltam')}</p>
        </>
      ) : (
        <>
          {breakouts.endsAt && (
            <p className="rm-timer">
              <Icon name="clock" size={13} />
              <span>{t('room.paralelas.terminaEm')}</span>
              <Countdown endsAt={breakouts.endsAt} render={(txt) => <strong className="dx-num">{txt}</strong>} />
            </p>
          )}
          {breakouts.rooms.map((b) => (
            <div key={b.code} className="rm-bo">
              <div className="rm-bo__head">
                <TextInput
                  className="rm-bo__name"
                  defaultValue={b.label}
                  maxLength={60}
                  aria-label={t('room.paralelas.renomear')}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
                  }}
                  onBlur={(e) => {
                    const label = e.target.value.trim()
                    if (label && label !== b.label) breakouts.rename(b.code, label)
                  }}
                />
                <span className="dx-num dx-muted">{t('room.paralelas.pessoas', { count: b.people.length })}</span>
                <Button size="sm" variant="outline" onClick={() => breakouts.visit(b.code)}>
                  {t('room.paralelas.entrar')}
                </Button>
              </div>
              {b.people.length === 0 && <p className="dx-muted">{t('room.paralelas.vazia')}</p>}
              {b.people.map((name) => (
                <div key={name} className="rm-bo__person">
                  <Avatar name={name} size={20} />
                  <span className="rm-bo__pname">{name}</span>
                  <Select
                    aria-label={t('room.paralelas.moverPessoa', { nome: name })}
                    value={b.code}
                    onChange={(e) => breakouts.moveUser(name, e.target.value)}
                  >
                    {breakouts.rooms.map((o) => (
                      <option key={o.code} value={o.code}>
                        {o.label}
                      </option>
                    ))}
                    <option value={code}>{t('room.paralelas.principal')}</option>
                  </Select>
                </div>
              ))}
            </div>
          ))}
          <div className="rm-block__row">
            <Button size="sm" variant="outline" icon="plus" onClick={breakouts.add}>
              {t('room.paralelas.novaSala')}
            </Button>
            <Button size="sm" variant="primary" onClick={breakouts.closeAll}>
              {t('room.paralelas.encerrar')}
            </Button>
          </div>
        </>
      )}
    </section>
  )
}

export function PeoplePanel({
  code,
  isHost,
  canAdmit,
  isTraining,
  peers,
  speaking,
  micOn,
  qos,
  participants,
  chatOn,
  onChatOpenForAll,
  hostShareOnly,
  onHostShareOnly,
  sharePerms,
  onGrantShare,
  onTransferHost,
  breakouts,
  recordings,
  onDownload,
  onInvite,
}: {
  code: string
  isHost: boolean
  canAdmit: boolean
  isTraining: boolean
  peers: RemotePeer[]
  speaking: Set<string>
  micOn: boolean
  qos: QosReport | null
  participants: Participants
  chatOn: boolean
  onChatOpenForAll: (on: boolean) => void
  hostShareOnly: boolean
  onHostShareOnly: (on: boolean) => void
  sharePerms: Set<string>
  onGrantShare: (peerId: string, allowed: boolean) => void
  onTransferHost: (peer: RemotePeer) => void
  breakouts: Breakouts
  recordings: Recording[]
  onDownload: (r: Recording) => void
  onInvite: () => void
}) {
  const { t, i18n } = useTranslation()
  const [search, setSearch] = useState('')
  const me = currentUser()?.username ?? ''
  const q = search.trim().toLowerCase()
  const lista = peers.filter((p) => !q || p.username.toLowerCase().includes(q))
  const maos = peers.filter((p) => p.hand).length

  return (
    <div className="rm-scroll">
      <div className="rm-block__row">
        <Button size="sm" variant="primary" icon="userPlus" onClick={onInvite}>
          {t('room.pessoas.convidar')}
        </Button>
        {isHost && (
          <Button size="sm" variant="outline" icon="micOff" onClick={() => participants.muteAll(true)}>
            {t('room.pessoas.silenciarTodos')}
          </Button>
        )}
      </div>
      {isHost && (
        <div className="rm-block__row">
          <Button size="sm" variant="ghost" icon="ban" onClick={() => participants.muteAll(false)}>
            {t('room.pessoas.silenciarSemVolta')}
          </Button>
          <Button size="sm" variant="ghost" icon="chat" onClick={() => onChatOpenForAll(!chatOn)}>
            {chatOn ? t('room.pessoas.fecharChat') : t('room.pessoas.reabrirChat')}
          </Button>
        </div>
      )}

      {canAdmit && participants.waitingQueue.length > 0 && (
        <section className="rm-block rm-block--accent" aria-labelledby="rm-wait-h">
          <h3 id="rm-wait-h" className="rm-block__title">
            <Icon name="door" size={13} />
            {t('room.avisos.salaDeEspera')}
            <span className="dx-spacer" />
            <span className="dx-num dx-muted">{t('room.pessoas.aAguardar', { count: participants.waitingQueue.length })}</span>
          </h3>
          {participants.waitingQueue.map((p) => (
            <div key={p.peer_id} className="rm-person">
              <Avatar name={p.username} size={28} />
              <span className="rm-person__name">
                <strong>{p.username}</strong>
                {p.is_pstn && <small className="dx-muted">{t('room.papel.telefone')}</small>}
              </span>
              <Button size="sm" variant="primary" onClick={() => participants.admit(p.peer_id, true)}>
                {t('room.avisos.admitir')}
              </Button>
              <Button size="sm" variant="outline" onClick={() => participants.admit(p.peer_id, false)}>
                {t('room.avisos.negar')}
              </Button>
            </div>
          ))}
          {participants.waitingQueue.length > 1 && (
            <Button size="sm" variant="outline" block onClick={participants.admitAll}>
              {t('room.avisos.admitirTodos', { count: participants.waitingQueue.length })}
            </Button>
          )}
        </section>
      )}

      <section className="rm-block" aria-labelledby="rm-people-h">
        <h3 id="rm-people-h" className="rm-block__title">
          <Icon name="people" size={13} />
          {t('room.pessoas.naSala', { count: peers.length + 1 })}
          <span className="dx-spacer" />
          {maos > 0 && (
            <Tag tone="live">
              <Icon name="hand" size={10} />
              {t('room.pessoas.maosNoAr', { count: maos })}
            </Tag>
          )}
        </h3>
        <label className="rm-search">
          <Icon name="search" size={13} />
          <input type="search" value={search} onChange={(e) => setSearch(e.target.value)} placeholder={t('room.pessoas.pesquisar')} aria-label={t('room.pessoas.pesquisar')} />
        </label>

        <div className="rm-person">
          <Avatar name={me} size={28} />
          <span className="rm-person__name">
            <strong>{me ? t('room.tile.nomeTu', { nome: me }) : t('room.tile.tu')}</strong>
            {qos && (
              <small className="dx-num dx-muted" title={t('room.pessoas.qualidadeDica', { score: qos.score })}>
                {t('room.pessoas.qualidadePropria', { score: qos.score, up: qos.upKbps })}
                {qos.rttMs != null && ` · ${t('room.pessoas.rtt', { ms: qos.rttMs })}`}
                {qos.turnRelay && ` · ${t('room.pessoas.viaRelay')}`}
              </small>
            )}
          </span>
          {isHost && <Tag tone="accent">{t('room.papel.anfitriao')}</Tag>}
          {micOn ? speaking.has('me') ? <SpeakingBars /> : <Icon name="mic" size={13} /> : <Icon name="micOff" size={13} className="dx-icon rm-tile__muted" />}
        </div>

        {lista.map((p) => {
          const pq = qos?.byPeer[p.peerId]
          return (
            <div key={p.peerId} className={cx('rm-person', p.reconnecting && 'is-reconnecting')}>
              <Avatar name={p.username} size={28} />
              <span className="rm-person__name">
                <strong>{p.username}</strong>
                {pq && (
                  <small className={cx('dx-num', ligacaoFraca(pq.lossPct) ? 'rm-bad' : 'dx-muted')}>
                    {t('room.pessoas.qualidadePar', { kbps: pq.kbps, perda: pq.lossPct })}
                    {pq.jitterMs > 30 && ` · ${t('room.pessoas.jitter', { ms: pq.jitterMs })}`}
                    {pq.freezeMs > 0 && ` · ${t('room.pessoas.congelado', { ms: Math.round(pq.freezeMs) })}`}
                  </small>
                )}
              </span>
              {p.hand && (
                <Tag tone="live">
                  <Icon name="hand" size={10} />
                  {t('room.tile.mao')}
                </Tag>
              )}
              {p.host ? (
                <Tag tone="accent">{t('room.papel.anfitriao')}</Tag>
              ) : p.canAdmit ? (
                <span title={t('room.papel.coAnfitriaoDica')}>
                  <Tag>{t('room.papel.coAnfitriao')}</Tag>
                </span>
              ) : p.is_pstn ? (
                <Tag>{t('room.papel.telefone')}</Tag>
              ) : p.is_bot ? (
                <Tag>{t('room.papel.assistente')}</Tag>
              ) : null}
              {speaking.has(p.peerId) ? <SpeakingBars /> : p.micOn ? <Icon name="mic" size={13} /> : <Icon name="micOff" size={13} className="dx-icon rm-tile__muted" />}
              {isHost && !p.host && (
                <div className="rm-person__actions" role="group" aria-label={t('room.pessoas.accoesSobre', { nome: p.username })}>
                  <IconButton icon="micOff" label={t('room.tile.silenciar', { nome: p.username })} onClick={() => participants.mute(p.peerId)} />
                  <IconButton icon="videoOff" label={t('room.pessoas.desligarCamara', { nome: p.username })} onClick={() => participants.camOff(p.peerId)} />
                  <IconButton
                    icon="screen"
                    label={sharePerms.has(p.peerId) ? t('room.pessoas.retirarPartilha', { nome: p.username }) : t('room.pessoas.permitirPartilha', { nome: p.username })}
                    aria-pressed={sharePerms.has(p.peerId)}
                    onClick={() => onGrantShare(p.peerId, !sharePerms.has(p.peerId))}
                  />
                  <IconButton
                    icon="door"
                    label={p.canAdmit ? t('room.pessoas.retirarAdmissao', { nome: p.username }) : t('room.pessoas.permitirAdmissao', { nome: p.username })}
                    aria-pressed={p.canAdmit}
                    onClick={() => participants.promoteAdmit(p.peerId, !p.canAdmit)}
                  />
                  <IconButton icon="key" label={t('room.pessoas.passarAnfitriao', { nome: p.username })} onClick={() => onTransferHost(p)} />
                  <IconButton icon="x" label={t('room.tile.remover', { nome: p.username })} onClick={() => participants.kick(p.peerId)} />
                </div>
              )}
            </div>
          )
        })}
        {q && lista.length === 0 && <p className="dx-muted">{t('room.pessoas.ninguem')}</p>}
      </section>

      {isHost && (
        <section className="rm-block" aria-labelledby="rm-host-h">
          <h3 id="rm-host-h" className="rm-block__title">
            <Icon name="shield" size={13} />
            {t('room.pessoas.controlosAnfitriao')}
          </h3>
          <Toggle
            label={t('room.pessoas.bloquear')}
            hint={t('room.pessoas.bloquearDica')}
            checked={participants.roomLocked}
            onChange={(e) => participants.setLocked(e.target.checked)}
          />
          <Toggle
            label={t('room.pessoas.soAnfitriaoPartilha')}
            hint={t('room.pessoas.soAnfitriaoPartilhaDica')}
            checked={hostShareOnly}
            onChange={(e) => onHostShareOnly(e.target.checked)}
          />
        </section>
      )}

      {isHost && isTraining && <BreakoutsBlock code={code} breakouts={breakouts} />}

      <section className="rm-block" aria-labelledby="rm-recs-h">
        <h3 id="rm-recs-h" className="rm-block__title">
          <Icon name="film" size={13} />
          {t('room.pessoas.gravacoes')}
        </h3>
        {recordings.length === 0 && <p className="dx-muted">{t('room.pessoas.semGravacoes')}</p>}
        {recordings.map((r) => (
          <button key={r.id} type="button" className="rm-rec" onClick={() => onDownload(r)}>
            <Icon name="download" size={14} />
            <span className="rm-rec__text">
              <span>{r.filename}</span>
              <small className="dx-num dx-muted">
                {new Date(r.created_at).toLocaleString(i18n.language)} · {t('room.pessoas.megabytes', { n: (r.size_bytes / 1_048_576).toFixed(1) })}
              </small>
            </span>
          </button>
        ))}
      </section>
    </div>
  )
}
