/**
 * As gravações da sala, no painel das pessoas — mais uma VISTA da gravação, e
 * por isso com a regra das outras (R59): só se oferece o que o servidor vai
 * aceitar. Descarregar pede uma gravação com ficheiro E a permissão de quem
 * vê (o `?dl=1` é do dono ou de um administrador); o resto fica inerte e diz
 * porquê — «A processar N%» enquanto o servidor compõe, a causa quando
 * falhou, e de quem é o download quando não é de quem vê.
 *
 * O estado não se decide aqui: vem de `fromRecordingItem`, a mesma leitura da
 * biblioteca, do início e do leitor.
 */
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { RecordingLibraryItem } from '../api'
import { formatBytes, formatDateTimeShort } from '../pages/recordings/format'
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
  const lang = i18n.language
  const views = recordings.map(fromRecordingItem)

  // A composição acaba sem ninguém tocar em nada: a linha passa a botão (ou a
  // falhada) sozinha, e quem não vê o ecrã tem de o ouvir. Só os dois fins —
  // a percentagem a andar de 4 em 4 s seria ruído.
  const [announcement, setAnnouncement] = useState('')
  const composing = useRef(new Set<string>())
  useEffect(() => {
    const ended = views.filter((v) => composing.current.has(v.id) && !v.processing)
    composing.current = new Set(views.filter((v) => v.processing).map((v) => v.id))
    if (ended.length > 0) setAnnouncement(ended.map((v) => `${v.name}: ${stateText({ kind: v.failed ? 'failed' : 'ready' })}`).join('. '))
    // `recordings` é a entrada real; `views` e `stateText` são derivados a cada
    // render, e pô-los aqui fazia o anúncio correr sempre — o leitor de ecrã
    // repetiria a lista inteira em vez de dizer só o que acabou.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recordings])

  if (views.length === 0) return <p className="dx-muted">{t('room.pessoas.semGravacoes')}</p>
  return (
    <>
      <ul className="rm-recs" role="list">
        {views.map((v) => {
          const when = formatDateTimeShort(v.createdAt, lang)
          const download = v.hasFile && v.canDownload
          const text = (
            <span className="rm-rec__text">
              <span>
                {download && <span className="dx-sr-only">{t('recordings.accoes.descarregar')} </span>}
                {v.filename}
              </span>
              {v.processing ? (
                <small className="dx-num rm-rec__state">
                  {when} · {stateText({ kind: 'processing', pct: v.progressPct })}
                </small>
              ) : v.failed ? (
                // Sem causa registada a frase de recurso já diz que falhou.
                <small className="rm-rec__state">
                  {v.failureReason ? `${stateText({ kind: 'failed' })} · ${v.failureReason}` : t('recordings.estado.semCausa')}
                </small>
              ) : (
                <small className="dx-num dx-muted">
                  {when} · {formatBytes(v.sizeBytes ?? 0, lang)}
                </small>
              )}
              {v.hasFile && !v.canDownload && <small className="dx-muted">{t('room.pessoas.semDescarga')}</small>}
            </span>
          )
          return (
            <li key={v.id}>
              {download ? (
                <button type="button" className="rm-rec" onClick={() => onDownload(v.source)}>
                  <Icon name="download" size={14} />
                  {text}
                </button>
              ) : (
                <div className={cx('rm-rec', 'rm-rec--inert', v.processing && 'rm-rec--processing', v.failed && 'rm-rec--failed')}>
                  <Icon name={v.processing ? 'hourglass' : v.failed ? 'alert' : 'film'} size={14} />
                  {text}
                </div>
              )}
            </li>
          )
        })}
      </ul>
      <span className="dx-sr-only" role="status">
        {announcement}
      </span>
    </>
  )
}
