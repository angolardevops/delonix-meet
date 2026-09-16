/**
 * Destinos em directo (ADR-0003): um cartão por plataforma, com estado.
 *
 * O QUE O ESTADO QUER DIZER, E O QUE NÃO QUER: é UMA ligação ao servidor e um
 * só `ffmpeg` que reparte para todos os destinos. O servidor não devolve saúde
 * POR destino, por isso cada cartão mostra a fase da emissão (e se tem chave)
 * — não um débito por plataforma que ninguém mediu. O único débito no ecrã é o
 * que o browser ENVIOU, calculado a partir dos bytes que o `Directo` conta.
 */
import { ReactNode, useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Alert, Button, cx, Field, IconButton, Meter, StatusBadge, TextInput } from '../ui/kit'
import type { BadgeTone } from '../ui/kit'
import Cronometro from './Cronometro'
import { useDebito } from './debito'
import { plataformaDoUrl } from './palco'
import type { Destino, EstadoDoDirecto } from './directo'

/**
 * O débito de vídeo que o `Directo` pede ao `MediaRecorder` por omissão
 * (`opcoes.bitrate ?? 4_500_000` em `directo.ts`). Serve só de escala ao
 * medidor — o número mostrado é o medido.
 */
const DEBITO_ALVO_KBPS = 4500

function EstadoDoDestino({ fase, temChave }: { fase: EstadoDoDirecto['fase']; temChave: boolean }) {
  const { t } = useTranslation()
  let tone: BadgeTone = 'neutral'
  let texto = t('studio.directo.estados.pronto')
  if (!temChave) texto = t('studio.directo.estados.semChave')
  else if (fase === 'a-ligar') {
    tone = 'warning'
    texto = t('studio.directo.estados.aLigar')
  } else if (fase === 'no-ar') {
    tone = 'live'
    texto = t('studio.directo.estados.noAr')
  } else if (fase === 'erro') {
    tone = 'record'
    texto = t('studio.directo.estados.erro')
  }
  return <StatusBadge tone={tone}>{texto}</StatusBadge>
}

function CampoChave({ value, onChange, disabled, id }: { value: string; onChange: (v: string) => void; disabled: boolean; id: string }) {
  const { t } = useTranslation()
  const [ver, setVer] = useState(false)
  return (
    <div className="st-secret">
      {/* `type=password`: a chave é uma credencial, e quem configura o directo
          com o ecrã partilhado mostrava-a a toda a gente. */}
      <TextInput
        id={id}
        type={ver ? 'text' : 'password'}
        value={value}
        disabled={disabled}
        autoComplete="off"
        spellCheck={false}
        placeholder={t('studio.directo.chavePh')}
        data-studio="destino-chave"
        onChange={(e) => onChange(e.target.value)}
      />
      <IconButton
        icon="eye"
        label={ver ? t('studio.directo.esconderChave') : t('studio.directo.mostrarChave')}
        aria-pressed={ver}
        onClick={() => setVer((v) => !v)}
      />
    </div>
  )
}

/**
 * Um destino. Fechado mostra o que o template mostra (etiqueta, nome, servidor,
 * estado); aberto, os campos. Abre sozinho enquanto não tem chave — é o único
 * momento em que há alguma coisa para preencher — e fecha-se no ar.
 */
function CartaoDoDestino({
  d,
  i,
  fase,
  bloqueado,
  podeRemover,
  onMudar,
  onRemover,
}: {
  d: Destino
  i: number
  fase: EstadoDoDirecto['fase']
  bloqueado: boolean
  podeRemover: boolean
  onMudar: (patch: Partial<Destino>) => void
  onRemover: () => void
}) {
  const { t } = useTranslation()
  const [aEditar, setAEditar] = useState(() => !d.chave.trim())
  const nome = d.rotulo?.trim() || t('studio.directo.destino', { n: i + 1 })
  const temChave = !!d.chave.trim()
  const aberto = aEditar && !bloqueado
  return (
    <li
      className={cx('st-dest', fase === 'no-ar' && temChave && 'is-live', fase === 'erro' && temChave && 'is-error')}
      data-studio="destino"
    >
      <div className="st-dest__head">
        {/* A etiqueta lê-se do HOST do servidor RTMP, não do rótulo. */}
        <span className="st-dest__tag dx-num" data-studio="destino-tag" title={t('studio.directo.tagDica')}>
          {plataformaDoUrl(d.url, location.host)}
        </span>
        <span className="st-dest__who">
          <strong className="st-dest__name">{nome}</strong>
          <span className="st-dest__url dx-num">{d.url || 'rtmp://'}</span>
        </span>
        <EstadoDoDestino fase={fase} temChave={temChave} />
        <IconButton
          icon="edit"
          bare
          disabled={bloqueado}
          aria-expanded={aberto}
          label={t('studio.directo.editar', { rotulo: nome })}
          onClick={() => setAEditar((v) => !v)}
        />
        {podeRemover && (
          <IconButton icon="trash" bare disabled={bloqueado} label={t('studio.directo.remover', { rotulo: nome })} onClick={onRemover} />
        )}
      </div>
      {aberto && (
        <div className="st-dest__form">
          <Field label={t('studio.directo.rotulo')} htmlFor={`st-d${i}-rotulo`}>
            <TextInput
              id={`st-d${i}-rotulo`}
              value={d.rotulo ?? ''}
              disabled={bloqueado}
              autoComplete="off"
              placeholder={t('studio.directo.rotuloPh')}
              onChange={(e) => onMudar({ rotulo: e.target.value })}
            />
          </Field>
          <Field label={t('studio.directo.url')} htmlFor={`st-d${i}-url`}>
            <TextInput
              id={`st-d${i}-url`}
              value={d.url}
              disabled={bloqueado}
              autoComplete="off"
              spellCheck={false}
              placeholder="rtmp://"
              data-studio="destino-url"
              onChange={(e) => onMudar({ url: e.target.value })}
            />
          </Field>
          <Field label={t('studio.directo.chave')} htmlFor={`st-d${i}-chave`}>
            <CampoChave id={`st-d${i}-chave`} value={d.chave} disabled={bloqueado} onChange={(v) => onMudar({ chave: v })} />
          </Field>
        </div>
      )}
    </li>
  )
}

/**
 * A coluna direita do DelonixStudio: «Destinos simultâneos», a gravação local
 * (`children`) e, no fundo, «Adicionar destino» e «Parar emissão».
 */
export default function LivePanel({
  suportado,
  destinos,
  maximo,
  estado,
  podeEmitir,
  onMudar,
  onAdicionar,
  onRemover,
  onIrParaOAr,
  onParar,
  children,
}: {
  suportado: boolean
  destinos: Destino[]
  maximo: number
  estado: EstadoDoDirecto
  /** Há imagem para emitir (ecrã ou câmara). */
  podeEmitir: boolean
  onMudar: (i: number, patch: Partial<Destino>) => void
  onAdicionar: () => void
  onRemover: (i: number) => void
  onIrParaOAr: () => void
  onParar: () => void
  /** A gravação local, que no template vive por cima dos botões do fundo. */
  children?: ReactNode
}) {
  const { t } = useTranslation()
  const kbps = useDebito(estado)
  const noAr = estado.fase === 'no-ar'
  const aLigar = estado.fase === 'a-ligar'
  const bloqueado = noAr || aLigar
  const comChave = destinos.filter((d) => d.chave.trim()).length
  const desde = estado.fase === 'no-ar' ? estado.desde : 0
  const lerNoAr = useCallback(() => (Date.now() - desde) / 1000, [desde])

  return (
    <section className={cx('st-live', noAr && 'st-live--on')} data-studio="directo" aria-labelledby="st-live-h">
      <header className="st-live__head">
        <h2 id="st-live-h" className="st-live__title">
          {t('studio.directo.titulo')}
        </h2>
        <span className="dx-spacer" />
        {suportado && (
          <span className="dx-num dx-muted st-small">
            {noAr ? t('studio.directo.noArN', { n: comChave }) : t('studio.directo.comChave', { n: comChave })}
          </span>
        )}
      </header>

      <div className="st-live__scroll">
        {!suportado ? (
          <Alert tone="warning">{t('studio.directo.indisponivel')}</Alert>
        ) : (
          <>
            {estado.fase === 'no-ar' && (
              <div className="st-dest is-live" data-studio="no-ar">
                <div className="st-card__row">
                  <StatusBadge tone="live">
                    {t('studio.topo.aoVivo')} <Cronometro activo ler={lerNoAr} />
                  </StatusBadge>
                  <span className="dx-spacer" />
                  <span className="dx-num st-small" data-studio="directo-bytes">
                    {t('studio.directo.enviado')} {(estado.bytes / 1_048_576).toFixed(1)} MB
                  </span>
                </div>
                <div className="st-card__row">
                  <Meter value={(kbps / DEBITO_ALVO_KBPS) * 100} tone="live" />
                  <span className="dx-num st-small">{kbps} kbps</span>
                </div>
                <p className="st-note">{t('studio.directo.umaLigacao')}</p>
              </div>
            )}

            <ul className="st-dests">
              {destinos.map((d, i) => (
                <CartaoDoDestino
                  key={i}
                  d={d}
                  i={i}
                  fase={estado.fase}
                  bloqueado={bloqueado}
                  podeRemover={destinos.length > 1}
                  onMudar={(patch) => onMudar(i, patch)}
                  onRemover={() => onRemover(i)}
                />
              ))}
            </ul>

            {estado.fase === 'erro' && (
              <div className="dx-alert dx-alert--danger" role="alert" data-studio="directo-erro">
                {estado.motivo}
              </div>
            )}
            {destinos.length >= maximo && <p className="st-note">{t('studio.directo.limite', { maximo })}</p>}
          </>
        )}
      </div>

      {children}

      {suportado && (
        <div className="st-live__actions">
          <Button variant="secondary" disabled={bloqueado || destinos.length >= maximo} onClick={onAdicionar}>
            {t('studio.directo.adicionar')}
          </Button>
          {noAr ? (
            <Button variant="outline" className="st-btn-stop" data-studio="sair-do-ar" onClick={onParar}>
              {t('studio.directo.parar')}
            </Button>
          ) : (
            <Button
              variant="live"
              busy={aLigar}
              disabled={aLigar || comChave === 0 || !podeEmitir}
              data-studio="ir-para-o-ar"
              onClick={onIrParaOAr}
            >
              {aLigar ? t('studio.directo.aLigar') : t('studio.directo.irParaOAr')}
            </Button>
          )}
        </div>
      )}
    </section>
  )
}
