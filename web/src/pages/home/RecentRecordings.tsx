/**
 * Gravações recentes em cartões. Uma gravação falhada NÃO é clicável (R59):
 * não há nada para abrir, e o cartão diz porquê em vez de fingir um vídeo.
 *
 * «Importar gravação» usa o mesmo caminho do Estúdio: uma gravação pertence a
 * uma sala e só quem participou nela a pode carregar (recordings.rs), por isso
 * cria-se uma sala com o nome do ficheiro, regista-se a entrada e carrega-se.
 * O tecto é o do servidor, 512 MiB (MAX_RECORDING_BYTES) — não os 12 GB do
 * template, que precisam de upload resumível.
 */
import { ChangeEvent, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, createRoom, joinRoom, MAX_RECORDING_UPLOAD_BYTES, recordingsLibrary, RecordingItem, uploadRecording } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { useShell } from '../../components/shellContext'
import { Icon } from '../../ui/icons'
import { Alert, Button, Empty, Skeleton, StatusBadge } from '../../ui/kit'
import { fmtBytes, localeOf } from '../calendar/dates'

const MAX = 4

export default function RecentRecordings() {
  const { t, i18n } = useTranslation()
  const { navigate } = useShell()
  const locale = localeOf(i18n.language)
  const fileRef = useRef<HTMLInputElement>(null)
  const [importing, setImporting] = useState(false)
  const [importMsg, setImportMsg] = useState<{ tone: 'danger' | 'success'; text: string } | null>(null)
  const { state, reload } = useAsync(async (signal) => {
    const all = await recordingsLibrary(signal)
    return [...all].sort((a, b) => b.created_at.localeCompare(a.created_at)).slice(0, MAX)
  }, [])

  async function onFile(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    e.target.value = ''
    if (!file) return
    setImportMsg(null)
    if (file.size > MAX_RECORDING_UPLOAD_BYTES) {
      setImportMsg({ tone: 'danger', text: t('consola.inicio.importarGrande', { max: fmtBytes(MAX_RECORDING_UPLOAD_BYTES, locale).value + ' ' + fmtBytes(MAX_RECORDING_UPLOAD_BYTES, locale).unit }) })
      return
    }
    setImporting(true)
    try {
      const title = file.name.replace(/\.[a-z0-9]+$/i, '') || file.name
      const room = await createRoom(title)
      await joinRoom(room.code)
      await uploadRecording(room.code, file, file.name)
      setImportMsg({ tone: 'success', text: t('consola.inicio.importada', { nome: file.name }) })
      reload()
    } catch (x) {
      setImportMsg({ tone: 'danger', text: apiErrorMessage(x, t('consola.inicio.importarErro')) })
    } finally {
      setImporting(false)
    }
  }

  const skeleton = (
    <div className="home-recs" aria-busy="true">
      {Array.from({ length: MAX }, (_, i) => (
        <div key={i} className="home-rec">
          <div className="home-rec__thumb" />
          <div className="home-rec__body">
            <Skeleton h={11} w="80%" />
            <Skeleton h={9} w="50%" />
          </div>
        </div>
      ))}
    </div>
  )

  function body(r: RecordingItem) {
    const failed = r.status !== 'ready'
    const size = fmtBytes(r.size_bytes, locale)
    return (
      <>
        <span className="home-rec__thumb">
          {failed ? (
            <StatusBadge tone="record" icon="alert">
              {t('home.gravacoes.falhou')}
            </StatusBadge>
          ) : (
            <>
              <Icon name="play" size={18} />
              <span className="home-rec__chip dx-num">
                {size.value} {size.unit}
              </span>
            </>
          )}
        </span>
        <span className="home-rec__body">
          <strong>{r.filename.replace(/\.(webm|mp4|mkv)$/i, '')}</strong>
          <small>
            {new Date(r.created_at).toLocaleDateString(locale, { day: 'numeric', month: 'short' })}
            {' · '}
            {failed ? r.failure_reason || t('home.gravacoes.semMedia') : <span className="dx-num">{r.room_code}</span>}
          </small>
        </span>
      </>
    )
  }

  return (
    <section className="home-section" aria-labelledby="home-gravacoes">
      <div className="home-section__head">
        <h2 id="home-gravacoes">{t('home.gravacoes.titulo')}</h2>
        <span className="dx-spacer" />
        <Button size="sm" variant="ghost" icon="upload" busy={importing} onClick={() => fileRef.current?.click()} data-testid="home-importar">
          {t('consola.inicio.importar')}
        </Button>
        <input
          ref={fileRef}
          type="file"
          accept="video/mp4,video/webm,video/x-matroska,.mp4,.webm,.mkv"
          hidden
          onChange={(e) => void onFile(e)}
          aria-label={t('consola.inicio.importar')}
        />
        <a className="home-link" href="#/recordings">
          {t('home.gravacoes.biblioteca')}
        </a>
      </div>
      <p className="dx-muted home-import-hint">{t('consola.inicio.importarDica')}</p>
      {importMsg && <Alert tone={importMsg.tone}>{importMsg.text}</Alert>}
      <AsyncSection state={state} onRetry={reload} skeleton={skeleton}>
        {(rs) =>
          rs.length === 0 ? (
            <div className="home-panel">
              <Empty icon="film" title={t('home.gravacoes.vazio')}>
                {t('home.gravacoes.vazioDica')}
              </Empty>
            </div>
          ) : (
            <ul className="home-recs" role="list">
              {rs.map((r) => (
                <li key={r.id}>
                  {r.status === 'ready' ? (
                    <button type="button" className="home-rec" onClick={() => navigate('recordings')}>
                      {body(r)}
                    </button>
                  ) : (
                    <div className="home-rec home-rec--failed">{body(r)}</div>
                  )}
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
    </section>
  )
}
