/**
 * Legendas de uma gravação (`…/captions`): a lista com o estado de cada língua,
 * enviar um WebVTT (PUT — cria ou substitui), publicar ou voltar a rascunho
 * (PATCH) e apagar. Enquanto uma legenda gera, ou se falhou, o servidor
 * responde 409 ao VTT: mostra-se «a gerar» ou «falhou» com a razão, nunca um
 * erro genérico. Gerar legendas pelo servidor ainda não existe (porte do
 * Ollama) e não tem botão.
 */
import { ChangeEvent, FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { deleteRecordingCaption, putRecordingCaption, recordingCaptions, setRecordingCaptionStatus, type RecordingCaption } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Checkbox, IconButton, StatusBadge, TextInput } from '../../ui/kit'
import { recordingErrorMessage } from './apiErrors'

const LANG = /^[a-z]{2,3}(-[A-Za-z0-9]{2,8})?$/

function Estado({ c }: { c: RecordingCaption }) {
  const { t } = useTranslation()
  switch (c.status) {
    case 'generating':
      return <StatusBadge tone="warning">{c.progress_pct === null ? t('player.legendas.aGerar') : t('player.legendas.aGerarPct', { pct: c.progress_pct })}</StatusBadge>
    case 'failed':
      return <StatusBadge tone="record">{t('player.legendas.falhou')}</StatusBadge>
    case 'published':
      return <StatusBadge tone="success">{t('player.legendas.publicada')}</StatusBadge>
    default:
      return <StatusBadge tone="neutral">{t('player.legendas.rascunho')}</StatusBadge>
  }
}

export default function RecordingCaptions({ recordingId, canManage, onChanged }: { recordingId: string; canManage: boolean; onChanged: () => void }) {
  const { t } = useTranslation()
  const { state, reload } = useAsync(() => recordingCaptions(recordingId), [recordingId])
  const [lang, setLang] = useState('')
  const [vtt, setVtt] = useState<{ name: string; text: string } | null>(null)
  const [publish, setPublish] = useState(true)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function run(fn: () => Promise<unknown>, fallback: string) {
    setBusy(true)
    setErr('')
    try {
      await fn()
      reload()
      onChanged()
      return true
    } catch (e) {
      setErr(recordingErrorMessage(e, t, fallback))
      return false
    } finally {
      setBusy(false)
    }
  }

  async function pick(e: ChangeEvent<HTMLInputElement>) {
    const f = e.target.files?.[0]
    setVtt(f ? { name: f.name, text: await f.text() } : null)
  }

  async function upload(e: FormEvent) {
    e.preventDefault()
    if (!vtt || !LANG.test(lang.trim())) return
    if (await run(() => putRecordingCaption(recordingId, lang.trim(), vtt.text, publish), 'player.legendas.erro')) {
      setLang('')
      setVtt(null)
    }
  }

  return (
    <div className="pl-captions">
      {err && <Alert tone="danger">{err}</Alert>}
      <AsyncSection state={state} onRetry={reload}>
        {(list) =>
          list.length === 0 ? (
            <p className="pl-empty">{t('player.legendas.vazio')}</p>
          ) : (
            <ul className="pl-captions__list">
              {list.map((c) => (
                <li key={c.lang} className="pl-form__row">
                  <strong className="dx-num">{c.lang}</strong>
                  <Estado c={c} />
                  {c.status === 'failed' && c.error && <span className="dx-muted">{c.error}</span>}
                  <span className="dx-spacer" />
                  {canManage && (c.status === 'draft' || c.status === 'published') && (
                    <Button size="sm" variant="ghost" disabled={busy} onClick={() => void run(() => setRecordingCaptionStatus(recordingId, c.lang, c.status === 'published' ? 'draft' : 'published'), 'player.legendas.erro')}>
                      {c.status === 'published' ? t('player.legendas.despublicar') : t('player.legendas.publicar')}
                    </Button>
                  )}
                  {canManage && <IconButton icon="trash" bare label={t('player.legendas.apagar', { lang: c.lang })} onClick={() => void run(() => deleteRecordingCaption(recordingId, c.lang), 'player.legendas.erroApagar')} />}
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
      {canManage && (
        <form className="pl-form" onSubmit={upload}>
          <div className="pl-form__row">
            <TextInput className="pl-captions__lang" value={lang} placeholder={t('player.legendas.linguaPh')} maxLength={12} aria-label={t('player.legendas.lingua')} onChange={(e) => setLang(e.target.value)} />
            <input type="file" accept=".vtt,text/vtt" aria-label={t('player.legendas.ficheiro')} onChange={(e) => void pick(e)} />
          </div>
          <div className="pl-form__row">
            <Checkbox label={t('player.legendas.publicarJa')} checked={publish} onChange={(e) => setPublish(e.target.checked)} />
            <span className="dx-spacer" />
            <Button type="submit" size="sm" icon="upload" busy={busy} disabled={!vtt || !LANG.test(lang.trim())}>
              {t('player.legendas.enviar')}
            </Button>
          </div>
        </form>
      )}
    </div>
  )
}
