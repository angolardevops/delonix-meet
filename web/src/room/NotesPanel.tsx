import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Alert, Button, Field, Select } from '../ui/kit'
import type { Transcription } from './useTranscription'

/** Idiomas do reconhecimento de voz. O nome de cada idioma escreve-se NO idioma. */
const STT_LANGS: { value: string; label: string }[] = [
  { value: 'pt-PT', label: 'Português' },
  { value: 'en-US', label: 'English' },
  { value: 'es-ES', label: 'Español' },
  { value: 'fr-FR', label: 'Français' },
  { value: 'de-DE', label: 'Deutsch' },
  { value: 'it-IT', label: 'Italiano' },
]

/** Transcrição partilhada e acta. Só o anfitrião liga; todos captam o seu mic. */
export function NotesPanel({ transcription, isHost }: { transcription: Transcription; isHost: boolean }) {
  const { t } = useTranslation()
  const tr = transcription
  return (
    <div className="rm-notes">
      <p className="rm-panel__note">
        <Icon name="info" size={12} />
        {t('room.notas.explicacao')}
      </p>
      {tr.scribeBy && !isHost && (
        <Alert tone="warning" icon="mic">
          {t('room.notas.aTuaFalaCaptada', { nome: tr.scribeBy })}
        </Alert>
      )}
      <div className="rm-notes__body" aria-live="polite">
        {tr.lines.length === 0 && !tr.interim && <p className="dx-muted">{t('room.notas.vazio')}</p>}
        {tr.lines.map((l, i) => (
          <p key={i} className="rm-notes__line">
            {l}
          </p>
        ))}
        {tr.interim && <p className="rm-notes__line is-interim">{tr.interim}</p>}
      </div>
      <div className="rm-notes__foot">
        <Field label={t('room.notas.idioma')} htmlFor="rm-stt-lang">
          <Select id="rm-stt-lang" value={tr.sttLang} disabled={tr.transcribing} onChange={(e) => tr.setSttLang(e.target.value)}>
            {STT_LANGS.map((l) => (
              <option key={l.value} value={l.value}>
                {l.label}
              </option>
            ))}
          </Select>
        </Field>
        <div className="rm-block__row">
          <Button
            size="sm"
            variant={tr.transcribing ? 'danger' : 'primary'}
            icon={tr.transcribing ? 'stop' : 'mic'}
            disabled={!isHost}
            title={isHost ? undefined : t('room.notas.soAnfitriao')}
            onClick={tr.toggleTranscription}
          >
            {tr.transcribing ? t('room.notas.parar') : t('room.notas.iniciar')}
          </Button>
          <Button size="sm" variant="outline" icon={tr.momSaved ? 'check' : 'notes'} disabled={tr.lines.length === 0} onClick={() => void tr.saveMinutes()}>
            {tr.momSaved ? t('room.notas.guardada') : t('room.notas.guardarActa')}
          </Button>
        </div>
        {!isHost && <p className="dx-muted">{t('room.notas.soAnfitriao')}</p>}
      </div>
    </div>
  )
}
