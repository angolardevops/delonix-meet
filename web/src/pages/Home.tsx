/**
 * Início — a porta de entrada da consola.
 *
 * As quatro acções rápidas vivem no CORPO da página em todas as larguras
 * (lote2 3.1.4, R103): no telemóvel, começar ou entrar numa reunião não pode
 * ficar atrás de um toque no menu. Entrar por código aceita o código solto ou
 * o link colado, pelo mesmo parser da paleta de comandos.
 */
import { FormEvent, useState } from 'react'
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
  const { user, enterRoom, navigate } = useShell()
  const [creating, setCreating] = useState(false)
  const [waitingRoom, setWaitingRoom] = useState(false)
  const [e2ee, setE2ee] = useState(false)
  const [format, setFormat] = useState<Format>('normal')
  const [startErr, setStartErr] = useState('')
  const [code, setCode] = useState('')
  const [joining, setJoining] = useState(false)
  const [joinErr, setJoinErr] = useState('')
  const odooCalendar = useOdooCalendar()

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

  return (
    <>
      <PageBar title={<Greeting name={user.username} />} meta={<TodayStamp />} />
      <div className="page home">
        <div className="home-grid">
          <div className="home-main">
            <section className="home-actions" aria-label={t('home.accoes.rotulo')}>
              <div className="quick-actions">
                <button
                  type="button"
                  className="qa-tile qa-tile--primary dx-btn dx-btn--primary"
                  disabled={creating}
                  aria-busy={creating || undefined}
                  onClick={() => void startNow()}
                >
                  {creating ? <span className="dx-spinner" aria-hidden="true" /> : <Icon name="video" size={18} />}
                  <span className="qa-tile__text">
                    <strong>{t('home.accoes.iniciar')}</strong>
                    <small>{creating ? t('home.accoes.aCriar') : t('home.accoes.iniciarSub')}</small>
                  </span>
                </button>

                <button type="button" className="qa-tile" onClick={() => (location.hash = calendarHash.schedule())}>
                  <Icon name="calendar" size={18} />
                  <span className="qa-tile__text">
                    <strong>{t('home.accoes.agendar')}</strong>
                    <small>{odooCalendar ? t('consola.inicio.agendarOdoo') : t('home.accoes.agendarSub')}</small>
                  </span>
                </button>

                <form className={cx('qa-tile qa-tile--join', joinErr && 'qa-tile--error')} onSubmit={join}>
                  <label className="qa-tile__text" htmlFor="home-join-code">
                    <strong>{t('home.accoes.entrar')}</strong>
                    <small>{t('home.accoes.entrarSub')}</small>
                  </label>
                  <span className="qa-join">
                    <input
                      id="home-join-code"
                      className="dx-input dx-input--code"
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
                    <button
                      type="submit"
                      className="dx-iconbtn"
                      disabled={!code.trim() || joining}
                      aria-label={t('home.entrar.botao')}
                      title={t('home.entrar.botao')}
                    >
                      {joining ? <span className="dx-spinner" aria-hidden="true" /> : <Icon name="chevronRight" />}
                    </button>
                  </span>
                </form>

                <button type="button" className="qa-tile qa-tile--live" onClick={() => navigate('studio')}>
                  <Icon name="live" size={18} />
                  <span className="qa-tile__text">
                    <strong>{t('home.accoes.estudio')}</strong>
                    <small>{t('home.accoes.estudioSub')}</small>
                  </span>
                </button>
              </div>

              <div className="qa-options">
                <span className="dx-muted" id="home-opcoes">
                  {t('home.iniciar.opcoes')}
                </span>
                <div className="dx-chips" role="group" aria-labelledby="home-opcoes">
                  <button
                    type="button"
                    className="dx-chip"
                    aria-pressed={waitingRoom}
                    title={t('home.iniciar.salaEsperaDica')}
                    onClick={() => setWaitingRoom((v) => !v)}
                  >
                    <Icon name="door" size={12} />
                    {t('home.iniciar.salaEspera')}
                  </button>
                  <button
                    type="button"
                    className="dx-chip"
                    aria-pressed={e2ee}
                    title={t('home.iniciar.e2eeDica')}
                    onClick={() => setE2ee((v) => !v)}
                  >
                    <Icon name="lock" size={12} />
                    {t('home.iniciar.e2ee')}
                  </button>
                  <button
                    type="button"
                    className="dx-chip"
                    aria-pressed={format === 'training'}
                    title={t('home.iniciar.treinoDica')}
                    onClick={() => setFormat((f) => (f === 'training' ? 'normal' : 'training'))}
                  >
                    <Icon name="grid" size={12} />
                    {t('home.iniciar.treino')}
                  </button>
                </div>
              </div>

              {(startErr || joinErr) && (
                <div className="qa-errors" id="home-join-err">
                  {startErr && <Alert tone="danger">{startErr}</Alert>}
                  {joinErr && <Alert tone="danger">{joinErr}</Alert>}
                </div>
              )}
            </section>

            <Upcoming />
            <RecentRecordings />
          </div>
          <SideColumn />
        </div>
      </div>
    </>
  )
}
