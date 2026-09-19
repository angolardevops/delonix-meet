import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { CallState } from '../callRecovery'
import { Icon, IconName } from '../ui/icons'
import { Button, Segmented, Select, Toggle, cx } from '../ui/kit'
import { Countdown } from './Clocks'
import { MenuItem, PopoverPanel, usePopover } from './Popover'
import type { LocalMedia } from './useLocalMedia'
import type { Layout } from './useLayout'
import type { Panel } from './useRoomChrome'
import { REACTION_EMOJIS } from './useReactions'

/** Um botão da barra: ícone + rótulo visível no telemóvel, dica no desktop. */
export function Ctrl({
  icon,
  label,
  onClick,
  active,
  off,
  danger,
  badge,
  className,
  pressed,
  popup,
  expanded,
  caption,
  children,
}: {
  icon: IconName
  label: string
  /** Rótulo curto visível por baixo do ícone no telemóvel («Som», «Vídeo»). */
  caption?: string
  onClick: () => void
  active?: boolean
  off?: boolean
  danger?: boolean
  badge?: ReactNode
  className?: string
  pressed?: boolean
  popup?: boolean
  expanded?: boolean
  children?: ReactNode
}) {
  return (
    <button
      type="button"
      className={cx('rm-ctrl', active && 'is-active', off && 'is-off', danger && 'is-danger', className)}
      onClick={onClick}
      aria-label={label}
      title={label}
      aria-pressed={pressed}
      aria-haspopup={popup || undefined}
      aria-expanded={popup ? expanded : undefined}
    >
      <Icon name={icon} />
      {caption && (
        <span className="rm-ctrl__caption" aria-hidden="true">
          {caption}
        </span>
      )}
      {children}
      {badge != null && <span className="rm-ctrl__badge dx-num">{badge}</span>}
    </button>
  )
}

export interface ControlBarProps {
  isHost: boolean
  topology: string
  status: string
  callState: CallState
  media: LocalMedia
  layout: Layout
  panel: Panel
  onTogglePanel: (p: Panel) => void
  onOpenSettings: () => void
  // sessão
  presenterLabel: string | null
  returnTo: string | null
  onReturnToMain: () => void
  breakoutEndsAt: number | null
  timerEndsAt: number | null
  // acções
  ccOn: boolean
  onToggleCc: () => void
  onReaction: (emoji: string) => void
  sharing: boolean
  shareNeedsPermission: boolean
  onShare: () => void
  handRaised: boolean
  onToggleHand: () => void
  recording: boolean
  recBusy: boolean
  onToggleRecording: () => void
  wbOpen: boolean
  onToggleWhiteboard: () => void
  transcribing: boolean
  unreadChat: number
  total: number
  openQuestions: number
  openPolls: number
  // mais opções
  hasPresentation: boolean
  pipDisponivel: boolean
  pipOn: boolean
  pipErro: string | null
  onTogglePip: () => void
  multicamAvailable: boolean
  onOpenMulticam: () => void
  serverRecAvailable: boolean
  serverRecOn: boolean
  onToggleServerRec: () => void
  onLeave: () => void
  // fonte de vídeo (câmara ou segunda fonte publicada como apresentação)
  /** Há uma segunda câmara que pode entrar como fonte 2 (só SFU). */
  fonte2Label: string | null
  fonte2On: boolean
  onFonte: (fonte: 'camara' | 'fonte2') => void
  // quem está na sala e à porta
  canAdmit: boolean
  waitingCount: number
  onAdmitAll: () => void
}

export function ControlBar(p: ControlBarProps) {
  const { t } = useTranslation()
  const { media, layout, pipDisponivel, pipErro } = p
  const micPop = usePopover()
  const camPop = usePopover()
  const reactPop = usePopover()
  const morePop = usePopover()

  const micLabel = media.micOn ? t('room.controlos.desligarMicrofone') : t('room.controlos.ligarMicrofone')
  const camLabel = media.camOn ? t('room.controlos.desligarCamara') : t('room.controlos.ligarCamara')
  const shareLabel = p.sharing
    ? t('room.controlos.pararPartilha')
    : p.shareNeedsPermission
      ? t('room.controlos.pedirParaPartilhar')
      : t('room.controlos.partilharEcra')

  const fecharMais = (fn: () => void) => () => {
    morePop.close()
    fn()
  }

  return (
    <footer className="rm-controls">
      <div className="rm-controls__info">
        {p.returnTo && (
          <Button size="sm" variant="outline" icon="chevronLeft" onClick={p.onReturnToMain}>
            {t('room.controlos.salaPrincipal')}
          </Button>
        )}
        {p.returnTo && p.breakoutEndsAt && (
          <span className="rm-chip dx-num" title={t('room.controlos.tempoNoGrupo')}>
            <Icon name="clock" size={12} />
            <Countdown endsAt={p.breakoutEndsAt} render={(txt) => txt} />
          </span>
        )}
        {p.timerEndsAt && (
          <Countdown
            endsAt={p.timerEndsAt}
            render={(txt, restam) => (
              <span className={cx('rm-chip dx-num', restam <= 60 && 'is-low')} title={t('room.controlos.temporizador')}>
                <Icon name="hourglass" size={12} />
                {txt}
              </span>
            )}
          />
        )}
        {p.fonte2Label && (
          <span className="rm-hide-narrow">
          <span className="rm-fonte">
            <span className="dx-eyebrow">{t('room.controlos.fonte')}</span>
            <Segmented<'camara' | 'fonte2'>
              label={t('room.controlos.fonteDeVideo')}
              value={p.fonte2On ? 'fonte2' : 'camara'}
              onChange={p.onFonte}
              options={[
                { value: 'camara', label: t('room.controlos.fonteCamara') },
                { value: 'fonte2', label: <span title={p.fonte2Label}>{t('room.controlos.fonte2')}</span> },
              ]}
            />
          </span>
          </span>
        )}
        {/* `polite`: informação, não pedido — mas quem não vê fica a saber (R104). */}
        <span className="rm-controls__status" role="status" aria-live="polite">
          {p.status}
        </span>
      </div>

      <div className="rm-controls__center">
        <div className="rm-controls__group">
          <div className="rm-split" ref={micPop.wrapRef}>
            <Ctrl icon={media.micOn ? 'mic' : 'micOff'} label={micLabel} caption={t('room.controlos.rotuloSom')} off={!media.micOn} onClick={() => void media.toggleMic()} pressed={!media.micOn} />
            <button
              type="button"
              className="rm-split__chevron"
              aria-haspopup="dialog"
              aria-expanded={micPop.open}
              aria-label={t('room.controlos.opcoesAudio')}
              title={t('room.controlos.opcoesAudio')}
              onClick={micPop.toggle}
            >
              <Icon name="chevronUp" size={12} />
            </button>
            {micPop.open && (
              <PopoverPanel label={t('room.controlos.opcoesAudio')} align="start" className="rm-devpop">
                <label className="rm-devpop__row">
                  <span>{t('room.definicoes.microfone')}</span>
                  <Select value={media.micId} onChange={(e) => void media.switchMic(e.target.value)}>
                    {media.devices.mics.length === 0 && <option value="">{t('room.definicoes.semDispositivos')}</option>}
                    {media.devices.mics.map((d, i) => (
                      <option key={d.deviceId || i} value={d.deviceId}>
                        {d.label || t('room.preEntrada.microfoneN', { n: i + 1 })}
                      </option>
                    ))}
                  </Select>
                </label>
                <label className="rm-devpop__row">
                  <span>{t('room.definicoes.altifalantes')}</span>
                  <Select value={media.speakerId} onChange={(e) => media.setSpeakerId(e.target.value)}>
                    <option value="">{t('room.preEntrada.predefinidoSistema')}</option>
                    {media.devices.speakers
                      .filter((d) => d.deviceId && d.deviceId !== 'default')
                      .map((d, i) => (
                        <option key={d.deviceId} value={d.deviceId}>
                          {d.label || t('room.preEntrada.altifalanteN', { n: i + 1 })}
                        </option>
                      ))}
                  </Select>
                </label>
                <Toggle label={t('room.definicoes.supressaoRuido')} checked={media.noiseSuppression} onChange={() => void media.toggleNoiseSuppression()} />
                <div className="rm-devpop__actions">
                  <Button size="sm" variant="outline" icon="volume" onClick={media.testSpeaker}>
                    {t('room.preEntrada.testarSom')}
                  </Button>
                  <Button size="sm" variant="ghost" icon="sliders" onClick={() => { micPop.close(); p.onOpenSettings() }}>
                    {t('room.controlos.definicoes')}
                  </Button>
                </div>
              </PopoverPanel>
            )}
          </div>
          <div className="rm-split" ref={camPop.wrapRef}>
            <Ctrl icon={media.camOn && media.hasLocalVideo ? 'video' : 'videoOff'} label={camLabel} caption={t('room.controlos.rotuloVideo')} off={!media.camOn || !media.hasLocalVideo} onClick={() => void media.toggleCam()} pressed={!media.camOn} />
            <button
              type="button"
              className="rm-split__chevron"
              aria-haspopup="dialog"
              aria-expanded={camPop.open}
              aria-label={t('room.controlos.opcoesVideo')}
              title={t('room.controlos.opcoesVideo')}
              onClick={camPop.toggle}
            >
              <Icon name="chevronUp" size={12} />
            </button>
            {camPop.open && (
              <PopoverPanel label={t('room.controlos.opcoesVideo')} align="start" className="rm-devpop">
                <label className="rm-devpop__row">
                  <span>{t('room.definicoes.camara')}</span>
                  <Select value={media.camId} onChange={(e) => void media.switchCam(e.target.value)}>
                    {media.devices.cams.length === 0 && <option value="">{t('room.definicoes.semDispositivos')}</option>}
                    {media.devices.cams.map((d, i) => (
                      <option key={d.deviceId || i} value={d.deviceId}>
                        {d.label || t('room.preEntrada.camaraN', { n: i + 1 })}
                      </option>
                    ))}
                  </Select>
                </label>
                <Toggle
                  label={t('room.definicoes.esbaterFundo')}
                  checked={media.bgMode === 'blur'}
                  disabled={media.bgBusy || !media.hasLocalVideo}
                  onChange={() => void media.applyBackground(media.bgMode === 'blur' ? 'none' : 'blur')}
                />
                <div className="rm-devpop__actions">
                  <Button size="sm" variant="ghost" icon="sparkles" onClick={() => { camPop.close(); p.onOpenSettings() }}>
                    {t('room.controlos.fundosEfeitos')}
                  </Button>
                </div>
              </PopoverPanel>
            )}
          </div>
        </div>

        <div className="rm-controls__group">
          <Ctrl icon="captions" label={p.ccOn ? t('room.controlos.desligarLegendas') : t('room.controlos.ligarLegendas')} active={p.ccOn} pressed={p.ccOn} onClick={p.onToggleCc} className="rm-hide-narrow" />
          <div className="rm-split rm-hide-narrow" ref={reactPop.wrapRef}>
            <Ctrl icon="smile" label={t('room.controlos.reaccoes')} active={reactPop.open} popup expanded={reactPop.open} onClick={reactPop.toggle} />
            {reactPop.open && (
              <PopoverPanel label={t('room.controlos.reaccoes')} className="rm-reactpop">
                {REACTION_EMOJIS.map((e) => (
                  <button
                    key={e}
                    type="button"
                    className="rm-reactpop__btn"
                    onClick={() => {
                      p.onReaction(e)
                      reactPop.close()
                    }}
                  >
                    {e}
                  </button>
                ))}
              </PopoverPanel>
            )}
          </div>
          <Ctrl icon="screen" label={shareLabel} active={p.sharing} pressed={p.sharing} onClick={p.onShare} className="rm-hide-narrow" />
          {/* No telemóvel o chat vive na barra, como no template; no desktop, à direita. */}
          <Ctrl
            icon="chat"
            label={t('room.painel.chat')}
            active={p.panel === 'chat'}
            pressed={p.panel === 'chat'}
            onClick={() => p.onTogglePanel('chat')}
            badge={p.unreadChat > 0 ? (p.unreadChat > 9 ? '9+' : p.unreadChat) : undefined}
            className="rm-only-narrow"
            caption={t('room.painel.chat')}
          />
          <Ctrl icon="hand" label={p.handRaised ? t('room.controlos.baixarMao') : t('room.controlos.levantarMao')} caption={t('room.controlos.rotuloMao')} active={p.handRaised} pressed={p.handRaised} onClick={p.onToggleHand} />
          <Ctrl
            icon={p.recording ? 'stop' : 'record'}
            label={p.recording ? t('room.controlos.pararGravacao') : t('room.controlos.gravar')}
            active={p.recording}
            danger={p.recording}
            pressed={p.recording}
            onClick={p.onToggleRecording}
            className="rm-hide-narrow"
          />
          <div className="rm-split" ref={morePop.wrapRef}>
            <Ctrl icon="more" label={t('room.controlos.maisOpcoes')} caption={t('room.controlos.rotuloMais')} active={morePop.open} popup expanded={morePop.open} onClick={morePop.toggle} />
            {morePop.open && (
              <PopoverPanel label={t('room.controlos.maisOpcoes')} role="menu" align="end" className="rm-menu">
                {/* Numa coluna estreita (telemóvel, ou painel aberto) a barra só leva o essencial; o resto vive aqui. */}
                <div className="rm-only-narrow rm-menu__extra">
                  <MenuItem icon={<Icon name="captions" />} checked={p.ccOn} onClick={fecharMais(p.onToggleCc)}>
                    {t('room.controlos.legendas')}
                  </MenuItem>
                  <MenuItem icon={<Icon name="screen" />} checked={p.sharing} onClick={fecharMais(p.onShare)}>
                    {shareLabel}
                  </MenuItem>
                  <MenuItem icon={<Icon name={p.recording ? 'stop' : 'record'} />} checked={p.recording} disabled={p.recBusy} onClick={fecharMais(p.onToggleRecording)}>
                    {t('room.controlos.gravar')}
                  </MenuItem>
                  <MenuItem icon={<Icon name="board" />} checked={p.wbOpen} onClick={fecharMais(p.onToggleWhiteboard)}>
                    {t('room.controlos.quadro')}
                  </MenuItem>
                  <MenuItem icon={<Icon name="notes" />} checked={p.panel === 'notes'} onClick={fecharMais(() => p.onTogglePanel('notes'))}>
                    {t('room.controlos.notas')}
                  </MenuItem>
                  <MenuItem icon={<Icon name="question" />} checked={p.panel === 'qa'} onClick={fecharMais(() => p.onTogglePanel('qa'))}>
                    {t('room.painel.perguntas')}
                  </MenuItem>
                  <MenuItem icon={<Icon name="poll" />} checked={p.panel === 'polls'} onClick={fecharMais(() => p.onTogglePanel('polls'))}>
                    {t('room.painel.sondagens')}
                  </MenuItem>
                  <MenuItem icon={<Icon name="people" />} checked={p.panel === 'people'} onClick={fecharMais(() => p.onTogglePanel('people'))}>
                    {t('room.painel.participantes')}
                  </MenuItem>
                  <div className="rm-menu__reactions" role="group" aria-label={t('room.controlos.reaccoes')}>
                    {REACTION_EMOJIS.map((e) => (
                      <button key={e} type="button" className="rm-reactpop__btn" onClick={fecharMais(() => p.onReaction(e))}>
                        {e}
                      </button>
                    ))}
                  </div>
                  <div className="rm-menu__sep" />
                </div>
                <MenuItem icon={<Icon name="grid" />} checked={layout.effectiveViewMode === 'grid' && !p.hasPresentation} onClick={fecharMais(() => { layout.setViewMode('grid'); layout.setPinnedId(null) })}>
                  {t('room.topo.grelha')}
                </MenuItem>
                <MenuItem icon={<Icon name="user" />} checked={layout.effectiveViewMode === 'stage' && !p.hasPresentation} onClick={fecharMais(() => layout.setViewMode('stage'))}>
                  {t('room.topo.orador')}
                </MenuItem>
                {p.hasPresentation && (
                  <>
                    <MenuItem icon={<Icon name="columns" />} checked={layout.presLayout === 'side'} onClick={fecharMais(() => layout.setPresLayout('side'))}>
                      {t('room.controlos.plateiaAoLado')}
                    </MenuItem>
                    <MenuItem icon={<Icon name="rows" />} checked={layout.presLayout === 'bottom'} onClick={fecharMais(() => layout.setPresLayout('bottom'))}>
                      {t('room.controlos.plateiaEmBaixo')}
                    </MenuItem>
                  </>
                )}
                <div className="rm-menu__sep" />
                <MenuItem icon={<Icon name="maximize" />} checked={layout.fullscreen} onClick={fecharMais(layout.toggleFullscreen)}>
                  {t('room.controlos.ecraInteiro')}
                </MenuItem>
                {pipDisponivel && (
                  <MenuItem icon={<Icon name="pip" />} checked={p.pipOn} onClick={fecharMais(p.onTogglePip)}>
                    {t('room.pip.janelaFlutuante')}
                  </MenuItem>
                )}
                <MenuItem icon={<Icon name="video" />} checked={!layout.hideSelf} onClick={() => layout.setHideSelf((v) => !v)}>
                  {t('room.controlos.mostrarMeuVideo')}
                </MenuItem>
                <MenuItem icon={<Icon name="videoOff" />} checked={!layout.hideNoVideo} onClick={() => layout.setHideNoVideo((v) => !v)}>
                  {t('room.controlos.mostrarSemVideo')}
                </MenuItem>
                <MenuItem icon={<Icon name="cube" />} checked={media.parallax} onClick={() => void media.toggleParallax()}>
                  {t('room.controlos.sala3d')}
                </MenuItem>
                <div className="rm-menu__sep" />
                <MenuItem icon={<Icon name="sparkles" />} onClick={fecharMais(p.onOpenSettings)}>
                  {t('room.controlos.fundosEfeitos')}
                </MenuItem>
                {p.multicamAvailable && (
                  <MenuItem icon={<Icon name="layers" />} onClick={fecharMais(p.onOpenMulticam)}>
                    {t('room.multicam.titulo')}
                  </MenuItem>
                )}
                {p.serverRecAvailable && (
                  <MenuItem icon={<Icon name="server" />} checked={p.serverRecOn} onClick={fecharMais(p.onToggleServerRec)}>
                    {p.serverRecOn ? t('room.controlos.pararGravacaoServidor') : t('room.controlos.gravarNoServidor')}
                  </MenuItem>
                )}
              </PopoverPanel>
            )}
          </div>
        </div>
        <div className="rm-controls__group rm-controls__group--panels">
          <Ctrl icon="board" label={t('room.controlos.quadro')} active={p.wbOpen} pressed={p.wbOpen} onClick={p.onToggleWhiteboard} />
          <Ctrl icon="notes" label={t('room.controlos.notas')} active={p.panel === 'notes'} pressed={p.panel === 'notes'} onClick={() => p.onTogglePanel('notes')}>
            {p.transcribing && <span className="rm-ctrl__live" aria-hidden="true" />}
          </Ctrl>
          <Ctrl icon="question" label={t('room.painel.perguntas')} active={p.panel === 'qa'} pressed={p.panel === 'qa'} onClick={() => p.onTogglePanel('qa')} badge={p.openQuestions || undefined} />
          <Ctrl icon="poll" label={t('room.painel.sondagens')} active={p.panel === 'polls'} pressed={p.panel === 'polls'} onClick={() => p.onTogglePanel('polls')} badge={p.openPolls || undefined} />
          <Ctrl icon="people" label={t('room.painel.participantes')} active={p.panel === 'people'} pressed={p.panel === 'people'} onClick={() => p.onTogglePanel('people')} badge={p.total} />
          <Ctrl
            icon="chat"
            label={t('room.painel.chat')}
            active={p.panel === 'chat'}
            pressed={p.panel === 'chat'}
            onClick={() => p.onTogglePanel('chat')}
            badge={p.unreadChat > 0 ? (p.unreadChat > 9 ? '9+' : p.unreadChat) : undefined}
          />
        </div>
        <button type="button" className="rm-ctrl rm-ctrl--hangup" onClick={p.onLeave} aria-label={t('room.controlos.sair')} title={t('room.controlos.sair')}>
          <Icon name="phoneOff" />
          <span className="rm-ctrl__caption" aria-hidden="true">
            {t('room.controlos.rotuloSair')}
          </span>
        </button>
      </div>

      {/* No telemóvel os seletores de dispositivo não cabem na barra: um botão só, ao alcance do polegar. */}
      <button type="button" className="rm-devices-narrow" onClick={p.onOpenSettings}>
        <Icon name="sliders" size={14} />
        {t('room.controlos.mudarDispositivos')}
      </button>

      {/* A recusa da janela flutuante tem de chegar ao ecrã, mesmo com o menu fechado. */}
      {pipErro && (
        <span className="rm-controls__piperr" role="status">
          {pipErro}
        </span>
      )}

      <div className="rm-controls__side">
        {/* Ocupação e fila de espera (template DelonixRoomGrid, canto inferior direito). */}
        <span className="rm-occupancy">
          <span className="dx-num">
            {p.canAdmit && p.waitingCount > 0
              ? t('room.controlos.naSalaEspera', { naSala: p.total, espera: p.waitingCount })
              : t('room.controlos.naSala', { count: p.total })}
          </span>
          {p.canAdmit && p.waitingCount > 0 && (
            <button type="button" className="rm-occupancy__admit rm-occupancy__waiting" onClick={p.onAdmitAll} aria-label={t('room.avisos.admitirTodos', { count: p.waitingCount })}>
              {t('room.avisos.admitir')}
            </button>
          )}
        </span>
      </div>
    </footer>
  )
}
