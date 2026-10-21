/**
 * Armazenamento das gravações — da PLATAFORMA, não de uma organização.
 * Adaptado de `web/src/pages/integrations/StorageCard.tsx` para esta app: a
 * forma do pedido e da resposta são exactamente as mesmas (as rotas de
 * operador já existiam, PR1/PR2), só o envolvimento muda — aqui é a página
 * inteira, lá era um cartão entre outros.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  apiErrorMessage,
  authedBlobUrl,
  getPlatformStorage,
  savePlatformStorage,
  StorageConfig,
  testPlatformStorage,
} from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import { guarded } from '../guarded'
import { Alert, Button, Card, Field, TextInput } from '../ui/kit'

type Backend = StorageConfig['storage_type']
const BACKENDS: Backend[] = ['local', 'nfs', 'webdav']

export default function Integrations() {
  const { t } = useTranslation()
  const { state, reload } = useAsync(() => guarded(getPlatformStorage()), [])
  return (
    <div className="page">
      <Card title={t('storage.titulo')} eyebrow={t('storage.sub')}>
        <AsyncSection state={state} onRetry={reload}>
          {(g) => (g.forbidden ? <Alert tone="warning">{t('storage.soPlataforma')}</Alert> : <StorageForm initial={g.d} onSaved={reload} />)}
        </AsyncSection>
      </Card>
    </div>
  )
}

function StorageForm({ initial, onSaved }: { initial: StorageConfig; onSaved: () => void }) {
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
      setErr(apiErrorMessage(e, t('storage.erroGuardar')))
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
      setTest({ ok: false, message: apiErrorMessage(e, t('storage.testeFalhou')) })
    } finally {
      setBusy(null)
    }
  }

  // O manifesto pede autenticação: um <a href> não leva o token.
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
      setErr(apiErrorMessage(e, t('storage.manifestoFalhou')))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div style={{ display: 'grid', gap: 14 }}>
      <fieldset style={{ display: 'grid', gap: 8, border: 0, padding: 0, margin: 0 }}>
        <legend className="dx-sr-only">{t('storage.destino')}</legend>
        {BACKENDS.map((b) => (
          <label
            key={b}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              border: `1px solid ${type === b ? 'var(--accent)' : 'var(--border)'}`,
              borderRadius: 'var(--r-2)',
              padding: 10,
            }}
          >
            <input type="radio" name="storage-backend" value={b} checked={type === b} onChange={() => setType(b)} />
            <span style={{ display: 'flex', flexDirection: 'column' }}>
              <strong>{t(`storage.tipo.${b}`)}</strong>
              <span className="dx-muted" style={{ fontSize: 11 }}>{t(`storage.tipoDica.${b}`)}</span>
            </span>
            {initial.storage_type === b && <span className="dx-tag dx-tag--plain" style={{ marginLeft: 'auto' }}>{t('storage.emUso')}</span>}
          </label>
        ))}
      </fieldset>

      {type === 'nfs' && (
        <>
          <p className="dx-muted">{t('storage.nfsDica')}</p>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
            <Field label={t('storage.nfsServidor')} htmlFor="nfs-server">
              <TextInput id="nfs-server" value={nfsServer} onChange={(e) => setNfsServer(e.target.value)} autoComplete="off" />
            </Field>
            <Field label={t('storage.nfsCaminho')} htmlFor="nfs-path">
              <TextInput id="nfs-path" value={nfsPath} onChange={(e) => setNfsPath(e.target.value)} autoComplete="off" />
            </Field>
          </div>
          <div>
            <Button size="sm" icon="download" busy={busy === 'manifest'} disabled={initial.storage_type !== 'nfs'} onClick={() => void downloadManifest()}>
              {t('storage.manifesto')}
            </Button>
            {initial.storage_type !== 'nfs' && <div className="dx-muted" style={{ fontSize: 11, marginTop: 4 }}>{t('storage.manifestoGuardarPrimeiro')}</div>}
          </div>
        </>
      )}

      {type === 'webdav' && (
        <>
          <p className="dx-muted">{t('storage.webdavDica')}</p>
          <Field label={t('storage.webdavUrl')} htmlFor="wd-url">
            <TextInput id="wd-url" type="url" inputMode="url" value={wdUrl} onChange={(e) => setWdUrl(e.target.value)} autoComplete="off" />
          </Field>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
            <Field label={t('storage.webdavUtilizador')} htmlFor="wd-user">
              <TextInput id="wd-user" value={wdUser} onChange={(e) => setWdUser(e.target.value)} autoComplete="off" />
            </Field>
            <Field label={t('storage.webdavPassword')} htmlFor="wd-pwd" hint={initial.webdav_password_set ? t('storage.passwordDefinida') : undefined}>
              <TextInput id="wd-pwd" type="password" value={wdPwd} onChange={(e) => setWdPwd(e.target.value)} autoComplete="new-password" />
            </Field>
          </div>
          <Field label={t('storage.webdavCaminho')} htmlFor="wd-path">
            <TextInput id="wd-path" className="dx-num" value={wdPath} onChange={(e) => setWdPath(e.target.value)} autoComplete="off" />
          </Field>
        </>
      )}

      {initial.object_store && !initial.object_store.used_for_recordings && (
        <div>
          <strong>{t('storage.objectosTitulo')}</strong>
          <div className="dx-muted" style={{ fontSize: 11 }}>{t('storage.objectosSub')}</div>
          <div className="dx-num" style={{ fontSize: 11 }}>
            {t('storage.objectosEndpoint')}: {initial.object_store.endpoint} · {t('storage.objectosBucket')}: {initial.object_store.bucket}
          </div>
          <Alert tone="warning">{t('storage.objectosAindaNao')}</Alert>
        </div>
      )}

      {err && <Alert tone="danger">{err}</Alert>}
      {ok && <Alert tone="success">{t('storage.guardado')}</Alert>}
      {test && (
        <Alert tone={test.ok ? 'success' : 'warning'}>
          <strong>{test.ok ? t('storage.testeOk') : t('storage.testeFalhou')}</strong>
          <div>{test.message}</div>
        </Alert>
      )}
      <div style={{ display: 'flex', gap: 8 }}>
        <Button icon="plug" busy={busy === 'test'} disabled={busy !== null || dirty} onClick={() => void runTest()}>
          {t('storage.testar')}
        </Button>
        <Button variant="primary" busy={busy === 'save'} disabled={busy !== null && busy !== 'save'} onClick={() => void save()}>
          {t('ui.guardar')}
        </Button>
      </div>
      {dirty && <div className="dx-muted" style={{ fontSize: 11 }}>{t('storage.testaGravado')}</div>}
    </div>
  )
}
