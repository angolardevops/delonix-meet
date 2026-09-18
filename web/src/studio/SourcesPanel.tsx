/**
 * Coluna esquerda do Estúdio: as FONTES e a composição — o ecrã (inteiro ou
 * uma região) e a tua imagem (câmara, posição, tamanho, fundo, forma).
 *
 * Não tem estado próprio: o estado é da página, que é quem fala com o
 * compositor. Isto só desenha e devolve intenções.
 */
import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Button, cx } from '../ui/kit'
import type { CantoDoAvatar, EstadoDaImagem, EstadoDoAvatar, FormaDoAvatar, Recorte } from './compositor'
import { IMAGEM_INICIAL } from './compositor'

/** Ordem fixa: 0 = superior-esquerdo … 3 = inferior-direito (o e2e conta com isto). */
const CANTOS: { key: Exclude<CantoDoAvatar, 'livre'>; i18n: string }[] = [
  { key: 'superior-esquerdo', i18n: 'superiorEsquerdo' },
  { key: 'superior-direito', i18n: 'superiorDireito' },
  { key: 'inferior-esquerdo', i18n: 'inferiorEsquerdo' },
  { key: 'inferior-direito', i18n: 'inferiorDireito' },
]

/** Escolha segmentada com `aria-pressed` e um atributo estável por opção. */
function Escolha<T extends string>({
  label,
  value,
  options,
  onPick,
  attr,
}: {
  label: string
  value: T | null
  options: { value: T; label: ReactNode; disabled?: boolean; title?: string }[]
  onPick: (v: T) => void
  attr: string
}) {
  return (
    <div className="dx-seg st-seg" role="group" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          aria-pressed={o.value === value}
          disabled={o.disabled}
          title={o.title}
          {...{ [attr]: o.value }}
          onClick={() => onPick(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}

export default function SourcesPanel({
  temEcra,
  temCamara,
  recorte,
  avatar,
  imagem,
  aPrepararRecorte,
  onEscolherEcra,
  onEcraInteiro,
  onAbrirRegiao,
  onAlternarCamara,
  onAvatar,
  onImagem,
  onFundo,
}: {
  temEcra: boolean
  temCamara: boolean
  recorte: Recorte
  avatar: EstadoDoAvatar
  imagem: EstadoDaImagem
  aPrepararRecorte: boolean
  onEscolherEcra: () => void
  onEcraInteiro: () => void
  onAbrirRegiao: () => void
  onAlternarCamara: () => void
  onAvatar: (patch: Partial<EstadoDoAvatar>) => void
  onImagem: (patch: Partial<EstadoDaImagem>) => void
  onFundo: (semFundo: boolean) => void
}) {
  const { t } = useTranslation()
  const parcial = recorte.w < 1 || recorte.h < 1

  return (
    <>
      <section className="st-group" data-studio-grupo="fonte" aria-labelledby="st-fonte-h">
        <h2 id="st-fonte-h" className="st-group__title">
          {t('studio.fonte.titulo')}
        </h2>
        <Button
          variant={temEcra ? 'secondary' : 'primary'}
          icon="screen"
          block
          data-studio="escolher-ecra"
          onClick={onEscolherEcra}
        >
          {temEcra ? t('studio.fonte.trocarEcra') : t('studio.fonte.escolherEcra')}
        </Button>
        {!temEcra ? (
          <p className="st-note">{t('studio.fonte.semEcra')}</p>
        ) : (
          <>
            <span className="st-label">{t('studio.fonte.enquadramento')}</span>
            <Escolha
              label={t('studio.fonte.enquadramento')}
              attr="data-studio-regiao"
              value={parcial ? 'regiao' : 'tudo'}
              options={[
                { value: 'tudo', label: t('studio.fonte.tudo') },
                { value: 'regiao', label: t('studio.fonte.regiao') },
              ]}
              onPick={(v) => (v === 'tudo' ? onEcraInteiro() : onAbrirRegiao())}
            />
            {parcial && (
              <p className="st-note">
                {t('studio.fonte.regiaoActual')}{' '}
                <span className="dx-num st-strong" data-studio="regiao-rotulo">
                  {Math.round(recorte.w * 100)}% × {Math.round(recorte.h * 100)}%
                </span>
              </p>
            )}
          </>
        )}
      </section>

      <section className="st-group" data-studio-grupo="imagem" aria-labelledby="st-imagem-h">
        <h2 id="st-imagem-h" className="st-group__title">
          {t('studio.imagem.titulo')}
        </h2>
        <Button
          variant={temCamara ? 'secondary' : 'primary'}
          icon={temCamara ? 'videoOff' : 'video'}
          block
          data-studio="camara"
          onClick={onAlternarCamara}
        >
          {temCamara ? t('studio.imagem.desligar') : t('studio.imagem.ligar')}
        </Button>

        {temCamara && (
          <>
            <span className="st-label">{t('studio.imagem.posicao')}</span>
            <div className="st-corners" role="group" aria-label={t('studio.imagem.posicao')}>
              {CANTOS.map((c) => (
                <button
                  key={c.key}
                  type="button"
                  className={cx('st-corner', `st-corner--${c.key}`)}
                  aria-pressed={avatar.canto === c.key}
                  aria-label={t(`studio.imagem.cantos.${c.i18n}`)}
                  title={t(`studio.imagem.cantos.${c.i18n}`)}
                  data-studio-canto={c.key}
                  onClick={() => onAvatar({ canto: c.key })}
                >
                  <span className="st-corner__dot" aria-hidden="true" />
                </button>
              ))}
            </div>
            <p className="st-note">
              {avatar.canto === 'livre' ? t('studio.imagem.livre') : t('studio.imagem.arrasta')}
            </p>

            <label className="st-label st-label--row" htmlFor="st-tamanho">
              <span>{t('studio.imagem.tamanho')}</span>
              <span className="dx-num">{Math.round(avatar.tamanho * 100)}%</span>
            </label>
            <input
              id="st-tamanho"
              className="st-range"
              type="range"
              min={10}
              max={45}
              value={Math.round(avatar.tamanho * 100)}
              data-studio="tamanho"
              onChange={(e) => onAvatar({ tamanho: Number(e.target.value) / 100 })}
            />

            <span className="st-label">{t('studio.imagem.fundo')}</span>
            <Escolha
              label={t('studio.imagem.fundo')}
              attr="data-studio-modo"
              value={avatar.modo}
              options={[
                { value: 'bolha', label: t('studio.imagem.comFundo'), disabled: aPrepararRecorte },
                {
                  value: 'recorte',
                  label: aPrepararRecorte ? t('studio.imagem.aPreparar') : t('studio.imagem.semFundo'),
                  disabled: aPrepararRecorte,
                },
              ]}
              onPick={(m) => onFundo(m === 'recorte')}
            />
            {avatar.modo === 'recorte' && (
              <p className="st-note" data-studio="nota-recorte">
                {t('studio.imagem.notaRecorte')}
              </p>
            )}

            <span className="st-label">{t('studio.imagem.forma')}</span>
            <Escolha<FormaDoAvatar>
              label={t('studio.imagem.forma')}
              attr="data-studio-forma"
              value={avatar.forma}
              options={(['circulo', 'rectangulo'] as FormaDoAvatar[]).map((f) => ({
                value: f,
                label: f === 'circulo' ? t('studio.imagem.circulo') : t('studio.imagem.rectangulo'),
                disabled: avatar.modo === 'recorte',
                title: avatar.modo === 'recorte' ? t('studio.imagem.formaIndisponivel') : undefined,
              }))}
              onPick={(f) => onAvatar({ forma: f })}
            />
          </>
        )}
      </section>

      {temCamara && (
        <section className="st-group" data-studio-grupo="iluminacao" aria-labelledby="st-iluminacao-h">
          <header className="st-group__head">
            <h2 id="st-iluminacao-h" className="st-group__title">
              {t('studio.iluminacao.titulo')}
            </h2>
            <span className="dx-spacer" />
            <Button
              size="sm"
              variant="ghost"
              data-studio="iluminacao-repor"
              disabled={imagem.brilho === 0 && imagem.contraste === 0 && imagem.saturacao === 0}
              onClick={() => onImagem({ ...IMAGEM_INICIAL })}
            >
              {t('studio.iluminacao.repor')}
            </Button>
          </header>
          {(
            [
              ['brilho', t('studio.iluminacao.brilho')],
              ['contraste', t('studio.iluminacao.contraste')],
              ['saturacao', t('studio.iluminacao.saturacao')],
            ] as const
          ).map(([campo, rotulo]) => (
            <div key={campo}>
              <label className="st-label st-label--row" htmlFor={`st-${campo}`}>
                <span>{rotulo}</span>
                <span className="dx-num">{imagem[campo] > 0 ? `+${imagem[campo]}` : imagem[campo]}</span>
              </label>
              <input
                id={`st-${campo}`}
                className="st-range"
                type="range"
                min={-50}
                max={50}
                value={imagem[campo]}
                data-studio={`iluminacao-${campo}`}
                onChange={(e) => onImagem({ [campo]: Number(e.target.value) })}
              />
            </div>
          ))}
          <p className="st-note">{t('studio.iluminacao.nota')}</p>
        </section>
      )}
    </>
  )
}
