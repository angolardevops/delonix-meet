import { ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage } from '../../api'
import { Alert, Button, Dialog } from '../../ui/kit'

/**
 * Confirmação de uma acção que não se desfaz (revogar, eliminar, rodar). O
 * pedido corre dentro do diálogo: se falhar, o erro fica à vista e o diálogo
 * não fecha — fechar em silêncio seria dizer que correu bem.
 */
export function ConfirmDialog({
  title,
  children,
  confirmLabel,
  onConfirm,
  onClose,
}: {
  title: ReactNode
  children: ReactNode
  confirmLabel: string
  onConfirm: () => Promise<void>
  onClose: () => void
}) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  async function run() {
    setBusy(true)
    setErr('')
    try {
      await onConfirm()
      onClose()
    } catch (e) {
      setErr(apiErrorMessage(e, t('ui.erroGenerico')))
      setBusy(false)
    }
  }
  return (
    <Dialog
      title={title}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button variant="danger" busy={busy} onClick={() => void run()}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      <div className="integ-stack">
        <div>{children}</div>
        {err && <Alert tone="danger">{err}</Alert>}
      </div>
    </Dialog>
  )
}
