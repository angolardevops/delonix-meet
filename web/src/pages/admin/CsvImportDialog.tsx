/**
 * Importar convites por CSV (ADR-0008): o servidor valida e cria
 * (rota `users/imports`); «simular» corre a mesma validação sem gravar
 * (`dry_run`). Só o texto sai daqui — o parse é do servidor.
 */
import { ChangeEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ImportReport, importUsersCsv } from '../../api'
import { Alert, Button, Dialog, TextArea } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'

export default function CsvImportDialog({
  orgId,
  onClose,
  onImported,
}: {
  orgId: string
  onClose: () => void
  onImported: () => void
}) {
  const { t } = useTranslation()
  const [csv, setCsv] = useState('')
  const [report, setReport] = useState<ImportReport | null>(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function onFile(e: ChangeEvent<HTMLInputElement>) {
    const f = e.target.files?.[0]
    if (f) setCsv(await f.text())
  }

  async function run(dryRun: boolean) {
    if (!csv.trim()) {
      setErr(t('org.csv.erroVazio'))
      return
    }
    setBusy(true)
    setErr('')
    try {
      setReport(await importUsersCsv(orgId, csv, dryRun))
      if (!dryRun) onImported()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.csv.erroImportar'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      title={t('org.csv.titulo')}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('ui.cancelar')}
          </Button>
          <Button variant="secondary" busy={busy} disabled={!csv.trim()} onClick={() => void run(true)}>
            {t('rbac.csvSimular')}
          </Button>
          <Button variant="primary" busy={busy} disabled={!csv.trim()} onClick={() => void run(false)}>
            {t('rbac.csvImportar')}
          </Button>
        </>
      }
    >
      <div className="org-form">
        <p className="dx-muted">{t('rbac.csvAviso')}</p>
        <label>
          <span>{t('org.csv.ficheiro')}</span>
          <input type="file" accept=".csv,text/csv" onChange={(e) => void onFile(e)} />
        </label>
        <label>
          <span>{t('org.csv.colar')}</span>
          <TextArea rows={6} value={csv} onChange={(e) => setCsv(e.target.value)} />
        </label>
        {err && <Alert tone="danger">{err}</Alert>}
        {report && (
          <Alert tone={report.errors > 0 ? 'warning' : 'success'}>
            {t(report.dry_run ? 'rbac.csvResultadoSimulado' : 'rbac.csvResultado', {
              convidados: report.invited,
              actualizados: report.updated,
              iguais: report.unchanged,
              erros: report.errors,
            })}
          </Alert>
        )}
        {report && report.lines.filter((l) => l.outcome === 'error').length > 0 && (
          <ul className="org-simple">
            {report.lines
              .filter((l) => l.outcome === 'error')
              .slice(0, 20)
              .map((l) => (
                <li key={l.line}>
                  <span className="dx-num">
                    {l.line}: {l.email}
                  </span>{' '}
                  <span className="dx-muted">{l.message ?? ''}</span>
                </li>
              ))}
          </ul>
        )}
      </div>
    </Dialog>
  )
}
