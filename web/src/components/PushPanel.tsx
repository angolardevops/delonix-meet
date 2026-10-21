/**
 * «Avisos com a aplicação fechada» (A-03, ADR-0032), nas definições da conta.
 *
 * Só oferece o botão quando ele pode funcionar: com a instalação a enviar
 * (`enabled`), o browser capaz, e — no iPhone e no iPad — a aplicação instalada
 * no ecrã principal. Em cada um dos outros casos diz PORQUÊ, em vez de um botão
 * que nunca faz nada.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { pushStatus } from '../api'
import { Alert, Button } from '../ui/kit'
import {
  ambienteDoBrowser,
  esteBrowserSubscrito,
  PushRecusado,
  retirarEsteBrowser,
  subscreverEsteBrowser,
  suportePush,
} from '../webPush'

type Estado =
  | { s: 'a-ler' }
  | { s: 'desligado-na-instalacao' }
  | { s: 'ios-instalar' | 'sem-suporte' | 'bloqueado' }
  | { s: 'pronto'; subscrito: boolean }

export default function PushPanel() {
  const { t } = useTranslation()
  const [estado, setEstado] = useState<Estado>({ s: 'a-ler' })
  const [busy, setBusy] = useState(false)
  const [erro, setErro] = useState('')

  async function ler() {
    const suporte = suportePush(ambienteDoBrowser())
    try {
      const st = await pushStatus()
      if (!st.enabled) return setEstado({ s: 'desligado-na-instalacao' })
    } catch {
      return setEstado({ s: 'sem-suporte' })
    }
    if (suporte !== 'pronto') return setEstado({ s: suporte })
    setEstado({ s: 'pronto', subscrito: await esteBrowserSubscrito().catch(() => false) })
  }

  useEffect(() => {
    void ler()
  }, [])

  async function alternar(ligar: boolean) {
    setBusy(true)
    setErro('')
    try {
      if (ligar) await subscreverEsteBrowser()
      else await retirarEsteBrowser()
    } catch (e) {
      setErro(e instanceof PushRecusado ? t('shell.def.push.recusado') : t('shell.def.push.erro'))
    } finally {
      setBusy(false)
      await ler()
    }
  }

  let corpo
  switch (estado.s) {
    case 'a-ler':
      corpo = null
      break
    case 'desligado-na-instalacao':
      corpo = <p className="dx-muted" style={{ margin: 0 }}>{t('shell.def.push.desligadoNaInstalacao')}</p>
      break
    case 'ios-instalar':
      corpo = <Alert>{t('shell.def.push.iosInstalar')}</Alert>
      break
    case 'sem-suporte':
      corpo = <p className="dx-muted" style={{ margin: 0 }}>{t('shell.def.push.semSuporte')}</p>
      break
    case 'bloqueado':
      corpo = <Alert tone="warning">{t('shell.def.push.bloqueado')}</Alert>
      break
    case 'pronto':
      corpo = (
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
          {estado.subscrito && <span>{t('shell.def.push.ligado')}</span>}
          <Button type="button" variant="secondary" busy={busy} onClick={() => void alternar(!estado.subscrito)}>
            {estado.subscrito ? t('shell.def.push.desligar') : t('shell.def.push.ligar')}
          </Button>
        </div>
      )
      break
  }

  return (
    <div className="settings-section">
      <h3 className="dx-eyebrow" style={{ margin: '8px 0 4px' }}>{t('shell.def.push.titulo')}</h3>
      <p className="dx-muted" style={{ margin: '0 0 8px' }}>{t('shell.def.push.dica')}</p>
      {erro && <Alert tone="danger">{erro}</Alert>}
      {corpo}
    </div>
  )
}
