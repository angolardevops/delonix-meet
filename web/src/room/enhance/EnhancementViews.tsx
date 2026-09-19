import { useState, useSyncExternalStore } from 'react'
import { useTranslation } from 'react-i18next'
import { bestLayer, type SentLayer } from '../../media/sendProfile'
import { Icon } from '../../ui/icons'
import { Alert, Button, IconButton, Toggle } from '../../ui/kit'
import '../../ui/enhance.css'
import type { StageEnhancements } from './useStageEnhancements'
import type { Store } from './target'

function useStore<T>(store: Store<T>): T {
  return useSyncExternalStore(store.subscribe, store.get, store.get)
}

const fmtMs = (ms: number) => ms.toLocaleString(undefined, { maximumFractionDigits: 1 })

type TFn = (k: string, o?: Record<string, unknown>) => string

/** Porque é que o orçamento desligou o efeito: tempo por frame, ou ritmo. */
function budgetText(t: TFn, area: 'rececao' | 'imersivo', off: { ms: number | null; fps?: number; src?: number; lagging?: boolean } | null) {
  if (off?.lagging) return t(`nitidez.${area}.offRitmo`, { fps: off.fps ?? 0, src: off.src ?? 0 })
  return t(`nitidez.${area}.offBudget`, { ms: fmtMs(off?.ms ?? 0) })
}

/* ------------------------------------------------------------------ */
/* Definições (entra no fim do painel de definições da sala)           */
/* ------------------------------------------------------------------ */

export function EnhancementSettings({
  enh,
  viewMode,
  onSpeakerView,
}: {
  enh: StageEnhancements
  viewMode: 'grid' | 'stage'
  onSpeakerView: () => void
}) {
  const { t } = useTranslation()
  return (
    <>
      <SendSection enh={enh} />
      <ReceiveSection enh={enh} />
      <section className="rm-block" aria-labelledby="enh-imm-h" data-enh="imersivo">
        <h3 id="enh-imm-h" className="rm-block__title">
          <Icon name="cube" size={13} />
          {t('nitidez.imersivo.titulo')}
        </h3>
        <Toggle
          label={t('nitidez.imersivo.interruptor')}
          hint={t('nitidez.imersivo.dica')}
          checked={enh.immersive.wanted}
          onChange={enh.immersive.toggle}
          data-enh-toggle="imersivo"
        />
        {enh.immersive.wanted && (
          <>
            <ImmersiveStatus enh={enh} viewMode={viewMode} onSpeakerView={onSpeakerView} />
            <Toggle
              label={t('nitidez.imersivo.seguirCabeca')}
              hint={t('nitidez.imersivo.seguirCabecaDica')}
              checked={enh.immersive.followHead}
              onChange={(e) => enh.immersive.setFollowHead(e.target.checked)}
              data-enh-toggle="cabeca"
            />
          </>
        )}
      </section>
    </>
  )
}

function layerText(t: (k: string, o?: Record<string, unknown>) => string, l: SentLayer | null) {
  return l ? t('nitidez.envio.medida', { w: l.width, h: l.height, fps: l.fps }) : t('nitidez.envio.semAmostra')
}

function SendSection({ enh }: { enh: StageEnhancements }) {
  const { t } = useTranslation()
  const { send } = enh
  const view = useStore(send.store)
  const antes = bestLayer(view.before)
  const agora = bestLayer(view.now)
  return (
    <section className="rm-block" aria-labelledby="enh-send-h" data-enh="envio" data-perfil={send.active}>
      <h3 id="enh-send-h" className="rm-block__title">
        <Icon name="video" size={13} />
        {t('nitidez.envio.titulo')}
      </h3>
      <Toggle label={t('nitidez.envio.interruptor')} hint={t('nitidez.envio.dica')} checked={send.wanted} onChange={send.toggle} data-enh-toggle="envio" />
      {send.wanted && (
        <>
          {send.refused && <Alert tone="warning">{t('nitidez.envio.recusado')}</Alert>}
          {send.reason ? (
            <Alert tone="warning">{t('nitidez.envio.pausa', { motivo: t(`nitidez.envio.motivo_${send.reason}`) })}</Alert>
          ) : (
            <p className="dx-muted" role="status">
              {send.active === 'sharp' ? t('nitidez.envio.activo') : t('nitidez.envio.aVerificar')}
            </p>
          )}
          {view.now == null && view.before == null ? (
            <p className="dx-muted">{t('nitidez.envio.semEnvio')}</p>
          ) : (
            <>
              <dl className="enh-measure dx-num">
                <dt>{t('nitidez.envio.antes')}</dt>
                <dd data-medida="antes">{layerText(t, antes)}</dd>
                <dt>{t('nitidez.envio.agora')}</dt>
                <dd data-medida="agora">{layerText(t, agora)}</dd>
              </dl>
              {view.now && view.now.layers.length > 0 && (
                <div>
                  <span className="dx-eyebrow">{t('nitidez.envio.camadas')}</span>
                  <ul className="enh-layers dx-num">
                    {view.now.layers.map((l) => (
                      <li key={l.rid || `${l.width}`}>
                        {t('nitidez.envio.camada', { rid: l.rid || '-', w: l.width, h: l.height, fps: l.fps })}
                        {l.limitedBy !== 'none' && <span className="dx-muted"> · {t(`nitidez.envio.limitado_${l.limitedBy === 'bandwidth' || l.limitedBy === 'cpu' ? l.limitedBy : 'other'}`)}</span>}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              {view.now?.availableUpKbps != null && <p className="dx-muted dx-num">{t('nitidez.envio.banda', { kbps: view.now.availableUpKbps })}</p>}
            </>
          )}
          <p className="dx-muted">{t('nitidez.envio.nota')}</p>
        </>
      )}
    </section>
  )
}

function ReceiveSection({ enh }: { enh: StageEnhancements }) {
  const { t } = useTranslation()
  const { receive, immersive } = enh
  const stats = useStore(receive.stats)
  const tgt = receive.target
  let offMsg: string | null = null
  if (receive.off?.why === 'budget') offMsg = budgetText(t, 'rececao', receive.off)
  else if (receive.off?.why === 'lost') offMsg = t('nitidez.rececao.offLost')
  else if (receive.off?.why === 'unsupported') offMsg = t('nitidez.rececao.offUnsupported')
  const pausedByImmersive = receive.wanted && !receive.running && !receive.off && !!tgt && immersive.running
  return (
    <section className="rm-block" aria-labelledby="enh-rx-h" data-enh="realce">
      <h3 id="enh-rx-h" className="rm-block__title">
        <Icon name="eye" size={13} />
        {t('nitidez.rececao.titulo')}
      </h3>
      <Toggle label={t('nitidez.rececao.interruptor')} hint={t('nitidez.rececao.dica')} checked={receive.wanted} onChange={receive.toggle} data-enh-toggle="realce" />
      <div className="enh-strength">
        <label htmlFor="enh-rx-strength" className="dx-muted">
          {t('nitidez.rececao.intensidade')}
        </label>
        <input
          id="enh-rx-strength"
          type="range"
          min={0}
          max={100}
          step={5}
          value={receive.strength}
          disabled={!receive.wanted}
          aria-valuetext={t('nitidez.rececao.intensidadeValor', { n: receive.strength })}
          onChange={(e) => receive.setStrength(Number(e.target.value))}
        />
        <span className="dx-num dx-muted">{t('nitidez.rececao.intensidadeValor', { n: receive.strength })}</span>
      </div>
      {receive.wanted && (
        <>
          {offMsg ? (
            <Alert tone="warning">{offMsg}</Alert>
          ) : pausedByImmersive ? (
            <p className="dx-muted">{t('nitidez.rececao.offImersivo')}</p>
          ) : !tgt ? (
            <p className="dx-muted">{t('nitidez.rececao.alvoNenhum')}</p>
          ) : (
            <p className="dx-muted" role="status">
              {tgt.kind === 'presentation' ? t('nitidez.rececao.alvoApresentacao') : t('nitidez.rececao.alvoPessoa', { nome: tgt.name })}
              {stats && <span className="dx-num"> {t('nitidez.rececao.custo', { fps: stats.fps, ms: fmtMs(stats.p95Ms) })}</span>}
            </p>
          )}
        </>
      )}
      <p className="dx-muted">{t('nitidez.rececao.soEcra')}</p>
    </section>
  )
}

function ImmersiveStatus({ enh, viewMode, onSpeakerView }: { enh: StageEnhancements; viewMode: 'grid' | 'stage'; onSpeakerView: () => void }) {
  const { t } = useTranslation()
  const { immersive } = enh
  const stats = useStore(immersive.stats)
  if (immersive.block) return <Alert tone="warning">{t(`nitidez.imersivo.bloqueio_${immersive.block}`)}</Alert>
  const off = immersive.off
  if (off) {
    if (off.why === 'budget') return <Alert tone="warning">{budgetText(t, 'imersivo', off)}</Alert>
    const k = { lost: 'offLost', unsupported: 'offUnsupported', segmenter: 'offSegmenter' }[off.why]
    return <Alert tone="warning">{t(`nitidez.imersivo.${k}`)}</Alert>
  }
  if (!immersive.target) {
    return (
      <>
        <p className="dx-muted">{t('nitidez.imersivo.precisaOrador')}</p>
        {viewMode === 'grid' && (
          <Button size="sm" variant="outline" icon="rows" onClick={onSpeakerView}>
            {t('nitidez.imersivo.vistaOrador')}
          </Button>
        )}
      </>
    )
  }
  return (
    <p className="dx-muted" role="status">
      {t(`nitidez.imersivo.fonte_${stats?.source ?? 'pointer'}`)}
      {stats && (
        <span className="dx-num">
          {' '}
          {t('nitidez.imersivo.custo', { fps: stats.fps, ms: fmtMs(stats.p95Ms), seg: fmtMs(stats.segMs) })} ·{' '}
          {stats.gpu ? t('nitidez.imersivo.recorteGpu') : t('nitidez.imersivo.recorteCpu')}
        </span>
      )}
    </p>
  )
}

/* ------------------------------------------------------------------ */
/* Por cima do palco: o que está a correr, e a sugestão ao anfitrião   */
/* ------------------------------------------------------------------ */

export function StageEnhancementsLayer({
  enh,
  suggestImmersive,
  roomCode,
}: {
  enh: StageEnhancements
  /** Anfitrião numa sala `training`, sem apresentação a decorrer. */
  suggestImmersive: boolean
  roomCode: string
}) {
  const { t } = useTranslation()
  const { receive, immersive } = enh
  const rxStats = useStore(receive.stats)
  const immStats = useStore(immersive.stats)
  const dismissKey = `dx_sugestao_imersivo_${roomCode}`
  const [dismissed, setDismissed] = useState(() => {
    try {
      return sessionStorage.getItem(dismissKey) === '1'
    } catch {
      return false
    }
  })
  const dismiss = () => {
    setDismissed(true)
    try {
      sessionStorage.setItem(dismissKey, '1')
    } catch {
      /* sem armazenamento: vale até sair */
    }
  }
  const [noticeSeen, setNoticeSeen] = useState<string | null>(null)
  const autoOff = immersive.off?.why === 'budget' ? 'imersivo' : receive.off?.why === 'budget' ? 'realce' : null
  const autoOffInfo = immersive.off?.why === 'budget' ? immersive.off : receive.off

  const showHud = (immersive.running && immStats) || (receive.running && rxStats) || (autoOff && noticeSeen !== autoOff)
  return (
    <>
      {showHud && (
        <div className="enh-hud" role="group" aria-label={t('nitidez.hud.rotulo')}>
          {immersive.running && immStats && (
            <span className="enh-pill" data-hud="imersivo">
              <Icon name="cube" size={12} />
              <span className="enh-pill__name">{t('nitidez.imersivo.activo')}</span>
              <span className="enh-pill__cost dx-num">{t('nitidez.imersivo.custo', { fps: immStats.fps, ms: fmtMs(immStats.p95Ms), seg: fmtMs(immStats.segMs) })}</span>
              <IconButton icon="x" bare label={t('nitidez.imersivo.desligar')} onClick={immersive.toggle} />
            </span>
          )}
          {receive.running && rxStats && (
            <span className="enh-pill" data-hud="realce">
              <Icon name="eye" size={12} />
              <span className="enh-pill__name">{t('nitidez.hud.realce')}</span>
              <span className="enh-pill__cost dx-num">{t('nitidez.rececao.custo', { fps: rxStats.fps, ms: fmtMs(rxStats.p95Ms) })}</span>
              <Button size="sm" variant="ghost" aria-pressed={receive.compare} onClick={() => receive.setCompare(!receive.compare)}>
                {t('nitidez.rececao.verOriginal')}
              </Button>
              <IconButton icon="x" bare label={t('nitidez.hud.desligarRealce')} onClick={receive.toggle} />
            </span>
          )}
          {autoOff && noticeSeen !== autoOff && (
            <span className="enh-pill enh-pill--notice" role="status" data-hud="aviso">
              <Icon name="alert" size={12} />
              <span>{budgetText(t, autoOff === 'imersivo' ? 'imersivo' : 'rececao', autoOffInfo)}</span>
              <IconButton icon="x" bare label={t('nitidez.hud.fecharAviso')} onClick={() => setNoticeSeen(autoOff)} />
            </span>
          )}
        </div>
      )}
      {suggestImmersive && !dismissed && !immersive.wanted && (
        <aside className="enh-suggest" aria-labelledby="enh-suggest-h" data-enh="sugestao">
          <h3 id="enh-suggest-h">
            <Icon name="cube" size={14} />
            {t('nitidez.imersivo.sugestaoTitulo')}
          </h3>
          <p className="dx-muted">{t('nitidez.imersivo.sugestaoTexto')}</p>
          <div className="enh-suggest__actions">
            <Button
              size="sm"
              variant="primary"
              onClick={() => {
                immersive.toggle()
                dismiss()
              }}
            >
              {t('nitidez.imersivo.sugestaoLigar')}
            </Button>
            <Button size="sm" variant="ghost" onClick={dismiss}>
              {t('nitidez.imersivo.sugestaoFechar')}
            </Button>
          </div>
        </aside>
      )}
    </>
  )
}
