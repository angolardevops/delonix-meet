/**
 * Início — a porta de entrada da consola, na grelha do DelonixHome: barra com
 * saudação, pesquisa e definições; quatro mosaicos de acção; próximas
 * reuniões; gravações recentes; coluna direita de 336 px.
 *
 * As quatro acções rápidas vivem no CORPO da página em todas as larguras
 * (lote2 3.1.4, R103): no telemóvel, começar ou entrar numa reunião não pode
 * ficar atrás de um toque no menu. Entrar por código aceita o código solto ou
 * o link colado, pelo mesmo parser da paleta de comandos.
 *
 * O template não tem a linha de opções da reunião instantânea (sala de espera,
 * E2EE, formato de treino). As opções existem e não se perdem: vivem num
 * menu no canto do mosaico «Iniciar agora».
 */
import { FormEvent, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, apiErrorMessage, createRoom, getRoom } from '../api'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { parseRoomCode } from '../roomCode'
import { Icon } from '../ui/icons'
import { Alert, cx } from '../ui/kit'
import '../ui/home.css'
import { calendarHash } from './calendar/dates'
import { Greeting, TodayStamp } from './home/Clock'
import RecentRecordings from './home/RecentRecordings'
import SideColumn from './home/SideColumn'
import Upcoming from './home/Upcoming'
import { useOdooCalendar } from './home/useOdooCalendar'

type Format = 'normal' | 'training'

export default function Home() {
  const { t } = useTranslation()
  const { user, enterRoom, navigate, openPalette, openSettings } = useShell()
  const [creating, setCreating] = useState(false)
  const [waitingRoom, setWaitingRoom] = useState(false)
  const [e2ee, setE2ee] = useState(false)
  const [format, setFormat] = useState<Format>('normal')
  const [optionsOpen, setOptionsOpen] = useState(false)
  const [startErr, setStartErr] = useState('')
  const [code, setCode] = useState('')
  const [joining, setJoining] = useState(false)
  const [joinErr, setJoinErr] = useState('')
  const odooCalendar = useOdooCalendar()
  const optionsRef = useRef<HTMLDivElement>(null)

  // O menu de opções fecha com Esc ou com um clique fora dele.
  useEffect(() => {
    if (!optionsOpen) return
    const onDown = (e: MouseEvent) => {
      if (!optionsRef.current?.contains(e.target as Node)) setOptionsOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOptionsOpen(false)
    }
    window.addEventListener('mousedown', onDown)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('mousedown', onDown)
      window.removeEventListener('keydown', onKey)
    }
  }, [optionsOpen])

  async function startNow() {
    setStartErr('')
    setCreating(true)
    try {
      const name =
        format === 'training'
          ? t('home.iniciar.nomeTreino', { nome: user.username })
          : t('home.iniciar.nomeReuniao', { nome: user.username })
      const room = await createRoom(name, 'sfu', waitingRoom, e2ee, format)
      enterRoom(room.code)
    } catch (e) {
      setStartErr(apiErrorMessage(e, t('home.iniciar.erro')))
      setCreating(false)
    }
  }

  async function join(e: FormEvent) {
    e.preventDefault()
    setJoinErr('')
    const parsed = parseRoomCode(code)
    if (!parsed) {
      setJoinErr(t('home.entrar.codigoInvalido'))
      return
    }
    setJoining(true)
    try {
      // Confirma que a sala existe antes de sair da consola: um código errado
      // diz-se aqui, e não num ecrã de sala vazio.
      const room = await getRoom(parsed)
      enterRoom(room.code)
    } catch (err) {
      setJoinErr(
        err instanceof ApiError && err.status === 404
          ? t('home.entrar.naoEncontrada', { codigo: parsed })
          : apiErrorMessage(err, t('home.entrar.erro')),
      )
      setJoining(false)
    }
  }

  const optionsOn = [waitingRoom, e2ee, format === 'training'].filter(Boolean).length

  return (
    <>
      <PageBar title={<Greeting name={user.username} />} meta={<TodayStamp />}>
        <button type="button" className="home-search" onClick={openPalette} data-testid="home-search">
          <span>{t('consola.inicio.pesquisar')}</span>
        </button>
        <button
          type="button"
          className="dx-iconbtn home-gear"
          onClick={() => openSettings('account')}
          aria-label={t('shell.definicoes')}
          title={t('shell.definicoes')}
        >
          <Icon name="sliders" />
        </button>
      </PageBar>
      <div className="page home">
        <div className="home-grid">
          <div className="home-main">
            <section className="home-actions" aria-label={t('home.accoes.rotulo')}>
              <div className="quick-actions">
                <div className={cx('qa-tile qa-tile--primary', creating && 'qa-tile--busy')} ref={optionsRef}>
                  <button
                    type="button"
                    className="qa-tile__hit"
                    disabled={creating}
                    aria-busy={creating || undefined}
                    onClick={() => void startNow()}
                  >
                    {creating ? <span className="dx-spinner" aria-hidden="true" /> : <Icon name="play" size={14} />}
                    <span className="qa-tile__text">
                      <strong>{t('home.accoes.iniciar')}</strong>
                      <small>{creating ? t('home.accoes.aCriar') : t('consola.inicio.iniciarSub')}</small>
                    </span>
                  </button>
                  <button
                    type="button"
                    className="qa-tile__corner"
                    aria-expanded={optionsOpen}
                    aria-label={t('home.iniciar.opcoes')}
                    title={t('home.iniciar.opcoes')}
                    onClick={() => setOptionsOpen((o) => !o)}
                  >
                    <Icon name="sliders" size={13} />
                    {optionsOn > 0 && <span className="qa-tile__count dx-num">{optionsOn}</span>}
                  </button>
                  {optionsOpen && (
                    <div className="qa-menu" role="group" aria-label={t('home.iniciar.opcoes')}>
                      <label className="qa-menu__row" title={t('home.iniciar.salaEsperaDica')}>
                        <input type="checkbox" checked={waitingRoom} onChange={() => setWaitingRoom((v) => !v)} />
                        <Icon name="door" size={13} />
                        <span>{t('home.iniciar.salaEspera')}</span>
                      </label>
                      <label className="qa-menu__row" title={t('home.iniciar.e2eeDica')}>
                        <input type="checkbox" checked={e2ee} onChange={() => setE2ee((v) => !v)} />
                        <Icon name="lock" size={13} />
                        <span>{t('home.iniciar.e2ee')}</span>
                      </label>
                      <label className="qa-menu__row" title={t('home.iniciar.treinoDica')}>
                        <input
                          type="checkbox"
                          checked={format === 'training'}
                          onChange={() => setFormat((f) => (f === 'training' ? 'normal' : 'training'))}
                        />
                        <Icon name="grid" size={13} />
                        <span>{t('home.iniciar.treino')}</span>
                      </label>
                    </div>
                  )}
                </div>

                <button type="button" className="qa-tile" onClick={() => (location.hash = calendarHash.schedule())}>
                  <Icon name="calendar" size={14} />
                  <span className="qa-tile__text">
                    <strong>{t('home.accoes.agendar')}</strong>
                    <small>{odooCalendar ? t('consola.inicio.agendarOdoo') : t('home.accoes.agendarSub')}</small>
                  </span>
                </button>

                <form className={cx('qa-tile qa-tile--join', joinErr && 'qa-tile--error')} onSubmit={join}>
                  <Icon name="keyboard" size={14} />
                  <span className="qa-tile__text">
                    <label htmlFor="home-join-code">
                      <strong>{t('home.accoes.entrar')}</strong>
                    </label>
                    <span className="qa-join">
                      <input
                        id="home-join-code"
                        className="qa-join__input dx-num"
                        value={code}
                        onChange={(e) => {
                          setCode(e.target.value)
                          setJoinErr('')
                        }}
                        placeholder={t('home.entrar.placeholder')}
                        autoComplete="off"
                        spellCheck={false}
                        aria-invalid={joinErr ? true : undefined}
                        aria-describedby={joinErr ? 'home-join-err' : undefined}
                      />
                      {code.trim() && (
                        <button
                          type="submit"
                          className="qa-join__go"
                          disabled={joining}
                          aria-label={t('home.entrar.botao')}
                          title={t('home.entrar.botao')}
                        >
                          {joining ? <span className="dx-spinner" aria-hidden="true" /> : <Icon name="chevronRight" size={13} />}
                        </button>
                      )}
                    </span>
                  </span>
                </form>

                <button type="button" className="qa-tile qa-tile--live" onClick={() => navigate('studio')}>
                  <Icon name="live" size={14} />
                  <span className="qa-tile__text">
                    <strong>{t('consola.inicio.novoEstudio')}</strong>
                    <small>{t('consola.inicio.novoEstudioSub')}</small>
                  </span>
                </button>
              </div>

              {(startErr || joinErr) && (
                <div className="qa-errors" id="home-join-err">
                  {startErr && <Alert tone="danger">{startErr}</Alert>}
                  {joinErr && <Alert tone="danger">{joinErr}</Alert>}
                </div>
              )}
            </section>

            <Upcoming odooCalendar={odooCalendar} />
            <RecentRecordings />
          </div>
          <SideColumn />
        </div>
      </div>
    </>
  )
}
