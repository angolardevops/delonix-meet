/**
 * Agendar uma reunião — o formulário à esquerda, a semana à direita com a
 * sessão a marcar desenhada por cima das que já existem, e o cartão de
 * conflitos e capacidade por baixo.
 *
 * O template mostra ainda «destinos de emissão» e «dial-in PSTN» no ecrã de
 * agendar — nenhum dos dois tem campo próprio na reunião: destinos são um
 * recurso da ORGANIZAÇÃO (`stream_destinations`, sem `meeting_id`), escolhidos
 * no Estúdio ao entrar no ar, não ao marcar a sessão; o dial-in é um número
 * fixo da organização (endpoint voice/dids), não algo que se define por
 * reunião. Mostrá-los aqui seria um campo que o servidor ignora — pior do
 * que não o ter. `format`/`waiting_room`/`auto_record`/`record_quality` (R184,
 * migração 0046) são diferentes: vivem na própria reunião e chegam à sala no
 * arranque, por isso têm campo neste formulário.
 */
import { FormEvent, KeyboardEvent, useEffect, useId, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  apiErrorMessage,
  checkConflicts,
  Conflicts,
  createMeetingWithOptions,
  listMeetingRooms,
  Meeting,
  MeetingRoom,
  RecordQuality,
  RecurrenceFreq,
  searchUsers,
  SessionKind,
  User,
} from '../../api'
import { useShell } from '../../components/shellContext'
import { Icon } from '../../ui/icons'
import { Alert, Avatar, cx, Field, IconButton, Segmented, Select, Spinner, TextArea, TextInput, Toggle } from '../../ui/kit'
import WeekView from './WeekView'
import { addDays, fmtDayMonth, fmtTime, groupByDay, hhmm, localeOf, mondayOf, parseYmd, tzShort, weekdayNames, ymd } from './dates'

const DURATIONS = [15, 30, 45, 60, 90, 120, 180]
const BYDAY = ['MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT', 'SUN'] as const
type End = 'never' | 'until' | 'count'
type Check = { s: 'idle' } | { s: 'loading' } | { s: 'ready'; d: Conflicts } | { s: 'error'; msg: string }

function defaultTime(): string {
  const d = new Date(Date.now() + 60 * 60_000)
  d.setMinutes(d.getMinutes() < 30 ? 0 : 30, 0, 0)
  return hhmm(d)
}

export default function ScheduleForm({
  formId,
  initialDate,
  initialTime,
  meetings,
  onBusy,
  onCreated,
}: {
  formId: string
  initialDate: string | null
  initialTime: string | null
  meetings: Meeting[]
  onBusy: (busy: boolean, blocked: boolean) => void
  onCreated: (date: string) => void
}) {
  const { t, i18n } = useTranslation()
  const locale = localeOf(i18n.language)
  const { orgs } = useShell()
  const uid = useId()

  const [title, setTitle] = useState('')
  const [titleErr, setTitleErr] = useState('')
  const [description, setDescription] = useState('')
  const [kind, setKind] = useState<'video' | 'voice'>('video')
  const [format, setFormat] = useState<SessionKind>('meeting')
  const [waitingRoom, setWaitingRoom] = useState(false)
  const [autoRecord, setAutoRecord] = useState(false)
  const [recordQuality, setRecordQuality] = useState<RecordQuality>('1080p')
  const [date, setDate] = useState(initialDate ?? ymd(new Date()))
  const [time, setTime] = useState(initialTime ?? defaultTime())
  const [duration, setDuration] = useState(30)
  const [freq, setFreq] = useState<RecurrenceFreq | ''>('')
  const [interval, setInterval_] = useState(1)
  const [end, setEnd] = useState<End>('never')
  const [until, setUntil] = useState('')
  const [count, setCount] = useState(10)
  const [byday, setByday] = useState<Set<string>>(() => new Set(['MON', 'TUE', 'WED', 'THU', 'FRI']))
  const [invitees, setInvitees] = useState<User[]>([])
  const [roomRef, setRoomRef] = useState('')
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)

  // Rotas vindas da URL (clicar num dia ou numa hora da grelha) mudam o rascunho.
  useEffect(() => {
    if (initialDate) setDate(initialDate)
    if (initialTime) setTime(initialTime)
  }, [initialDate, initialTime])

  // ---- salas físicas de todas as organizações ----
  const [rooms, setRooms] = useState<MeetingRoom[]>([])
  const [roomsFailed, setRoomsFailed] = useState<string[]>([])
  const orgList = orgs.s === 'ready' ? orgs.d : null
  useEffect(() => {
    if (!orgList) return
    let alive = true
    void Promise.allSettled(orgList.map((o) => listMeetingRooms(o.id))).then((res) => {
      if (!alive) return
      setRooms(res.flatMap((r) => (r.status === 'fulfilled' ? r.value : [])))
      setRoomsFailed(orgList.filter((_, i) => res[i].status === 'rejected').map((o) => o.name))
    })
    return () => {
      alive = false
    }
  }, [orgList])
  const room = rooms.find((r) => r.id === roomRef) ?? null

  // ---- início como Date (hora local) ----
  const start = useMemo(() => {
    const d = parseYmd(date)
    const tm = time.match(/^(\d{2}):(\d{2})$/)
    if (!d || !tm) return null
    d.setHours(Number(tm[1]), Number(tm[2]), 0, 0)
    return d
  }, [date, time])

  // ---- conflitos em tempo real (agenda dos convidados + sala física) ----
  const [check, setCheck] = useState<Check>({ s: 'idle' })
  const inviteeKey = invitees.map((i) => i.id).join(',')
  useEffect(() => {
    if (!start) {
      setCheck({ s: 'idle' })
      return
    }
    let alive = true
    setCheck({ s: 'loading' })
    const h = window.setTimeout(() => {
      checkConflicts({
        starts_at: start.toISOString(),
        duration_min: duration,
        invitee_ids: inviteeKey ? inviteeKey.split(',') : [],
        room_ref: roomRef || null,
      })
        .then((d) => alive && setCheck({ s: 'ready', d }))
        .catch((e) => alive && setCheck({ s: 'error', msg: apiErrorMessage(e, t('schedule.conflitos.erro')) }))
    }, 350)
    return () => {
      alive = false
      window.clearTimeout(h)
    }
  }, [start, duration, inviteeKey, roomRef, t])

  const roomBlocked = check.s === 'ready' && check.d.room.length > 0
  useEffect(() => onBusy(busy, roomBlocked), [busy, roomBlocked, onBusy])

  async function submit(e: FormEvent) {
    e.preventDefault()
    setErr('')
    if (!title.trim()) {
      setTitleErr(t('schedule.form.tituloObrigatorio'))
      document.getElementById(`${uid}-title`)?.focus()
      return
    }
    if (!start) {
      setErr(t('schedule.form.dataInvalida'))
      return
    }
    if (roomBlocked) return
    setBusy(true)
    try {
      await createMeetingWithOptions({
        title: title.trim(),
        description: description.trim(),
        kind,
        starts_at: start.toISOString(),
        duration_min: duration,
        invitee_ids: invitees.map((i) => i.id),
        room_ref: roomRef || null,
        recurrence_freq: freq || null,
        recurrence_interval: freq ? interval : undefined,
        recurrence_until: freq && end === 'until' && until ? until : null,
        recurrence_count: freq && end === 'count' ? count : null,
        recurrence_byday: freq === 'weekly' ? BYDAY.filter((d) => byday.has(d)).join(',') : null,
        format,
        waiting_room: waitingRoom,
        auto_record: autoRecord,
        record_quality: recordQuality,
      })
      onCreated(date)
    } catch (e2) {
      setErr(apiErrorMessage(e2, t('schedule.form.erroCriar')))
      setBusy(false)
    }
  }

  const unitLabel =
    freq === 'daily'
      ? t('schedule.recorrencia.dias', { count: interval })
      : freq === 'weekly'
        ? t('schedule.recorrencia.semanas', { count: interval })
        : freq === 'monthly'
          ? t('schedule.recorrencia.meses', { count: interval })
          : t('schedule.recorrencia.anos', { count: interval })
  const wdNames = weekdayNames(locale)

  // ---- semana do lado direito ----
  const weekStart = mondayOf(start ?? parseYmd(date) ?? new Date())
  // Segunda a sexta, como no template; o fim-de-semana só aparece quando a
  // sessão a marcar cai nele.
  const draftDay = start ? (start.getDay() + 6) % 7 : 0
  const weekDays = Array.from({ length: draftDay >= 5 ? 7 : 5 }, (_, i) => addDays(weekStart, i))
  const byDay = useMemo(() => groupByDay(meetings), [meetings])

  return (
    <div className="sched">
      <form id={formId} className="sched__form" onSubmit={submit} noValidate>
        {err && <Alert tone="danger">{err}</Alert>}

        <Field label={t('schedule.form.titulo')} htmlFor={`${uid}-title`} error={titleErr || undefined}>
          <TextInput
            id={`${uid}-title`}
            large
            autoFocus
            value={title}
            onChange={(e) => {
              setTitle(e.target.value)
              setTitleErr('')
            }}
            placeholder={t('schedule.form.tituloPh')}
            aria-invalid={titleErr ? true : undefined}
            required
          />
        </Field>

        <fieldset className="sched__fieldset">
          <legend className="dx-field__label">{t('schedule.form.tipo')}</legend>
          <div className="sched__kinds">
            {(['video', 'voice'] as const).map((k) => (
              <button key={k} type="button" className="sched__kind" aria-pressed={kind === k} onClick={() => setKind(k)}>
                <span>
                  <strong>
                    {kind === k && <Icon name="check" size={11} />}
                    {k === 'voice' ? t('schedule.form.voz') : t('schedule.form.video')}
                  </strong>
                  <small>{k === 'voice' ? t('schedule.form.vozSub') : t('schedule.form.videoSub')}</small>
                </span>
              </button>
            ))}
          </div>
        </fieldset>

        <Segmented<SessionKind>
          label={t('schedule.form.formato')}
          value={format}
          onChange={setFormat}
          options={[
            { value: 'meeting', label: t('schedule.form.formatoReuniao') },
            { value: 'training', label: t('schedule.form.formatoFormacao') },
            { value: 'broadcast', label: t('schedule.form.formatoEmissao') },
            { value: 'hybrid', label: t('schedule.form.formatoHibrida') },
          ]}
        />

        <div className="sched__pair">
          <Toggle
            label={t('schedule.form.salaDeEspera')}
            hint={t('schedule.form.salaDeEsperaDica')}
            checked={waitingRoom}
            onChange={(e) => setWaitingRoom(e.target.checked)}
          />
          <Toggle
            label={t('schedule.form.gravacaoAutomatica')}
            hint={t('schedule.form.gravacaoAutomaticaDica')}
            checked={autoRecord}
            onChange={(e) => setAutoRecord(e.target.checked)}
          />
        </div>
        {autoRecord && (
          <Field label={t('schedule.form.qualidadeGravacao')} htmlFor={`${uid}-quality`}>
            <Select id={`${uid}-quality`} value={recordQuality} onChange={(e) => setRecordQuality(e.target.value as RecordQuality)}>
              <option value="2160p">{t('schedule.form.qualidade2160p')}</option>
              <option value="1080p">{t('schedule.form.qualidade1080p')}</option>
              <option value="720p">{t('schedule.form.qualidade720p')}</option>
              <option value="audio">{t('schedule.form.qualidadeAudio')}</option>
            </Select>
          </Field>
        )}

        <div className="sched__pair">
          <fieldset className="sched__fieldset">
            <legend className="dx-field__label">{t('consola.agenda.dataHora', { fuso: tzShort(locale) })}</legend>
            <div className="sched__when">
              <TextInput
                id={`${uid}-date`}
                type="date"
                value={date}
                onChange={(e) => setDate(e.target.value)}
                required
                aria-label={t('schedule.form.data')}
                className="dx-num"
              />
              <TextInput
                id={`${uid}-time`}
                type="time"
                step={300}
                value={time}
                onChange={(e) => setTime(e.target.value)}
                required
                aria-label={t('schedule.form.hora', { fuso: tzShort(locale) })}
                className="dx-num"
              />
              <Select id={`${uid}-dur`} value={duration} onChange={(e) => setDuration(Number(e.target.value))} aria-label={t('schedule.form.duracao')}>
                {DURATIONS.map((d) => (
                  <option key={d} value={d}>
                    {t('schedule.evento.duracao', { n: d })}
                  </option>
                ))}
              </Select>
            </div>
          </fieldset>
          <Field label={t('schedule.recorrencia.titulo')} htmlFor={`${uid}-freq`}>
            <Select id={`${uid}-freq`} value={freq} onChange={(e) => setFreq(e.target.value as RecurrenceFreq | '')}>
              <option value="">{t('schedule.recorrencia.nao')}</option>
              <option value="daily">{t('schedule.recorrencia.diaria')}</option>
              <option value="weekly">{t('schedule.recorrencia.semanal')}</option>
              <option value="monthly">{t('schedule.recorrencia.mensal')}</option>
              <option value="yearly">{t('schedule.recorrencia.anual')}</option>
            </Select>
          </Field>
        </div>

        {freq && (
        <div className="sched__recur" role="group" aria-label={t('schedule.recorrencia.titulo')}>
          <div className="sched__row">
            <label className="sched__every">
              <span>{t('schedule.recorrencia.aCada')}</span>
              <TextInput
                type="number"
                min={1}
                max={99}
                value={interval}
                onChange={(e) => setInterval_(Math.max(1, Math.min(99, Number(e.target.value) || 1)))}
                className="sched__num"
              />
              <span>{unitLabel}</span>
            </label>
          </div>
          {freq === 'weekly' && (
            <div className="dx-chips" role="group" aria-label={t('schedule.recorrencia.diasSemana')}>
              {BYDAY.map((d, i) => {
                const on = byday.has(d)
                return (
                  <button
                    key={d}
                    type="button"
                    className="dx-chip"
                    aria-pressed={on}
                    onClick={() => {
                      const next = new Set(byday)
                      if (on) next.delete(d)
                      else next.add(d)
                      // Pelo menos um dia: uma recorrência semanal sem dias não tem ocorrências.
                      if (next.size > 0) setByday(next)
                    }}
                  >
                    {wdNames[i]}
                  </button>
                )
              })}
            </div>
          )}
          {freq && (
            <div className="sched__row sched__end">
              <Segmented<End>
                label={t('schedule.recorrencia.fim')}
                value={end}
                onChange={(v) => {
                  setEnd(v)
                  if (v === 'until' && !until) setUntil(ymd(addDays(parseYmd(date) ?? new Date(), 28)))
                }}
                options={[
                  { value: 'never', label: t('schedule.recorrencia.semFim') },
                  { value: 'until', label: t('schedule.recorrencia.ate') },
                  { value: 'count', label: t('schedule.recorrencia.apos') },
                ]}
              />
              {end === 'until' && (
                <TextInput type="date" min={date} value={until} onChange={(e) => setUntil(e.target.value)} aria-label={t('schedule.recorrencia.ateData')} className="sched__until" />
              )}
              {end === 'count' && (
                <label className="sched__every">
                  <TextInput
                    type="number"
                    min={2}
                    max={365}
                    value={count}
                    onChange={(e) => setCount(Math.max(2, Math.min(365, Number(e.target.value) || 2)))}
                    className="sched__num"
                  />
                  <span>{t('schedule.recorrencia.ocorrencias', { count })}</span>
                </label>
              )}
            </div>
          )}
        </div>
        )}

        <Guests invitees={invitees} onChange={setInvitees} />

        <div className="sched__pair">
        {(rooms.length > 0 || roomsFailed.length > 0) && (
          <Field
            label={t('schedule.form.sala')}
            htmlFor={`${uid}-room`}
            hint={roomsFailed.length > 0 ? t('schedule.form.salasFalharam', { orgs: roomsFailed.join(', ') }) : undefined}
          >
            <Select id={`${uid}-room`} value={roomRef} onChange={(e) => setRoomRef(e.target.value)}>
              <option value="">{t('schedule.form.semSala')}</option>
              {rooms.map((r) => (
                <option key={r.id} value={r.id}>
                  {[r.name, r.location, t('schedule.form.lugares', { count: r.capacity })].filter(Boolean).join(' · ')}
                </option>
              ))}
            </Select>
          </Field>
        )}
        </div>

        <Field label={t('schedule.form.descricao')} htmlFor={`${uid}-desc`}>
          <TextArea id={`${uid}-desc`} rows={2} value={description} onChange={(e) => setDescription(e.target.value)} placeholder={t('schedule.form.descricaoPh')} />
        </Field>
      </form>

      <aside className="sched__side" aria-label={t('schedule.form.semanaRotulo')}>
        <div className="sched__side-head">
          <h2>{t('schedule.form.semanaDe', { data: fmtDayMonth(weekStart, locale) })}</h2>
          <span className="dx-spacer" />
          <span className="dx-num dx-muted">{tzShort(locale)}</span>
        </div>
        <WeekView
          days={weekDays}
          byDay={byDay}
          compact
          draft={start ? { start, durationMin: duration, label: title.trim() } : null}
        />
        <div className="sched__legend">
          <span>
            <i className="sched__swatch sched__swatch--draft" aria-hidden="true" />
            {t('schedule.form.estaSessao')}
          </span>
          <span>
            <i className="sched__swatch" aria-hidden="true" />
            {t('schedule.form.outrasReunioes')}
          </span>
        </div>
        <ConflictsCard check={check} room={room} people={invitees.length + 1} locale={locale} />
      </aside>
    </div>
  )
}

function ConflictsCard({ check, room, people, locale }: { check: Check; room: MeetingRoom | null; people: number; locale: string }) {
  const { t } = useTranslation()
  const lines: { tone: 'warning' | 'success' | 'danger'; text: string }[] = []
  if (check.s === 'ready') {
    for (const c of check.d.room) {
      lines.push({ tone: 'danger', text: t('schedule.conflitos.salaOcupada', { titulo: c.meeting_title, hora: fmtTime(new Date(c.starts_at), locale) }) })
    }
    for (const c of check.d.participants) {
      lines.push({
        tone: 'warning',
        text: t('schedule.conflitos.pessoa', { nome: c.username, titulo: c.meeting_title, hora: fmtTime(new Date(c.starts_at), locale) }),
      })
    }
    if (check.d.room.length === 0 && check.d.participants.length === 0) {
      lines.push({ tone: 'success', text: room ? t('schedule.conflitos.nenhumComSala') : t('schedule.conflitos.nenhum') })
    }
  }
  if (room) {
    lines.push(
      people > room.capacity
        ? { tone: 'warning', text: t('schedule.conflitos.capacidadeExcedida', { sala: room.name, lugares: room.capacity, pessoas: people }) }
        : { tone: 'success', text: t('schedule.conflitos.capacidade', { sala: room.name, lugares: room.capacity, pessoas: people }) },
    )
  }
  return (
    <section className="sched__conflicts" aria-live="polite">
      <h3>{t('schedule.conflitos.titulo')}</h3>
      {check.s === 'loading' && (
        <div className="sched__line">
          <Spinner />
          <span>{t('schedule.conflitos.aVerificar')}</span>
        </div>
      )}
      {check.s === 'error' && (
        <div className="sched__line sched__line--warning">
          <Icon name="alert" size={13} />
          <span>{check.msg}</span>
        </div>
      )}
      {lines.map((l, i) => (
        <div key={i} className={cx('sched__line', `sched__line--${l.tone}`)}>
          <Icon name={l.tone === 'success' ? 'check' : 'alert'} size={13} />
          <span>{l.text}</span>
        </div>
      ))}
      {check.s === 'ready' && check.d.room.length > 0 && <small className="dx-muted">{t('schedule.conflitos.salaDica')}</small>}
      {check.s === 'ready' && check.d.participants.length > 0 && <small className="dx-muted">{t('schedule.conflitos.pessoaDica')}</small>}
    </section>
  )
}

/** Convidados com autocompletar: setas, Enter para acrescentar, Esc para limpar. */
function Guests({ invitees, onChange }: { invitees: User[]; onChange: (u: User[]) => void }) {
  const { t } = useTranslation()
  const uid = useId()
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<User[]>([])
  const [searching, setSearching] = useState(false)
  const [err, setErr] = useState('')
  const [active, setActive] = useState(0)
  const seq = useRef(0)
  const chosen = new Set(invitees.map((i) => i.id))

  useEffect(() => {
    const q = query.trim()
    if (q.length < 2) {
      setResults([])
      setSearching(false)
      return
    }
    const n = ++seq.current
    setSearching(true)
    const h = window.setTimeout(() => {
      searchUsers(q)
        .then((r) => {
          if (n !== seq.current) return
          setResults(r)
          setActive(0)
          setErr('')
        })
        .catch((e) => n === seq.current && setErr(apiErrorMessage(e, t('schedule.convidados.erro'))))
        .finally(() => n === seq.current && setSearching(false))
    }, 250)
    return () => window.clearTimeout(h)
  }, [query, t])

  const options = results.filter((u) => !chosen.has(u.id))
  function add(u: User) {
    onChange([...invitees, u])
    setQuery('')
    setResults([])
  }
  function onKey(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'ArrowDown' && options.length) {
      e.preventDefault()
      setActive((a) => (a + 1) % options.length)
    } else if (e.key === 'ArrowUp' && options.length) {
      e.preventDefault()
      setActive((a) => (a - 1 + options.length) % options.length)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (options[active]) add(options[active])
    } else if (e.key === 'Escape' && query) {
      e.stopPropagation()
      setQuery('')
    } else if (e.key === 'Backspace' && !query && invitees.length) {
      onChange(invitees.slice(0, -1))
    }
  }
  const open = query.trim().length >= 2 && (options.length > 0 || (!searching && !err))

  return (
    <div className="dx-field">
      <label className="dx-field__label" htmlFor={`${uid}-q`}>
        <span>
          {t('consola.agenda.participantes')}
          {invitees.length > 0 && <span className="dx-muted dx-num"> · {invitees.length}</span>}
        </span>
        <span className="dx-muted dx-num sched__aside">{t('consola.agenda.autocompletar')}</span>
      </label>
      <div className="sched__guests">
        {invitees.map((u) => (
          <span key={u.id} className="sched__guest">
            <Avatar name={u.username} size={18} />
            {u.username}
            <IconButton icon="x" bare label={t('schedule.convidados.remover', { nome: u.username })} onClick={() => onChange(invitees.filter((i) => i.id !== u.id))} />
          </span>
        ))}
        <input
          id={`${uid}-q`}
          className="sched__guest-input"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKey}
          placeholder={t('schedule.convidados.placeholder')}
          role="combobox"
          aria-expanded={open}
          aria-controls={`${uid}-list`}
          aria-autocomplete="list"
          aria-activedescendant={open && options[active] ? `${uid}-opt-${options[active].id}` : undefined}
          autoComplete="off"
        />
        {searching && <Spinner label={t('schedule.convidados.aProcurar')} />}
      </div>
      {err && <span className="dx-field__error">{err}</span>}
      {open && (
        <ul id={`${uid}-list`} role="listbox" className="sched__results">
          {options.length === 0 ? (
            <li className="dx-muted sched__result--none">{t('ui.semResultados')}</li>
          ) : (
            options.map((u, i) => (
              <li
                key={u.id}
                id={`${uid}-opt-${u.id}`}
                role="option"
                aria-selected={i === active}
                className={cx('sched__result', i === active && 'is-active')}
                onMouseDown={(e) => {
                  e.preventDefault()
                  add(u)
                }}
                onMouseEnter={() => setActive(i)}
              >
                <Avatar name={u.username} />
                <span>
                  <strong>{u.username}</strong>
                  <small>{u.email}</small>
                </span>
                <Icon name="plus" size={14} />
              </li>
            ))
          )}
        </ul>
      )}
    </div>
  )
}
