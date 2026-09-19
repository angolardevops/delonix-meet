/**
 * Gravação partilhada por link público (`#/share/:token`) — sem sessão; o
 * token é a credencial. GET /api/public/recordings/{token} responde 404 para link
 * inexistente OU expirado (o servidor não distingue, e o ecrã também não) e
 * 401 quando o link tem palavra-passe e ela falta ou está errada.
 */
import { FormEvent, useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getPublicShare, PublicShareInfo } from '../api'
import { Icon } from '../ui/icons'
import { Alert, Button, Card, Empty, Field, Spinner } from '../ui/kit'
import CampoPalavraPasse from './auth/CampoPalavraPasse'
import { megabytes } from './auth/logica'
import Moldura from './publico/Moldura'

type Estado =
  | { k: 'aVerificar' }
  | { k: 'pronta'; info: PublicShareInfo }
  | { k: 'protegida'; errada: boolean }
  | { k: 'indisponivel'; motivo: string }

const estadoDe = (e: unknown) => (e as { status?: number } | null)?.status

export default function SharePage({ token }: { token: string }) {
  const { t, i18n } = useTranslation()
  const [estado, setEstado] = useState<Estado>({ k: 'aVerificar' })
  const [password, setPassword] = useState('')
  const [busy, setBusy] = useState(false)

  const carregar = useCallback(
    async (pw?: string) => {
      try {
        const info = await getPublicShare(token, pw)
        setEstado({ k: 'pronta', info })
      } catch (e) {
        const s = estadoDe(e)
        if (s === 401) setEstado({ k: 'protegida', errada: !!pw })
        else if (s === 404) setEstado({ k: 'indisponivel', motivo: t('publico.partilha.invalido') })
        else setEstado({ k: 'indisponivel', motivo: t('publico.partilha.erro') })
      }
    },
    [token, t],
  )

  useEffect(() => {
    setEstado({ k: 'aVerificar' })
    setPassword('')
    void carregar()
  }, [carregar])

  async function aceder(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    await carregar(password)
    setBusy(false)
  }

  return (
    <Moldura pagina="share" estreita>
      {estado.k === 'aVerificar' && (
        <div className="pub-espera" role="status">
          <Spinner />
          <span>{t('publico.partilha.aVerificar')}</span>
        </div>
      )}

      {estado.k === 'indisponivel' && (
        <Card className="pub-cartao">
          <Empty
            icon="film"
            title={t('publico.partilha.indisponivel')}
            action={
              <a href="#/" className="dx-btn dx-btn--secondary">
                {t('publico.partilha.irInicio')}
              </a>
            }
          >
            {estado.motivo}
          </Empty>
        </Card>
      )}

      {estado.k === 'protegida' && (
        <Card className="pub-cartao">
          <form className="pub-partilha-senha" onSubmit={aceder}>
            <div className="pub-partilha__icone">
              <Icon name="lock" size={20} />
            </div>
            <h1>{t('publico.partilha.protegida')}</h1>
            <p className="dx-muted">{t('publico.partilha.pedePalavraPasse')}</p>
            <Field label={t('publico.partilha.palavraPasse')} htmlFor="share-password">
              <CampoPalavraPasse
                id="share-password"
                name="password"
                autoFocus
                required
                autoComplete="off"
                aria-invalid={estado.errada || undefined}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </Field>
            {estado.errada && <Alert tone="danger">{t('publico.partilha.errada')}</Alert>}
            <Button type="submit" variant="primary" size="lg" block busy={busy} disabled={!password}>
              {t('publico.partilha.aceder')}
            </Button>
          </form>
        </Card>
      )}

      {estado.k === 'pronta' && (
        <Card className="pub-cartao">
          <div className="pub-partilha">
            <div className="pub-partilha__icone">
              <Icon name="film" size={20} />
            </div>
            <div className="dx-eyebrow">{t('publico.partilha.gravacao')}</div>
            <h1 className="pub-partilha__nome">{estado.info.filename.replace(/\.(webm|mp4|mkv)$/i, '')}</h1>
            <dl className="dx-kv">
              <dt>{t('publico.partilha.criada')}</dt>
              <dd className="dx-num">{new Date(estado.info.created_at).toLocaleString(i18n.language)}</dd>
              <dt>{t('publico.partilha.tamanho')}</dt>
              <dd className="dx-num">{t('publico.partilha.tamanhoMb', { mb: megabytes(estado.info.size_bytes) })}</dd>
            </dl>
            <a
              className="dx-btn dx-btn--primary dx-btn--lg dx-btn--block"
              href={`/api/public/recordings/${token}/content${password ? `?password=${encodeURIComponent(password)}` : ''}`}
              download={estado.info.filename}
            >
              <Icon name="download" />
              {t('publico.partilha.descarregar')}
            </a>
          </div>
        </Card>
      )}
    </Moldura>
  )
}
