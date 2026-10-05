/**
 * As gravações da sala, no painel das pessoas — mais uma VISTA da gravação, e
 * por isso com a regra das outras (R59): só se oferece o que o servidor vai
 * aceitar. Descarregar pede uma gravação com ficheiro E a permissão de quem
 * vê (o `?dl=1` é do dono ou de um administrador); o resto fica inerte e diz
 * porquê — «A processar N%» enquanto o servidor compõe, a causa quando falhou.
 *
 * O estado não se decide aqui: vem de `fromRecordingItem`, a mesma leitura da
 * biblioteca, do início e do leitor.
 */
import { useTranslation } from 'react-i18next'
import type { RecordingLibraryItem } from '../api'
import { useStateText } from '../pages/recordings/RecordingState'
import { fromRecordingItem } from '../pages/recordings/recordingView'
import { Icon } from '../ui/icons'
import { cx } from '../ui/kit'

export function RoomRecordings({
  recordings,
  onDownload,
}: {
  recordings: RecordingLibraryItem[]
  onDownload: (r: RecordingLibraryItem) => void
}) {
  const { t, i18n } = useTranslation()
  const stateText = useStateText()
  if (recordings.length === 0) return <p className="dx-muted">{t('room.pessoas.semGravacoes')}</p>
  return (
    <>
      {recordings.map((r) => {
        const v = fromRecordingItem(r)
        const when = new Date(v.createdAt).toLocaleString(i18n.language)
        const text = (
          <span className="rm-rec__text">
            <span>{v.filename}</span>
            {v.processing ? (
              <small className="dx-num rm-rec__state">
                {when} · {stateText({ kind: 'processing', pct: v.progressPct })}
              </small>
            ) : v.failed ? (
              <small className="rm-rec__state">
                {stateText({ kind: 'failed' })} · {v.failureReason || t('recordings.estado.semCausa')}
              </small>
            ) : (
              <small className="dx-num dx-muted">
                {when} · {t('room.pessoas.megabytes', { n: ((v.sizeBytes ?? 0) / 1_048_576).toFixed(1) })}
              </small>
            )}
          </span>
        )
        if (v.hasFile && v.canDownload) {
          return (
            <button key={v.id} type="button" className="rm-rec" onClick={() => onDownload(r)}>
              <Icon name="download" size={14} />
              {text}
            </button>
          )
        }
        return (
          <div key={v.id} className={cx('rm-rec', 'rm-rec--inert', v.processing && 'rm-rec--processing', v.failed && 'rm-rec--failed')}>
            <Icon name={v.processing ? 'hourglass' : v.failed ? 'alert' : 'film'} size={14} />
            {text}
          </div>
        )
      })}
    </>
  )
}
