/**
 * Na página Quadros: os diagramas editáveis que vivem NESTE browser.
 *
 * Não se misturam com a grelha de cima (a biblioteca da organização, no
 * servidor): um diagrama daqui só existe aqui até ser guardado como PNG, e a
 * lista di-lo. Sem diagramas, a secção não aparece.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { Alert, Button, Dialog, IconButton, Tag } from '../../ui/kit'
import '../../ui/diagrams-local.css'
import { formatDateTimeShort } from '../recordings/format'
import { deleteDiagram, DiagramSummary, listDiagrams } from './store'

export default function LocalDiagrams() {
  const { t, i18n } = useTranslation()
  const { state, reload, mutate } = useAsync(() => listDiagrams(), [])
  const [toDelete, setToDelete] = useState<DiagramSummary | null>(null)
  const [err, setErr] = useState('')

  if (state.s === 'ready' && state.d.length === 0) return null
  if (state.s === 'error') {
    return (
      <Alert tone="warning" icon="alert">
        {t('diagrams.local.erro')}
      </Alert>
    )
  }

  async function confirmDelete() {
    if (!toDelete) return
    try {
      await deleteDiagram(toDelete.id)
      const id = toDelete.id
      mutate((l) => l.filter((x) => x.id !== id))
      setToDelete(null)
    } catch {
      setErr(t('diagrams.local.erro'))
    }
  }

  return (
    <section className="board-local" aria-labelledby="board-local-title">
      <header className="board-local__head">
        <h2 id="board-local-title">{t('diagrams.local.titulo')}</h2>
        <p className="dx-muted">{t('diagrams.local.ajuda')}</p>
      </header>
      <AsyncSection state={state} onRetry={reload}>
        {(items) => (
          <ul className="board-local__list">
            {items.map((d) => {
              const title = d.title || t('diagrams.semTitulo')
              return (
                <li key={d.id} className="board-local__item">
                  <a className="board-local__open" href={`#/whiteboards/diagram/${d.id}`} aria-label={t('diagrams.local.abrir', { title })}>
                    <Icon name="board" size={16} />
                    <span className="board-local__title">{title}</span>
                    <Tag plain>{t(`diagrams.notacoes.${d.notation}`)}</Tag>
                    <span className="dx-muted dx-num">{t('diagrams.local.elementos', { count: d.elements })}</span>
                    <span className="dx-muted dx-num">{formatDateTimeShort(d.updatedAt, i18n.language)}</span>
                    {d.roomCode && <span className="dx-muted dx-num">{d.roomCode}</span>}
                    {d.boardId && <Tag tone="success">{t('diagrams.local.comPng')}</Tag>}
                  </a>
                  <IconButton icon="trash" bare label={t('diagrams.local.eliminar', { title })} onClick={() => setToDelete(d)} />
                </li>
              )
            })}
          </ul>
        )}
      </AsyncSection>
      {toDelete && (
        <Dialog
          title={t('diagrams.local.eliminarTitulo')}
          onClose={() => setToDelete(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setToDelete(null)}>
                {t('ui.cancelar')}
              </Button>
              <Button variant="danger" icon="trash" onClick={() => void confirmDelete()}>
                {t('diagrams.inspector.eliminar')}
              </Button>
            </>
          }
        >
          <p className="board-delete__text">{t('diagrams.local.eliminarTexto', { title: toDelete.title || t('diagrams.semTitulo') })}</p>
          {err && <Alert tone="danger">{err}</Alert>}
        </Dialog>
      )}
    </section>
  )
}
