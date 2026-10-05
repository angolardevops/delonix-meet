/**
 * Coluna esquerda do Estúdio: as FONTES e a composição — o ecrã (inteiro ou
 * uma região) e a tua imagem (câmara, posição, tamanho, fundo, forma).
 *
 * Não tem estado próprio: o estado é da página, que é quem fala com o
 * compositor. Isto só desenha e devolve intenções.
 */
import { useTranslation } from 'react-i18next'
import { Button, cx, Segmented, Select } from '../ui/kit'
import { Grupo } from './Grupo'
import type { Camara } from './usePalco'
import type { CantoDoAvatar, EstadoDaImagem, EstadoDoAvatar, FormaDoAvatar, Recorte } from './compositor'
import { IMAGEM_INICIAL } from './compositor'

/** Ordem fixa: 0 = superior-esquerdo … 3 = inferior-direito (o e2e conta com isto). */
const CANTOS: { key: Exclude<CantoDoAvatar, 'livre'>; i18n: string }[] = [
  { key: 'superior-esquerdo', i18n: 'superiorEsquerdo' },
  { key: 'superior-direito', i18n: 'superiorDireito' },
  { key: 'inferior-esquerdo', i18n: 'inferiorEsquerdo' },
  { key: 'inferior-direito', i18n: 'inferiorDireito' },
]

export default function SourcesPanel({
  temEcra,
  temCamara,
  recorte,
  avatar,
  imagem,
  aPrepararRecorte,
  cameras,
  camara,
  onEscolherEcra,
  onEcraInteiro,
  onAbrirRegiao,
  onAlternarCamara,
  onEscolherCamara,
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
  cameras: Camara[]
  camara: string
  onEscolherEcra: () => void
  onEcraInteiro: () => void
  onAbrirRegiao: () => void
  onAlternarCamara: () => void
  onEscolherCamara: (id: string) => void
  onAvatar: (patch: Partial<EstadoDoAvatar>) => void
  onImagem: (patch: Partial<EstadoDaImagem>) => void
  onFundo: (semFundo: boolean) => void
}) {
  const { t } = useTranslation()
  const parcial = recorte.w < 1 || recorte.h < 1

  return (
    <>
      <Grupo grupo="fonte" titulo={t('studio.fonte.titulo')}>
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
            <Segmented
              className="st-seg"
              label={t('studio.fonte.enquadramento')}
              dataKey="studio-regiao"
              value={parcial ? 'regiao' : 'tudo'}
              options={[
                { value: 'tudo', label: t('studio.fonte.tudo') },
                { value: 'regiao', label: t('studio.fonte.regiao') },
              ]}
              onChange={(v) => (v === 'tudo' ? onEcraInteiro() : onAbrirRegiao())}
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
      </Grupo>

      <Grupo grupo="imagem" titulo={t('studio.imagem.titulo')}>
        <Button
          variant={temCamara ? 'secondary' : 'primary'}
          icon={temCamara ? 'videoOff' : 'video'}
          block
          data-studio="camara"
          onClick={onAlternarCamara}
        >
          {temCamara ? t('studio.imagem.desligar') : t('studio.imagem.ligar')}
        </Button>

        {cameras.length > 1 && (
          <>
            <label className="st-label" htmlFor="st-camara-dispositivo">
              {t('studio.imagem.dispositivo')}
            </label>
            <Select
              id="st-camara-dispositivo"
              data-studio="camara-dispositivo"
              value={camara}
              onChange={(e) => onEscolherCamara(e.target.value)}
            >
              <option value="">{t('studio.imagem.dispositivoOmissao')}</option>
              {cameras
                .filter((c) => c.id)
                .map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.nome}
                  </option>
                ))}
            </Select>
          </>
        )}

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
            <Segmented
              className="st-seg"
              label={t('studio.imagem.fundo')}
              dataKey="studio-modo"
              value={avatar.modo}
              options={[
                { value: 'bolha', label: t('studio.imagem.comFundo'), disabled: aPrepararRecorte },
                {
                  value: 'recorte',
                  label: aPrepararRecorte ? t('studio.imagem.aPreparar') : t('studio.imagem.semFundo'),
                  disabled: aPrepararRecorte,
                },
              ]}
              onChange={(m) => onFundo(m === 'recorte')}
            />
            {avatar.modo === 'recorte' && (
              <p className="st-note" data-studio="nota-recorte">
                {t('studio.imagem.notaRecorte')}
              </p>
            )}

            <span className="st-label">{t('studio.imagem.forma')}</span>
            <Segmented<FormaDoAvatar>
              className="st-seg"
              label={t('studio.imagem.forma')}
              dataKey="studio-forma"
              value={avatar.forma}
              options={(['circulo', 'rectangulo'] as FormaDoAvatar[]).map((f) => ({
                value: f,
                label: f === 'circulo' ? t('studio.imagem.circulo') : t('studio.imagem.rectangulo'),
                disabled: avatar.modo === 'recorte',
                title: avatar.modo === 'recorte' ? t('studio.imagem.formaIndisponivel') : undefined,
              }))}
              onChange={(f) => onAvatar({ forma: f })}
            />
          </>
        )}
      </Grupo>

      {temCamara && (
        <Grupo
          grupo="iluminacao"
          titulo={t('studio.iluminacao.titulo')}
          accao={
            <Button
              size="sm"
              variant="ghost"
              data-studio="iluminacao-repor"
              disabled={imagem.brilho === 0 && imagem.contraste === 0 && imagem.saturacao === 0}
              onClick={() => onImagem({ ...IMAGEM_INICIAL })}
            >
              {t('studio.iluminacao.repor')}
            </Button>
          }
        >
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
        </Grupo>
      )}
    </>
  )
}
