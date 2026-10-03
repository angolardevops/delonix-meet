/**
 * «Operadoras ligadas»: os troncos pela ordem de encaminhamento, e as escritas
 * sobre eles — criar (formulário ou assistente de operadora móvel), editar,
 * activar/desactivar, apagar, preços e ordem.
 *
 * A ordem É o encaminhamento: muda-se a arrastar pela pega ou, sem rato, com os
 * botões «subir»/«descer» de cada linha. Os dois fazem o mesmo pedido
 * (`setTrunkOrder`, que leva a lista INTEIRA) — por isso só se reordena com
 * todas as operadoras carregadas.
 */
import { DragEvent, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { deleteTrunk, setTrunkOrder, updateTrunk } from '../../api'
import type { Trunk } from '../../api'
import { Async, AsyncSection } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { Alert, Button, Card, Dialog, IconButton, StatusBadge, Tag } from '../../ui/kit'
import { useLocaleTag } from '../admin/orgShared'
import { formatMoney, formatNumber, formatRatio, measured, nextToken, sortTrunks, trunkTone } from './format'
import OperatorWizard from './OperatorWizard'
import PricesDialog from './PricesDialog'
import { LoadMore, NA, useTelecomText } from './shared'
import TrunkDialog from './TrunkDialog'
import { moveItem, moveTo } from './trunkForm'
import type { PagedList } from './usePaged'

type Dir = -1 | 1

function TrunkItem({
  trunk: k,
  ordinal,
  total,
  canOrder,
  busy,
  dropTarget,
  onMove,
  onDragStart,
  onDragOver,
  onDrop,
  onDragEnd,
  onEdit,
  onPrices,
  onToggle,
  onDelete,
}: {
  trunk: Trunk
  ordinal: number
  total: number
  canOrder: boolean
  busy: boolean
  dropTarget: boolean
  onMove: (dir: Dir) => void
  onDragStart: (e: DragEvent<HTMLElement>) => void
  onDragOver: (e: DragEvent<HTMLElement>) => void
  onDrop: (e: DragEvent<HTMLElement>) => void
  onDragEnd: () => void
  onEdit: () => void
  onPrices: () => void
  onToggle: () => void
  onDelete: () => void
}) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const { reason, label } = useTelecomText()
  const inUse = measured(k.status.channels_in_use)
  const asr = measured(k.status.asr)
  const semMedicao = <span className="dx-muted">{t('telecom.semMedicao')}</span>
  return (
    <li className="tel-trunk" data-trunk={k.id} data-drop={dropTarget || undefined} onDragOver={onDragOver} onDrop={onDrop}>
      <div className="tel-trunk__head">
        {canOrder && (
          <span className="tel-trunk__order">
            {/* A pega é só para o rato; o teclado e o leitor de ecrã usam os botões ao lado. */}
            <span className="tel-grip" draggable={!busy} onDragStart={onDragStart} onDragEnd={onDragEnd} title={t('telecom.operadoras.arrastar')} aria-hidden="true">
              <Icon name="menu" />
            </span>
            <IconButton
              icon="chevronUp"
              bare
              label={t('telecom.operadoras.subir', { nome: k.name })}
              disabled={busy || ordinal === 1}
              data-move={`${k.id}:-1`}
              onClick={() => onMove(-1)}
            />
            <IconButton
              icon="chevronDown"
              bare
              label={t('telecom.operadoras.descer', { nome: k.name })}
              disabled={busy || ordinal === total}
              data-move={`${k.id}:1`}
              onClick={() => onMove(1)}
            />
          </span>
        )}
        <span className="dx-num dx-muted tel-trunk__ord" aria-label={t('telecom.operadoras.ordem', { n: ordinal })}>
          {ordinal}
        </span>
        <Tag>{k.short_code}</Tag>
        <span className="tel-trunk__id">
          <strong>{k.name}</strong>
          <span className="dx-num dx-muted tel-break">
            {k.host}:{k.port} · {k.transport.toUpperCase()}
          </span>
        </span>
        <span className="tel-trunk__state">
          {!k.enabled && (
            <StatusBadge tone="neutral" icon="ban">
              {t('telecom.operadoras.desactivada')}
            </StatusBadge>
          )}
          <StatusBadge tone={trunkTone(k.status.state)}>{label('tronco', k.status.state)}</StatusBadge>
        </span>
      </div>
      {k.status.reasons.length > 0 && (
        <ul className="tel-reasons">
          {k.status.reasons.map((c) => (
            <li key={c}>{reason(c)}</li>
          ))}
        </ul>
      )}
      <dl className="tel-trunk__facts">
        <div>
          <dt className="dx-eyebrow">{t('telecom.operadoras.canais')}</dt>
          <dd className="dx-num">
            {inUse === null ? semMedicao : formatNumber(inUse, locale)}
            <span className="dx-muted"> {t('telecom.sbc.deMax', { max: formatNumber(k.status.channels_max, locale) })}</span>
          </dd>
        </div>
        <div>
          <dt className="dx-eyebrow">{t('telecom.operadoras.prefixos')}</dt>
          <dd className="dx-num tel-break">{k.prefixes.length > 0 ? k.prefixes.join(' · ') : NA}</dd>
        </div>
        <div>
          <dt className="dx-eyebrow">{t('telecom.operadoras.custoMin')}</dt>
          <dd className="dx-num">
            {k.current_price_per_min ? (
              formatMoney(k.current_price_per_min, locale)
            ) : (
              <span className="dx-muted">{t('telecom.razao.no_price_in_force')}</span>
            )}
          </dd>
        </div>
        <div>
          <dt className="dx-eyebrow">{t('telecom.operadoras.asr')}</dt>
          <dd className="dx-num">
            {asr === null ? (
              <>
                {semMedicao}
                {k.status.asr_reason && <span className="dx-muted"> · {reason(k.status.asr_reason)}</span>}
              </>
            ) : (
              <>
                {formatRatio(asr, locale)}
                <span className="dx-muted">
                  {' '}
                  {t('telecom.operadoras.asrBase', {
                    atendidas: k.status.asr_answered,
                    tentativas: k.status.asr_attempts,
                    horas: k.status.asr_window_hours,
                  })}
                </span>
              </>
            )}
          </dd>
        </div>
      </dl>
      <div className="tel-actions" role="group" aria-label={t('telecom.operadoras.accoes', { nome: k.name })}>
        <Button size="sm" variant="secondary" icon="edit" onClick={onEdit} disabled={busy}>
          {t('ui.editar')}
        </Button>
        <Button size="sm" variant="secondary" onClick={onPrices} disabled={busy}>
          {t('telecom.operadoras.precos')}
        </Button>
        <Button size="sm" variant="secondary" onClick={onToggle} disabled={busy}>
          {k.enabled ? t('telecom.operadoras.desactivar') : t('telecom.operadoras.activar')}
        </Button>
        <Button size="sm" variant="danger" icon="trash" onClick={onDelete} disabled={busy}>
          {t('telecom.operadoras.apagar')}
        </Button>
      </div>
    </li>
  )
}

function DeleteTrunkDialog({ orgId, trunk, onClose, onDeleted }: { orgId: string; trunk: Trunk; onClose: () => void; onDeleted: () => void }) {
  const { t } = useTranslation()
  const { failure } = useTelecomText()
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  async function confirm() {
    setBusy(true)
    setErr('')
    try {
      await deleteTrunk(orgId, trunk.id)
      onDeleted()
    } catch (x) {
      // Em uso pelo plano (`telephony.trunk_in_use`): diz-se o que fazer primeiro.
      setErr(failure(x))
      setBusy(false)
    }
  }
  return (
    <Dialog title={t('telecom.operadoras.apagarTitulo', { nome: trunk.name })} onClose={onClose}>
      <p className="tel-small">{t('telecom.operadoras.apagarTexto')}</p>
      {err && <Alert tone="danger">{err}</Alert>}
      <div className="tel-form__foot">
        <Button variant="secondary" onClick={onClose} disabled={busy}>
          {t('ui.cancelar')}
        </Button>
        <Button variant="danger" icon="trash" busy={busy} onClick={() => void confirm()}>
          {t('telecom.operadoras.apagar')}
        </Button>
      </div>
    </Dialog>
  )
}

type Open = { kind: 'new' } | { kind: 'wizard' } | { kind: 'edit'; trunk: Trunk } | { kind: 'prices'; trunk: Trunk } | { kind: 'delete'; trunk: Trunk } | null

export default function TrunksCard({
  orgId,
  state,
  reload,
  loadMore,
  busy,
  err,
  mutate,
  onChanged,
}: {
  orgId: string
  state: Async<PagedList<Trunk>>
  reload: () => void
  loadMore: () => void
  busy: boolean
  err: string
  /** Troca a lista local (ordem optimista); o próximo `reload` repõe a do servidor. */
  mutate: (fn: (d: PagedList<Trunk>) => PagedList<Trunk>) => void
  /** Algo mudou nas operadoras: o resto da página (estado, plano) deve reler. */
  onChanged: () => void
}) {
  const { t } = useTranslation()
  const { failure } = useTelecomText()
  const [open, setOpen] = useState<Open>(null)
  const [working, setWorking] = useState(false)
  const [actionErr, setActionErr] = useState('')
  const [announce, setAnnounce] = useState('')
  const [drag, setDrag] = useState<{ from: string; over: string | null } | null>(null)
  const list = useRef<HTMLOListElement>(null)
  const focusAfter = useRef<{ id: string; dir: Dir } | null>(null)

  // Depois de mover por teclado, o foco fica no botão que se usou — ou, se a
  // linha chegou ao topo/fundo e ele ficou inactivo, no do sentido contrário.
  useEffect(() => {
    const f = focusAfter.current
    if (!f || working) return
    focusAfter.current = null
    const btn = (dir: Dir) => list.current?.querySelector<HTMLButtonElement>(`[data-move="${f.id}:${dir}"]`)
    const alvo = btn(f.dir)
    ;(alvo && !alvo.disabled ? alvo : btn(f.dir === 1 ? -1 : 1))?.focus()
  })

  const changed = () => {
    reload()
    onChanged()
  }

  async function applyOrder(next: Trunk[], moved: Trunk) {
    setWorking(true)
    setActionErr('')
    // Optimista: a posição passa a ser o índice, para a lista se mostrar já na ordem nova.
    mutate((d) => ({ ...d, items: next.map((k, i) => ({ ...k, position: i })) }))
    try {
      const page = await setTrunkOrder(orgId, next.map((k) => k.id))
      if (nextToken(page) === null) mutate((d) => ({ ...d, items: page.items }))
      else reload()
      setAnnounce(t('telecom.operadoras.movida', { nome: moved.name, n: next.findIndex((k) => k.id === moved.id) + 1, total: next.length }))
      onChanged()
    } catch (x) {
      setActionErr(failure(x))
      reload()
    } finally {
      setWorking(false)
    }
  }

  async function toggle(k: Trunk) {
    setWorking(true)
    setActionErr('')
    try {
      await updateTrunk(orgId, k.id, { enabled: !k.enabled })
      changed()
    } catch (x) {
      setActionErr(failure(x))
    } finally {
      setWorking(false)
    }
  }

  const close = () => setOpen(null)
  const saved = () => {
    close()
    changed()
  }

  return (
    <Card title={t('telecom.operadoras.titulo')} eyebrow={t('telecom.operadoras.eyebrow')} flush className="tel-card">
      <div className="tel-actions tel-actions--bar">
        <Button size="sm" variant="primary" icon="phone" onClick={() => setOpen({ kind: 'wizard' })}>
          {t('telecom.assistente.abrir')}
        </Button>
        <Button size="sm" variant="secondary" icon="plus" onClick={() => setOpen({ kind: 'new' })}>
          {t('telecom.form.tituloNovo')}
        </Button>
      </div>
      <AsyncSection state={state} onRetry={reload}>
        {(d) => {
          if (d.items.length === 0) return <p className="dx-muted tel-note">{t('telecom.operadoras.vazio')}</p>
          const sorted = sortTrunks(d.items)
          // Sem a lista inteira não há ordem para mandar.
          const canOrder = d.next === null && sorted.length > 1
          const move = (k: Trunk, i: number, dir: Dir) => {
            const next = moveItem(sorted, i, dir)
            if (next === sorted) return
            focusAfter.current = { id: k.id, dir }
            void applyOrder(next, k)
          }
          const drop = (toId: string) => {
            const from = sorted.findIndex((k) => k.id === drag?.from)
            const to = sorted.findIndex((k) => k.id === toId)
            setDrag(null)
            const next = moveTo(sorted, from, to)
            if (next !== sorted) void applyOrder(next, sorted[from])
          }
          return (
            <>
              {(d.next !== null || canOrder) && (
                <p className="dx-muted tel-note">{d.next !== null ? t('telecom.operadoras.ordemParcial') : t('telecom.operadoras.ordemAjuda')}</p>
              )}
              <ol className="tel-trunks" data-testid="tel-trunks" ref={list}>
                {sorted.map((k, i) => (
                  <TrunkItem
                    key={k.id}
                    trunk={k}
                    ordinal={i + 1}
                    total={sorted.length}
                    canOrder={canOrder}
                    busy={working}
                    dropTarget={drag !== null && drag.over === k.id && drag.from !== k.id}
                    onMove={(dir) => move(k, i, dir)}
                    onDragStart={(e) => {
                      e.dataTransfer.effectAllowed = 'move'
                      e.dataTransfer.setData('text/plain', k.name)
                      const li = e.currentTarget.closest('li')
                      if (li) e.dataTransfer.setDragImage(li, 0, 0)
                      setDrag({ from: k.id, over: null })
                    }}
                    onDragOver={(e) => {
                      if (!drag) return
                      e.preventDefault()
                      e.dataTransfer.dropEffect = 'move'
                      if (drag.over !== k.id) setDrag({ from: drag.from, over: k.id })
                    }}
                    onDrop={(e) => {
                      if (!drag) return
                      e.preventDefault()
                      drop(k.id)
                    }}
                    onDragEnd={() => setDrag(null)}
                    onEdit={() => setOpen({ kind: 'edit', trunk: k })}
                    onPrices={() => setOpen({ kind: 'prices', trunk: k })}
                    onToggle={() => void toggle(k)}
                    onDelete={() => setOpen({ kind: 'delete', trunk: k })}
                  />
                ))}
              </ol>
              <LoadMore next={d.next} busy={busy} err={err} onMore={loadMore} />
            </>
          )
        }}
      </AsyncSection>
      {actionErr && (
        <div className="tel-pad">
          <Alert tone="danger">{actionErr}</Alert>
        </div>
      )}
      <p className="dx-sr-only" role="status" aria-live="polite">
        {announce}
      </p>
      {open?.kind === 'new' && <TrunkDialog orgId={orgId} onClose={close} onDone={saved} />}
      {open?.kind === 'wizard' && <OperatorWizard orgId={orgId} onClose={close} onDone={saved} />}
      {open?.kind === 'edit' && <TrunkDialog orgId={orgId} trunk={open.trunk} onClose={close} onDone={saved} />}
      {open?.kind === 'prices' && <PricesDialog orgId={orgId} trunk={open.trunk} onClose={close} onChanged={changed} />}
      {open?.kind === 'delete' && <DeleteTrunkDialog orgId={orgId} trunk={open.trunk} onClose={close} onDeleted={saved} />}
    </Card>
  )
}
