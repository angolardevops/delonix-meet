/**
 * Gravação local: o estado do gravador e o arquivo do dispositivo (as aulas
 * guardadas que ainda não subiram). Os números vêm do IndexedDB, não do
 * template — `arquivo.porEnviar()` e `arquivo.ocupacao()`.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button, cx, Select, StatusBadge } from '../ui/kit'
import type { AulaGuardada } from './arquivo'
import Cronometro from './Cronometro'
import { formatarBytes } from './palco'

/** O tamanho da gravação em curso, lido do compositor uma vez por segundo. */
function TamanhoEmCurso({ activo, ler }: { activo: boolean; ler: () => number }) {
  const { i18n } = useTranslation()
  const [bytes, setBytes] = useState(() => ler())
  useEffect(() => {
    setBytes(ler())
    if (!activo) return
    const id = setInterval(() => setBytes(ler()), 1000)
    return () => clearInterval(id)
  }, [activo, ler])
  return <span data-studio="tamanho-gravacao">{formatarBytes(bytes, i18n.language)}</span>
}

export default function LocalPanel({
  estado,
  lerSegundos,
  porEnviar,
  ocupacaoBytes,
  online,
  resolucao,
  qualidade,
  qualidades,
  qualidadeBloqueada,
  onQualidade,
  lerBytes,
  onEnviar,
}: {
  estado: 'parado' | 'a-gravar' | 'pausa'
  lerSegundos: () => number
  porEnviar: AulaGuardada[]
  ocupacaoBytes: number
  online: boolean
  /** Tamanho real do canvas de gravação, lido do compositor. */
  resolucao: string
  /** O perfil escolhido. */
  qualidade: string
  qualidades: { valor: string; rotulo: string }[]
  /** A gravar ou no ar: a qualidade não muda por baixo do fluxo. */
  qualidadeBloqueada: boolean
  onQualidade: (q: string) => void
  /** Bytes do ficheiro completo da gravação em curso. */
  lerBytes: () => number
  onEnviar: () => void
}) {
  const { t } = useTranslation()
  const activo = estado !== 'parado'
  return (
    <section className={cx('st-local', activo && 'is-rec')} data-studio="local" aria-labelledby="st-local-h">
      <div className="st-card__row">
        <h2 id="st-local-h" className="st-local__title">
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
      </div>
      <p className="dx-num st-rec-line">
        <span className={cx('st-rec-dot', !activo && 'is-off')} aria-hidden="true" />
        <span>{resolucao}</span>
        <span aria-hidden="true">·</span>
        <Cronometro activo={estado === 'a-gravar'} ler={lerSegundos} />
        {activo && (
          <>
            <span aria-hidden="true">·</span>
            <TamanhoEmCurso activo={estado === 'a-gravar'} ler={lerBytes} />
          </>
        )}
      </p>
      <div className="st-local__row">
        <label className="st-label" htmlFor="st-qualidade">
          {t('studio.qualidade.titulo')}
        </label>
        <Select
          id="st-qualidade"
          value={qualidade}
          disabled={qualidadeBloqueada}
          data-studio="qualidade"
          title={qualidadeBloqueada ? t('studio.qualidade.bloqueada') : t('studio.qualidade.nota')}
          onChange={(e) => onQualidade(e.target.value)}
        >
          {qualidades.map((q) => (
            <option key={q.valor} value={q.valor}>
              {q.rotulo}
            </option>
          ))}
        </Select>
      </div>
      <div className="st-local__queue" data-studio="fila">
        {porEnviar.length === 0 ? (
          <span className="st-note">{t('studio.local.nadaPorEnviar')}</span>
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
