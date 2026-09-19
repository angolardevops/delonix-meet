/**
 * Importação em massa de convites por CSV. Um ficheiro `.csv` (ou texto
 * colado) com `email,title,role,branch` — só `email` é obrigatório, o resto
 * é opcional e em qualquer ordem (ver `csv.ts`). Cada linha é um convite
 * independente: o servidor processa-as todas e devolve um resultado por
 * linha (não é tudo-ou-nada, mesmo padrão do `applyToSelected` de
 * `MembersCard`) — uma linha malformada não bloqueia as outras.
 */
import { ChangeEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { bulkCreateInvites, BulkInviteResult } from '../../api'
import { Alert, Button, Dialog, TextArea } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'
import { parseInviteCsv } from './csv'

const EXEMPLO = 'email,title,role,branch\nana@empresa.co,Gestora,member,Luanda\nrui@empresa.co,,admin,'

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
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [results, setResults] = useState<BulkInviteResult[] | null>(null)

  const parsed = parseInviteCsv(text)
  const canSubmit = text.trim() !== '' && !parsed.errorKey && parsed.rows.length > 0

  async function onFile(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    e.target.value = ''
    if (!file) return
    setText(await file.text())
  }

  async function submit() {
    if (!canSubmit) return
    setBusy(true)
    setErr('')
    try {
      const res = await bulkCreateInvites(orgId, parsed.rows)
      setResults(res)
      onImported()
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'org.csv.erroImportar'))
    } finally {
      setBusy(false)
    }
  }

  const okCount = results?.filter((r) => r.ok).length ?? 0

  return (
    <Dialog
      wide
      title={t('org.csv.titulo')}
      onClose={onClose}
      footer={
        results ? (
          <Button variant="primary" onClick={onClose}>
            {t('ui.fechar')}
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose}>
              {t('ui.cancelar')}
            </Button>
            <Button variant="primary" busy={busy} disabled={!canSubmit} onClick={() => void submit()}>
              {t('org.csv.importar', { count: parsed.rows.length })}
            </Button>
          </>
        )
      }
    >
      {results ? (
        <div className="org-form">
          <Alert tone={okCount === results.length ? 'success' : 'warning'}>
            {t('org.csv.resultado', { ok: okCount, total: results.length })}
          </Alert>
          <div className="dx-table-wrap org-table-wrap">
            <table className="dx-table org-table">
              <thead>
                <tr>
                  <th scope="col">{t('org.campo.email')}</th>
                  <th scope="col">{t('org.csv.coluna.estado')}</th>
                </tr>
              </thead>
              <tbody>
                {results.map((r, i) => (
                  <tr key={`${r.email}-${i}`}>
                    <td className="dx-num">{r.email}</td>
                    <td>{r.ok ? <span className="dx-muted">✓ {t('org.csv.criado')}</span> : <span className="org-csv-erro">✗ {r.error}</span>}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ) : (
        <div className="org-form">
          <Alert>{t('org.csv.aviso')}</Alert>
          <label className="org-form__file">
            <span>{t('org.csv.ficheiro')}</span>
            <input type="file" accept=".csv,text/csv" onChange={(e) => void onFile(e)} />
          </label>
          <TextArea
            rows={8}
            value={text}
            placeholder={EXEMPLO}
            onChange={(e) => setText(e.target.value)}
            aria-label={t('org.csv.colar')}
          />
          {text.trim() !== '' && parsed.errorKey && <Alert tone="danger">{t(parsed.errorKey)}</Alert>}
          {err && <Alert tone="danger">{err}</Alert>}
          {!parsed.errorKey && parsed.rows.length > 0 && (
            <>
              <p className="dx-muted">{t('org.csv.pre', { count: parsed.rows.length })}</p>
              <div className="dx-table-wrap org-table-wrap">
                <table className="dx-table org-table">
                  <thead>
                    <tr>
                      <th scope="col">{t('org.campo.email')}</th>
                      <th scope="col">{t('org.campo.cargo')}</th>
                      <th scope="col">{t('org.campo.papel')}</th>
                      <th scope="col">{t('org.campo.filial')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {parsed.rows.slice(0, 20).map((r, i) => (
                      <tr key={`${r.email}-${i}`}>
                        <td className="dx-num">{r.email}</td>
                        <td>{r.title ?? <span className="dx-muted">—</span>}</td>
                        <td>{r.role ?? <span className="dx-muted">—</span>}</td>
                        <td>{r.branch ?? <span className="dx-muted">—</span>}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {parsed.rows.length > 20 && <p className="dx-muted">{t('org.csv.maisLinhas', { count: parsed.rows.length - 20 })}</p>}
            </>
          )}
        </div>
      )}
    </Dialog>
  )
}
