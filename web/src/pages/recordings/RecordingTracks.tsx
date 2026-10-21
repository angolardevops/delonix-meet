/**
 * As pistas por fonte de uma gravação ISO (`…/tracks`, ADR-0014 §2.3).
 *
 * Existe porque o interruptor «ISO» do estúdio estava ligado por omissão e o
 * servidor já guardava e servia uma pista por fonte, mas nenhum ecrã as
 * mostrava (lacuna `A-02` da fila de paridade). Quem ligava o estúdio procurava
 * as pistas por câmara e só as encontrava pela API.
 *
 * O vídeo sai em IVF (VP8 cru) e nenhum browser o reproduz: aqui só se
 * descarrega, que é para o que serve, a pós-produção. A ajuda di-lo, para
 * ninguém esperar um leitor.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { downloadRecordingTrack, type RecordingTrack } from '../../api'
import { Alert, Button } from '../../ui/kit'
import { formatBytes, formatDuration } from './format'

export default function RecordingTracks({
  recordingId,
  tracks,
  canDownload,
}: {
  recordingId: string
  tracks: RecordingTrack[]
  canDownload: boolean
}) {
  const { t, i18n } = useTranslation()
  const [busy, setBusy] = useState<string | null>(null)
  const [err, setErr] = useState('')

  // Um `switch` com chaves LITERAIS, e não `t(\`player.pistas.tipo.${kind}\`)`:
  // o extractor de i18n só vê chaves escritas por extenso, e uma chave montada
  // em tempo de execução saía da cobertura do portão sem ninguém notar.
  const tipo = (kind: string) => {
    switch (kind) {
      case 'video':
        return t('player.pistas.tipoVideo')
      case 'screen':
        return t('player.pistas.tipoEcra')
      case 'audio':
        return t('player.pistas.tipoAudio')
      case 'screen-audio':
        return t('player.pistas.tipoSomDoEcra')
      default:
        return kind
    }
  }

  async function descarregar(track: RecordingTrack) {
    setBusy(track.id)
    setErr('')
    try {
      await downloadRecordingTrack(recordingId, track)
    } catch {
      setErr(t('player.pistas.erroDescarregar'))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="pl-tracks">
      <p className="pl-empty">{t('player.pistas.ajuda')}</p>
      {err && <Alert tone="danger">{err}</Alert>}
      <ul className="pl-tracks__list">
        {tracks.map((tr) => (
          <li key={tr.id} className="pl-tracks__item">
            <span className="pl-tracks__name">
              {tr.source_number !== null
                ? tr.source_label
                  ? t('player.pistas.camaraComNome', { n: tr.source_number, nome: tr.source_label })
                  : t('player.pistas.camara', { n: tr.source_number })
                : t('player.pistas.semFonte')}
              {' · '}
              {tipo(tr.kind)}
            </span>
            <span className="pl-tracks__meta dx-num">
              {t('player.pistas.comecaEm', { quando: formatDuration(tr.starts_at_ms / 1000) })} · {formatBytes(tr.size_bytes, i18n.language)}
            </span>
            {canDownload && (
              <Button size="sm" variant="secondary" busy={busy === tr.id} onClick={() => void descarregar(tr)}>
                {t('player.pistas.descarregar')}
              </Button>
            )}
          </li>
        ))}
      </ul>
    </div>
  )
}
