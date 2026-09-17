import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { BreakoutRoom } from '../signaling'
import { AvatarStack, Button, Select, TextInput, cx } from '../ui/kit'
import { Countdown } from './Clocks'

/** O que o cartão precisa — serve o hook da sala (`useBreakouts`) e a consola de moderação. */
export interface BreakoutsApi {
  rooms: BreakoutRoom[]
  /** Fim em SEGUNDOS epoch. */
  endsAt: number | null
  minutes: number
  setMinutes: (m: number) => void
  create: (count: number) => void
  rename: (roomCode: string, label: string) => void
  add: () => void
  moveUser: (name: string, roomCode: string) => void
  closeAll: () => void
  visit: (roomCode: string) => void
  /** À vez (servidor) ou à mão (as salas começam vazias). */
  assign: 'auto' | 'manual'
  setAssign: (a: 'auto' | 'manual') => void
  /** Mensagem a todas as salas (só anfitrião). */
  broadcast: (text: string) => void
}

const MINUTOS = [0, 5, 10, 15, 20, 30, 45, 60]

/**
 * Salas paralelas (template DelonixModeration): cabeçalho com salas e tempo
 * restante, a regra de atribuição, um cartão por sala com quem lá está,
 * temporizador, mensagem a todas as salas e acções. Em «Manual» as salas
 * nascem vazias e cada pessoa é posta numa sala pela lista.
 */
export function BreakoutsCard({ code, api, className }: { code: string; api: BreakoutsApi; className?: string }) {
  const { t } = useTranslation()
  const ativas = api.rooms.length > 0
  const [mensagem, setMensagem] = useState('')
  const [difundida, setDifundida] = useState(false)
  return (
    <section className={cx('rm-bocard', className)} aria-labelledby={`rm-bo-${code}`}>
      <div className="rm-bocard__head">
        <h3 id={`rm-bo-${code}`}>{t('room.paralelas.titulo')}</h3>
        {ativas && (
          <span className="dx-num dx-muted">
            {t('room.paralelas.salas', { count: api.rooms.length })}
            {api.endsAt && (
              <>
                {' · '}
                <Countdown endsAt={api.endsAt} render={(txt) => t('room.paralelas.restantes', { tempo: txt })} />
              </>
            )}
          </span>
        )}
      </div>

      <div className="rm-bocard__mode" role="radiogroup" aria-label={t('room.paralelas.titulo')}>
        {(['auto', 'manual'] as const).map((a) => (
          <button
            key={a}
            type="button"
            role="radio"
            aria-checked={api.assign === a}
            className={cx(api.assign === a && 'is-on')}
            // A regra só vale para salas a criar: com salas abertas, mostra-se mas não muda nada.
            disabled={ativas}
            title={a === 'auto' ? t('room.paralelas.automaticaDica') : t('room.paralelas.manualDica')}
            onClick={() => api.setAssign(a)}
          >
            {a === 'auto' ? t('room.paralelas.atribuicaoAutomatica') : t('room.paralelas.manual')}
          </button>
        ))}
      </div>

      {!ativas ? (
        <>
          <p className="dx-muted rm-bocard__text">{t('room.paralelas.dividir')}</p>
          <div className="rm-bocard__row">
            {[2, 3, 4].map((n) => (
              <Button key={n} size="sm" variant="outline" onClick={() => api.create(n)}>
                {t('room.paralelas.grupos', { count: n })}
              </Button>
            ))}
          </div>
          <label className="rm-bocard__row">
            <span className="dx-muted">{t('room.paralelas.duracao')}</span>
            <Select value={api.minutes} onChange={(e) => api.setMinutes(Number(e.target.value))}>
              {MINUTOS.map((m) => (
                <option key={m} value={m}>
                  {m === 0 ? t('room.paralelas.semLimite') : t('room.temporizador.minutos', { n: m })}
                </option>
              ))}
            </Select>
          </label>
          <p className="dx-muted rm-bocard__text">{t('room.paralelas.noFimVoltam')}</p>
        </>
      ) : (
        <>
          <div className="rm-bocard__rooms">
            {api.rooms.map((b) =>
              b.people.length === 0 ? (
                <div key={b.code} className="rm-boroom is-empty">
                  <TextInput
                    className="rm-boroom__name"
                    defaultValue={b.label}
                    maxLength={60}
                    aria-label={t('room.paralelas.renomear')}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
                    }}
                    onBlur={(e) => {
                      const label = e.target.value.trim()
                      if (label && label !== b.label) api.rename(b.code, label)
                    }}
                  />
                  <span className="dx-num dx-muted">{t('room.paralelas.vaziaMin')}</span>
                </div>
              ) : (
                <div key={b.code} className="rm-boroom">
                  <div className="rm-boroom__head">
                    <TextInput
                      className="rm-boroom__name"
                      defaultValue={b.label}
                      maxLength={60}
                      aria-label={t('room.paralelas.renomear')}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
                      }}
                      onBlur={(e) => {
                        const label = e.target.value.trim()
                        if (label && label !== b.label) api.rename(b.code, label)
                      }}
                    />
                    <span className="dx-num dx-muted">{t('room.paralelas.pessoas', { count: b.people.length })}</span>
                  </div>
                  <div className="rm-boroom__row">
                    <AvatarStack names={b.people} max={6} size={22} />
                    <Button size="sm" variant="outline" onClick={() => api.visit(b.code)}>
                      {t('room.paralelas.entrar')}
                    </Button>
                  </div>
                  <details className="rm-boroom__move">
                    <summary>{t('room.paralelas.moverPessoas')}</summary>
                    {b.people.map((name) => (
                      <label key={name} className="rm-boroom__person">
                        <span>{name}</span>
                        <Select aria-label={t('room.paralelas.moverPessoa', { nome: name })} value={b.code} onChange={(e) => api.moveUser(name, e.target.value)}>
                          {api.rooms.map((o) => (
                            <option key={o.code} value={o.code}>
                              {o.label}
                            </option>
                          ))}
                          <option value={code}>{t('room.paralelas.principal')}</option>
                        </Select>
                      </label>
                    ))}
                  </details>
                </div>
              ),
            )}
          </div>
          <div className="rm-bocard__foot">
            {api.endsAt && (
              <div className="rm-bocard__timer">
                <span className="dx-num dx-muted">{t('room.paralelas.temporizador')}</span>
                <Countdown endsAt={api.endsAt} render={(txt) => <strong className="dx-num">{txt}</strong>} />
              </div>
            )}
            <form
              className="rm-bocard__broadcast"
              onSubmit={(e) => {
                e.preventDefault()
                const texto = mensagem.trim()
                if (!texto) return
                api.broadcast(texto)
                setMensagem('')
                setDifundida(true)
                window.setTimeout(() => setDifundida(false), 4000)
              }}
            >
              <input
                value={mensagem}
                maxLength={500}
                placeholder={t('room.paralelas.mensagemTodas')}
                aria-label={t('room.paralelas.mensagemTodas')}
                onChange={(e) => setMensagem(e.target.value)}
              />
              <button type="submit" disabled={!mensagem.trim()}>
                {t('room.paralelas.difundir')}
              </button>
            </form>
            {difundida && (
              <span className="dx-muted rm-bocard__text" role="status">
                {t('room.paralelas.difundida')}
              </span>
            )}
            <div className="rm-bocard__row">
              <Button size="sm" variant="outline" block onClick={api.add}>
                {t('room.paralelas.novaSala')}
              </Button>
              <Button size="sm" variant="primary" block onClick={api.closeAll}>
                {t('room.paralelas.encerrar')}
              </Button>
            </div>
          </div>
        </>
      )}
    </section>
  )
}
