/**
 * Sobreposições queimadas na imagem: rodapé animado, logótipo, ticker de URL,
 * cronómetro, sondagem (vem da sala ligada) e legendas ao vivo (Whisper local).
 * O «comentário em destaque» do template precisa do chat das plataformas, que
 * não existe no servidor — não aparece aqui.
 */
import { ReactNode, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Button, Field, TextInput } from '../ui/kit'
import type { Sobreposicoes } from './palco'

type Interruptor = 'rodape' | 'logotipo' | 'ticker' | 'cronometro' | 'sondagem' | 'legendas'
const INTERRUPTORES: Interruptor[] = ['rodape', 'logotipo', 'ticker', 'cronometro', 'sondagem', 'legendas']

export default function SobreposicoesPanel({
  valor,
  temLogotipo,
  temSondagem,
  legendas,
  onMudar,
  onLogotipo,
}: {
  valor: Sobreposicoes
  temLogotipo: boolean
  /** Há uma sondagem da sala para mostrar. */
  temSondagem: boolean
  /** Estado das legendas, já em texto (a preparar, erro…); vazio = nada a dizer. */
  legendas: ReactNode
  onMudar: (patch: Partial<Sobreposicoes>) => void
  onLogotipo: (f: File | null) => void
}) {
  const { t } = useTranslation()
  const [editar, setEditar] = useState(false)
  const ficheiro = useRef<HTMLInputElement>(null)

  return (
    <section className="st-group st-panel" data-studio-grupo="sobreposicoes" aria-labelledby="st-sob-h">
      <header className="st-group__head">
        <h2 id="st-sob-h" className="st-group__title">
          {t('studio.sobreposicoes.titulo')}
        </h2>
        <span className="dx-spacer" />
        <Button size="sm" variant="ghost" icon="edit" aria-expanded={editar} onClick={() => setEditar((v) => !v)}>
          {t('studio.sobreposicoes.textos')}
        </Button>
      </header>
      <div className="st-overlays">
        {INTERRUPTORES.map((k) => (
          <button
            key={k}
            type="button"
            className="st-overlay-btn"
            aria-pressed={valor[k]}
            data-studio-sobreposicao={k}
            onClick={() => onMudar({ [k]: !valor[k] })}
          >
            <span className="st-overlay-btn__box" aria-hidden="true">
              {valor[k] && <Icon name="check" size={11} />}
            </span>
            {t(`studio.sobreposicoes.${k}`)}
          </button>
        ))}
      </div>
      {valor.sondagem && !temSondagem && <p className="st-note">{t('studio.sobreposicoes.semSondagem')}</p>}
      {valor.rodape && !valor.nome && !valor.cargo && <p className="st-note">{t('studio.sobreposicoes.semRodape')}</p>}
      {valor.ticker && !valor.url && <p className="st-note">{t('studio.sobreposicoes.semTicker')}</p>}
      {valor.legendas && legendas && (
        <p className="st-note" data-studio="legendas-estado">
          {legendas}
        </p>
      )}
      {editar && (
        <div className="st-overlay-edit">
          <Field label={t('studio.sobreposicoes.nome')} htmlFor="st-sob-nome">
            <TextInput id="st-sob-nome" value={valor.nome} maxLength={60} onChange={(e) => onMudar({ nome: e.target.value })} />
          </Field>
          <Field label={t('studio.sobreposicoes.cargo')} htmlFor="st-sob-cargo">
            <TextInput id="st-sob-cargo" value={valor.cargo} maxLength={80} onChange={(e) => onMudar({ cargo: e.target.value })} />
          </Field>
          <Field label={t('studio.sobreposicoes.url')} htmlFor="st-sob-url">
            <TextInput id="st-sob-url" value={valor.url} maxLength={80} spellCheck={false} onChange={(e) => onMudar({ url: e.target.value })} />
          </Field>
          <Field label={t('studio.sobreposicoes.minutos')} htmlFor="st-sob-min" hint={t('studio.sobreposicoes.minutosDica')}>
            <TextInput
              id="st-sob-min"
              type="number"
              min={0}
              max={600}
              value={valor.minutos}
              onChange={(e) => onMudar({ minutos: Math.min(600, Math.max(0, Number(e.target.value) || 0)) })}
            />
          </Field>
          <input
            ref={ficheiro}
            type="file"
            accept="image/png,image/jpeg,image/svg+xml,image/webp"
            hidden
            onChange={(e) => {
              onLogotipo(e.target.files?.[0] ?? null)
              e.target.value = ''
            }}
          />
          <div className="st-actions">
            <Button size="sm" variant="outline" icon="upload" onClick={() => ficheiro.current?.click()}>
              {t('studio.sobreposicoes.carregarLogotipo')}
            </Button>
            {temLogotipo && (
              <Button size="sm" variant="ghost" icon="x" onClick={() => onLogotipo(null)}>
                {t('studio.sobreposicoes.tirarLogotipo')}
              </Button>
            )}
          </div>
        </div>
      )}
    </section>
  )
}
