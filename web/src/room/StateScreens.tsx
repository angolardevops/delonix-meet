import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { DelonixSymbol, Icon } from '../ui/icons'
import { Button, Field, TextInput } from '../ui/kit'

/** Sala E2EE sem frase-chave nesta sessão: pede-se antes de ligar. */
export function PassphraseScreen({ code, onSubmit, onCancel }: { code: string; onSubmit: (pass: string) => void; onCancel: () => void }) {
  const { t } = useTranslation()
  const [pass, setPass] = useState('')
  const [show, setShow] = useState(false)
  return (
    <div className="rm-center">
      <form
        className="rm-center__card"
        onSubmit={(e) => {
          e.preventDefault()
          onSubmit(pass)
        }}
      >
        <span className="rm-center__icon" aria-hidden="true">
          <Icon name="lock" size={22} />
        </span>
        <h1>{t('room.e2ee.titulo')}</h1>
        <p className="dx-muted">{t('room.e2ee.explicacao')}</p>
        <Field label={t('room.e2ee.fraseChave')} htmlFor="rm-e2ee-pass" hint={t('room.e2ee.fraseErrada')}>
          <div className="rm-pass">
            <TextInput
              id="rm-e2ee-pass"
              large
              type={show ? 'text' : 'password'}
              autoComplete="off"
              autoFocus
              value={pass}
              onChange={(e) => setPass(e.target.value)}
            />
            <button
              type="button"
              className="dx-iconbtn"
              aria-pressed={show}
              aria-label={show ? t('room.e2ee.esconder') : t('room.e2ee.mostrar')}
              onClick={() => setShow((v) => !v)}
            >
              <Icon name="eye" />
            </button>
          </div>
        </Field>
        <div className="rm-center__actions">
          <Button variant="ghost" onClick={onCancel}>
            {t('room.e2ee.cancelar')}
          </Button>
          <Button variant="primary" type="submit" disabled={!pass.trim()} icon="key">
            {t('room.e2ee.entrar')}
          </Button>
        </div>
        <span className="dx-num dx-muted">{code}</span>
      </form>
    </div>
  )
}

/** Recusado à porta ou removido pelo anfitrião. */
export function EndedScreen({ kind, onLeave }: { kind: 'denied' | 'kicked'; onLeave: () => void }) {
  const { t } = useTranslation()
  return (
    <div className="rm-center">
      <div className="rm-center__card" role="alert">
        <span className="rm-center__icon" aria-hidden="true">
          <DelonixSymbol size={22} />
        </span>
        <h1>{kind === 'denied' ? t('room.fim.recusado') : t('room.fim.removido')}</h1>
        <p className="dx-muted">{kind === 'denied' ? t('room.fim.recusadoTexto') : t('room.fim.removidoTexto')}</p>
        <div className="rm-center__actions">
          <Button variant="primary" icon="home" onClick={onLeave}>
            {t('room.fim.voltar')}
          </Button>
        </div>
      </div>
    </div>
  )
}
