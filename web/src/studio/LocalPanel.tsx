/**
 * Gravação local: o estado do gravador e o arquivo do dispositivo (as aulas
 * guardadas que ainda não subiram). Os números vêm do IndexedDB, não do
 * template — `arquivo.porEnviar()` e `arquivo.ocupacao()`.
 */
import { useTranslation } from 'react-i18next'
import { Button, cx, StatusBadge } from '../ui/kit'
import type { AulaGuardada } from './arquivo'
import Cronometro from './Cronometro'

export default function LocalPanel({
  estado,
  lerSegundos,
  porEnviar,
  ocupacaoBytes,
  online,
  resolucao,
  onEnviar,
}: {
  estado: 'parado' | 'a-gravar' | 'pausa'
  lerSegundos: () => number
  porEnviar: AulaGuardada[]
  ocupacaoBytes: number
  online: boolean
  /** Tamanho real do canvas de gravação, lido do compositor. */
  resolucao: string
  onEnviar: () => void
}) {
  const { t } = useTranslation()
  const activo = estado !== 'parado'
  return (
    <section className="st-group" data-studio="local" aria-labelledby="st-local-h">
      <header className="st-group__head">
        <h2 id="st-local-h" className="st-group__title">
          {t('studio.local.titulo')}
        </h2>
        <span className="dx-spacer" />
        {estado === 'a-gravar' ? (
          <StatusBadge tone="record">{t('studio.local.aGravar')}</StatusBadge>
        ) : estado === 'pausa' ? (
          <StatusBadge tone="warning">{t('studio.local.emPausa')}</StatusBadge>
        ) : (
          <StatusBadge tone="neutral">{t('studio.local.parada')}</StatusBadge>
        )}
      </header>
      <div className={cx('st-card', activo && 'st-card--rec')}>
        <div className="st-card__row">
          <span className="dx-num st-strong">
            <Cronometro activo={estado === 'a-gravar'} ler={lerSegundos} />
          </span>
          <span className="dx-spacer" />
          <span className="dx-num st-small dx-muted">{resolucao}</span>
        </div>
        <p className="st-note">{t('studio.local.formato')}</p>
      </div>
      <div className="st-card" data-studio="fila">
        {porEnviar.length === 0 ? (
          <p className="st-note">{t('studio.local.nadaPorEnviar')}</p>
        ) : (
          <>
            <div className="st-card__row">
              <strong className="st-small">{t('studio.local.porEnviar', { count: porEnviar.length })}</strong>
              <span className="dx-spacer" />
              <span className="dx-num st-small dx-muted">
                {t('studio.local.ocupacao', { mb: (ocupacaoBytes / 1_048_576).toFixed(1) })}
              </span>
            </div>
            {porEnviar[0]?.erro && <p className="st-note st-note--warn">{porEnviar[0].erro}</p>}
            <Button size="sm" variant="secondary" icon="upload" disabled={!online} onClick={onEnviar}>
              {t('studio.local.enviarAgora')}
            </Button>
          </>
        )}
      </div>
    </section>
  )
}
