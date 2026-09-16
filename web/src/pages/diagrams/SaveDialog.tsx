/**
 * «Guardar no storage»: manda um PNG do quadro para a biblioteca da
 * organização (POST /api/whiteboards) com o título e a sala.
 *
 * A sala é o que anexa o quadro às gravações: o leitor lista os quadros cujo
 * `room_code` é o da gravação. As sugestões vêm do que o servidor já devolve
 * (salas das gravações e das reuniões da pessoa); um código escrito à mão
 * passa pelo `parseRoomCode`.
 *
 * O MODELO não vai: o servidor só guarda PNG. O diálogo di-lo.
 */
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, isAbort, listMeetings, recordingsLibrary, saveWhiteboard, WhiteboardMeta } from '../../api'
import { useAsync } from '../../components/AsyncSection'
import { parseRoomCode } from '../../roomCode'
import { Alert, Button, Dialog, Field, Select, TextInput } from '../../ui/kit'

const OTHER = '__outra__'

export default function SaveDialog({
  title,
  roomCode,
  empty,
  makePng,
  onClose,
  onSaved,
}: {
  title: string
  roomCode: string
  empty: boolean
  makePng: () => Promise<string>
  onClose: () => void
  onSaved: (meta: WhiteboardMeta, title: string, roomCode: string) => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState(title)
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)
  const rooms = useAsync(
    async (signal) => {
      const [recs, meetings] = await Promise.all([
        recordingsLibrary(signal).catch((e) => {
          if (isAbort(e)) throw e
          return []
        }),
        listMeetings(signal).catch((e) => {
          if (isAbort(e)) throw e
          return []
        }),
      ])
      return {
        recordings: [...new Set(recs.filter((r) => r.status !== 'failed').map((r) => r.room_code))],
        meetings: meetings.filter((m) => m.room_code).map((m) => ({ code: m.room_code as string, title: m.title })),
      }
    },
    [],
  )
  const lists = rooms.state.s === 'ready' ? rooms.state.d : { recordings: [], meetings: [] }
  const meetingRooms = useMemo(() => lists.meetings.filter((m) => !lists.recordings.includes(m.code)), [lists])
  const known = new Set([...lists.recordings, ...lists.meetings.map((m) => m.code)])
  const [choice, setChoice] = useState(roomCode)
  const [other, setOther] = useState('')
  const parsed = choice === OTHER ? parseRoomCode(other) : null
  const finalRoom = choice === OTHER ? parsed ?? '' : choice
  const invalid = choice === OTHER && !parsed

  async function save() {
    if (empty || invalid) return
    setBusy(true)
    setErr('')
    try {
      const png = await makePng()
      const meta = await saveWhiteboard(name.trim(), finalRoom, png)
      onSaved(meta, name.trim(), finalRoom)
    } catch (e) {
      setErr(apiErrorMessage(e, t('diagrams.guardar.erro')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      title={t('diagrams.guardar.titulo')}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" disabled={busy} onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button variant="primary" icon="upload" busy={busy} disabled={empty || invalid} onClick={() => void save()}>
            {t('diagrams.guardar.confirmar')}
          </Button>
        </>
      }
    >
      <p className="dg-dialog__text">{t('diagrams.guardar.texto')}</p>
      {empty && <Alert tone="warning">{t('diagrams.guardar.vazio')}</Alert>}
      <Field label={t('diagrams.guardar.tituloCampo')}>
        <TextInput value={name} maxLength={120} onChange={(e) => setName(e.target.value)} />
      </Field>
      <Field label={t('diagrams.guardar.sala')} hint={t('diagrams.guardar.salaAjuda')} error={invalid && other.trim() ? t('diagrams.guardar.salaInvalida') : undefined}>
        <Select value={choice} onChange={(e) => setChoice(e.target.value)}>
          <option value="">{t('diagrams.guardar.salaNenhuma')}</option>
          {roomCode && !known.has(roomCode) && <option value={roomCode}>{roomCode}</option>}
          {lists.recordings.length > 0 && (
            <optgroup label={t('diagrams.guardar.salasGravacoes')}>
              {lists.recordings.map((c) => (
                <option key={`r-${c}`} value={c}>
                  {c}
                </option>
              ))}
            </optgroup>
          )}
          {meetingRooms.length > 0 && (
            <optgroup label={t('diagrams.guardar.salasReunioes')}>
              {meetingRooms.map((m) => (
                <option key={`m-${m.code}`} value={m.code}>
                  {m.title} · {m.code}
                </option>
              ))}
            </optgroup>
          )}
          <option value={OTHER}>{t('diagrams.guardar.salaOutra')}</option>
        </Select>
      </Field>
      {choice === OTHER && (
        <Field label={t('diagrams.guardar.salaCodigo')}>
          <TextInput code autoFocus value={other} placeholder="abc-defg-hij" onChange={(e) => setOther(e.target.value)} />
        </Field>
      )}
      {err && <Alert tone="danger">{err}</Alert>}
    </Dialog>
  )
}
