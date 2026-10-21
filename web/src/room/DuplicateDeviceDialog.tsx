import { useTranslation } from 'react-i18next'
import { Alert, Button, Dialog } from '../ui/kit'
import type { DuplicateDevice } from './useDuplicateDevice'

/** «Estás também ao telefone»: continuar só no Meet, só no telefone, ou nos dois. */
export function DuplicateDeviceDialog({ dup }: { dup: DuplicateDevice }) {
  const { t } = useTranslation()
  if (!dup.aviso) return null
  return (
    <Dialog title={t('room.dispositivos.titulo')} onClose={dup.dispensar}>
      <p>{t('room.dispositivos.texto')}</p>
      <div className="rm-block__row" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 8 }}>
        <Button variant="primary" icon="video" busy={dup.aTratar} disabled={dup.aTratar} onClick={() => dup.escolher('meet')}>
          {dup.aviso.canHangup ? t('room.dispositivos.soMeet') : t('room.dispositivos.soMeetSemSom')}
        </Button>
        <Button variant="outline" icon="phone" disabled={dup.aTratar} onClick={() => dup.escolher('phone')}>
          {t('room.dispositivos.soTelefone')}
        </Button>
        <Button variant="ghost" disabled={dup.aTratar} onClick={() => dup.escolher('both')}>
          {t('room.dispositivos.nosDois')}
        </Button>
      </div>
      <Alert tone="warning">{t('room.dispositivos.eco')}</Alert>
    </Dialog>
  )
}

/** O que o servidor fez com a escolha, dito em poucas palavras. */
export function DuplicateDeviceResult({ dup }: { dup: DuplicateDevice }) {
  const { t } = useTranslation()
  if (!dup.resultado) return null
  return (
    <section className="rm-notice" role="status" aria-label={t('room.dispositivos.titulo')}>
      <header className="rm-notice__head">
        <strong>{t(`room.dispositivos.resultado.${dup.resultado}`)}</strong>
        <span className="dx-spacer" />
        <Button size="sm" variant="ghost" onClick={dup.dispensarResultado}>
          {t('room.dispositivos.ok')}
        </Button>
      </header>
    </section>
  )
}
