/**
 * Armazenamento das gravações — da PLATAFORMA, não da organização. Só quem o
 * servidor declara em `PLATFORM_ADMIN_USER_IDS` o lê; os outros levam 403 e o
 * cartão diz porquê em vez de mostrar um formulário que falharia ao guardar.
 *
 * Os destinos são exactamente os do `StorageConfig` (local, NFS, WebDAV).
 *
 * O volume ocupado é o das gravações da ORGANIZAÇÃO (orgStats, só admins da
 * org): o servidor não soma a plataforma inteira nem conhece um tecto.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ApiError,
  apiErrorMessage,
  authedBlobUrl,
  getPlatformStorage,
  orgStats,
  savePlatformStorage,
  StorageConfig,
  testPlatformStorage,
} from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Card, cx, Field, TextInput } from '../../ui/kit'
import { Icon } from '../../ui/icons'
import { formatBytes } from '../admin/orgShared'
import { guarded, IntegHead } from './common'

type Backend = StorageConfig['storage_type']
const BACKENDS: Backend[] = ['local', 'nfs', 'webdav']

export function StorageCard({ orgId }: { orgId?: string }) {
  const { t, i18n } = useTranslation()
  const { state, reload } = useAsync(() => guarded(getPlatformStorage()), [])
  const used = useAsync(async () => {
    if (!orgId) return null
    try {
      return (await orgStats(orgId)).recordings_bytes
    } catch (e) {
      if (e instanceof ApiError && (e.status === 401 || e.status === 403)) return null
      throw e
    }
  }, [orgId])
  const usedText = used.state.s === 'ready' && used.state.d !== null ? formatBytes(used.state.d, i18n.language) : null
  return (
    <Card className="integ-card">
      <IntegHead
        icon="database"
        title={t('integrations.storage.titulo')}
        sub={t('integrations.storage.sub')}
      />
      <AsyncSection state={state} onRetry={reload}>
        {(g) =>
          g.forbidden ? (
            <Alert tone="warning">{t('integrations.storage.soPlataforma')}</Alert>
          ) : (
            <StorageForm initial={g.d} onSaved={reload} usedText={usedText} />
          )
        }
      </AsyncSection>
    </Card>
  )
}

function StorageForm({ initial, onSaved, usedText }: { initial: StorageConfig; onSaved: () => void; usedText: string | null }) {
  const { t } = useTranslation()
  const [type, setType] = useState<Backend>(initial.storage_type)
  const [nfsServer, setNfsServer] = useState(initial.nfs_server ?? '')
  const [nfsPath, setNfsPath] = useState(initial.nfs_path ?? '')
  const [wdUrl, setWdUrl] = useState(initial.webdav_url ?? '')
  const [wdUser, setWdUser] = useState(initial.webdav_user ?? '')
  const [wdPwd, setWdPwd] = useState('')
  const [wdPath, setWdPath] = useState(initial.webdav_path)
  const [busy, setBusy] = useState<'save' | 'test' | 'manifest' | null>(null)
  const [err, setErr] = useState('')
  const [ok, setOk] = useState(false)
  const [test, setTest] = useState<{ ok: boolean; message: string } | null>(null)
  const dirty = type !== initial.storage_type

  async function save() {
    setBusy('save')
    setErr('')
    setOk(false)
    setTest(null)
    try {
      await savePlatformStorage({
        storage_type: type,
        nfs_server: nfsServer.trim() || undefined,
        nfs_path: nfsPath.trim() || undefined,
        webdav_url: wdUrl.trim() || undefined,
        webdav_user: wdUser.trim() || undefined,
        webdav_password: wdPwd || undefined,
        webdav_path: wdPath.trim() || undefined,
      })
      setWdPwd('')
      setOk(true)
      onSaved()
    } catch (e) {
      setErr(apiErrorMessage(e, t('integrations.erroGuardar')))
    } finally {
      setBusy(null)
    }
  }

  async function runTest() {
    setBusy('test')
    setErr('')
    setTest(null)
    try {
      const r = await testPlatformStorage()
      setTest({ ok: r.ok, message: r.message })
    } catch (e) {
      setTest({ ok: false, message: apiErrorMessage(e, t('integrations.storage.testeFalhou')) })
    } finally {
      setBusy(null)
    }
  }

  // O manifesto pede autenticação: um <a href> não leva o token, por isso
  // busca-se com ele e entrega-se ao browser como ficheiro.
  async function downloadManifest() {
    setBusy('manifest')
    setErr('')
    try {
      const href = await authedBlobUrl('/api/operator/v1/storage/pvc-manifest')
      const a = document.createElement('a')
      a.href = href
      a.download = 'delonix-recordings-pv.yaml'
      a.click()
      window.setTimeout(() => URL.revokeObjectURL(href), 2000)
    } catch (e) {
      setErr(apiErrorMessage(e, t('integrations.storage.manifestoFalhou')))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="integ-stack">
      <fieldset className="integ-options">
        <legend className="dx-sr-only">{t('integrations.storage.destino')}</legend>
        {BACKENDS.map((b) => (
          <label key={b} className={cx('integ-option', type === b && 'is-on')}>
            <input type="radio" name="storage-backend" value={b} checked={type === b} onChange={() => setType(b)} />
            <span className="integ-option__text">
              <span className="integ-option__title">{t(`integrations.storage.tipo.${b}`)}</span>
              <span className="integ-option__sub">{t(`integrations.storage.tipoDica.${b}`)}</span>
            </span>
            {initial.storage_type === b && (
              <span className="integ-option__tag dx-num" data-testid={usedText ? 'integ-storage-used' : undefined} title={usedText ? t('consola.integracoes.ocupado') : undefined}>
                {usedText ? (
                  <>
                    <Icon name="check" size={10} /> {usedText}
                  </>
                ) : (
                  t('integrations.storage.emUso')
                )}
              </span>
            )}
          </label>
        ))}
      </fieldset>

      {type === 'nfs' && (
        <>
          <p className="integ-desc">{t('integrations.storage.nfsDica')}</p>
          <div className="integ-grid2">
            <Field label={t('integrations.storage.nfsServidor')} htmlFor="nfs-server">
              <TextInput id="nfs-server" value={nfsServer} onChange={(e) => setNfsServer(e.target.value)} autoComplete="off" />
            </Field>
            <Field label={t('integrations.storage.nfsCaminho')} htmlFor="nfs-path">
              <TextInput id="nfs-path" value={nfsPath} onChange={(e) => setNfsPath(e.target.value)} autoComplete="off" />
            </Field>
          </div>
          <div>
            <Button
              size="sm"
              icon="download"
              busy={busy === 'manifest'}
              disabled={initial.storage_type !== 'nfs'}
              onClick={() => void downloadManifest()}
            >
              {t('integrations.storage.manifesto')}
            </Button>
            {initial.storage_type !== 'nfs' && <div className="dx-muted integ-small">{t('integrations.storage.manifestoGuardarPrimeiro')}</div>}
          </div>
        </>
      )}

      {type === 'webdav' && (
        <>
          <p className="integ-desc">{t('integrations.storage.webdavDica')}</p>
          <Field label={t('integrations.storage.webdavUrl')} htmlFor="wd-url">
            <TextInput id="wd-url" type="url" inputMode="url" value={wdUrl} onChange={(e) => setWdUrl(e.target.value)} autoComplete="off" />
          </Field>
          <div className="integ-grid2">
            <Field label={t('integrations.storage.webdavUtilizador')} htmlFor="wd-user">
              <TextInput id="wd-user" value={wdUser} onChange={(e) => setWdUser(e.target.value)} autoComplete="off" />
            </Field>
            <Field
              label={t('integrations.storage.webdavPassword')}
              htmlFor="wd-pwd"
              hint={initial.webdav_password_set ? t('integrations.storage.passwordDefinida') : undefined}
            >
              <TextInput id="wd-pwd" type="password" value={wdPwd} onChange={(e) => setWdPwd(e.target.value)} autoComplete="new-password" />
            </Field>
          </div>
          <Field label={t('integrations.storage.webdavCaminho')} htmlFor="wd-path">
            <TextInput id="wd-path" className="dx-num" value={wdPath} onChange={(e) => setWdPath(e.target.value)} autoComplete="off" />
          </Field>
        </>
      )}

      {err && <Alert tone="danger">{err}</Alert>}
      {ok && <Alert tone="success">{t('integrations.guardado')}</Alert>}
      {test && (
        <Alert tone={test.ok ? 'success' : 'warning'}>
          <strong>{test.ok ? t('integrations.storage.testeOk') : t('integrations.storage.testeFalhou')}</strong>
          <div>{test.message}</div>
        </Alert>
      )}
      <div className="integ-actions">
        <Button icon="plug" busy={busy === 'test'} disabled={busy !== null || dirty} onClick={() => void runTest()}>
          {t('integrations.storage.testar')}
        </Button>
        <Button variant="primary" busy={busy === 'save'} disabled={busy !== null && busy !== 'save'} onClick={() => void save()}>
          {t('ui.guardar')}
        </Button>
      </div>
      {dirty && <div className="dx-muted integ-small">{t('integrations.storage.testaGravado')}</div>}
    </div>
  )
}
