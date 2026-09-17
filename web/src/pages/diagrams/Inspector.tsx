/**
 * Coluna direita: propriedades do que está seleccionado, estilo, camadas,
 * pistas (BPMN) e validação.
 *
 * Cada alteração passa por `onChange(novo, chave)`: a chave junta as teclas
 * seguidas do mesmo campo num só passo de «desfazer».
 */
import { ReactNode, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../../ui/icons'
import { Button, cx, Field, IconButton, Select, TextArea, TextInput, Toggle } from '../../ui/kit'
import type { Sel } from './Canvas'
import { laneOf, laneTop, nodeBox, poolHeight } from './geometry'
import {
  BPMN_FLOW_NODES,
  CLASSIFIERS,
  DEdge,
  DiagramDoc,
  DNode,
  edgeTypesFor,
  EdgeType,
  EventTrigger,
  FRAGMENT_OPERATORS,
  FragmentOperator,
  GatewayKind,
  Lane,
  MultiInstance,
  NODE_NOTATION,
  Notation,
  TaskKind,
  uid,
} from './model'
import { FILLS } from './paint'
import { Issue, issueParams } from './validate'

export type InspectorTab = 'element' | 'style' | 'layers' | 'lanes' | 'validation'

export function tabsFor(notation: Notation): InspectorTab[] {
  // Os separadores do template: BPMN tem «Validação»; UML (e as notações que
  // seguem a mesma gramática) mostram a validação como cartão em «Elemento».
  return notation === 'bpmn' ? ['element', 'lanes', 'validation'] : ['element', 'style', 'layers']
}

type Change = (next: DiagramDoc, coalesce?: string) => void

const patchNode = (doc: DiagramDoc, id: string, fn: (n: DNode) => DNode): DiagramDoc => ({
  ...doc,
  nodes: doc.nodes.map((n) => (n.id === id ? fn(n) : n)),
})
const patchEdge = (doc: DiagramDoc, id: string, patch: Partial<DEdge>): DiagramDoc => ({
  ...doc,
  edges: doc.edges.map((e) => (e.id === id ? { ...e, ...patch } : e)),
})

function Section({ title, children, tone }: { title?: ReactNode; children: ReactNode; tone?: 'warning' }) {
  return (
    <section className={cx('dg-card', tone === 'warning' && 'is-warning')}>
      {title && <h3 className="dg-card__title">{title}</h3>}
      {children}
    </section>
  )
}

export default function Inspector({
  doc,
  notation,
  selection,
  tab,
  issues,
  typeLabel,
  showValidation = false,
  onTab,
  onChange,
  onSelect,
  onDelete,
  onDuplicate,
  onFix,
  onFixAll,
  footer,
}: {
  doc: DiagramDoc
  notation: Notation
  selection: Sel | null
  tab: InspectorTab
  issues: Issue[]
  typeLabel: (n: DNode) => string
  showValidation?: boolean
  onTab: (t: InspectorTab) => void
  onChange: Change
  onSelect: (s: Sel | null) => void
  onDelete: () => void
  onDuplicate: () => void
  onFix: (i: Issue) => void
  onFixAll: () => void
  footer?: ReactNode
}) {
  const { t } = useTranslation()
  const tabs = tabsFor(notation)
  const current = tabs.includes(tab) ? tab : 'element'
  const node = selection?.kind === 'node' ? doc.nodes.find((n) => n.id === selection.id) : undefined
  const edge = selection?.kind === 'edge' ? doc.edges.find((e) => e.id === selection.id) : undefined
  const stroke = selection?.kind === 'stroke' ? doc.strokes.find((s) => s.id === selection.id) : undefined

  return (
    <div className="dg-inspector">
      <div className="dg-seg" role="tablist" aria-label={t('diagrams.inspector.rotulo')}>
        {tabs.map((k) => (
          <button key={k} type="button" role="tab" aria-selected={current === k} onClick={() => onTab(k)}>
            {t(`diagrams.inspector.tabs.${k}`)}
            {k === 'validation' && issues.length > 0 && <span className="dg-seg__count dx-num">{issues.length}</span>}
          </button>
        ))}
      </div>

      <div className="dg-inspector__body" role="tabpanel">
        {current === 'element' &&
          (node ? (
            <NodePanel doc={doc} n={node} onChange={onChange} onSelect={onSelect} onDelete={onDelete} onDuplicate={onDuplicate} />
          ) : edge ? (
            <EdgePanel doc={doc} e={edge} onChange={onChange} onDelete={onDelete} />
          ) : stroke ? (
            <Section title={t('diagrams.inspector.traco')}>
              <Button size="sm" variant="danger" icon="trash" onClick={onDelete}>
                {t('diagrams.inspector.eliminar')}
              </Button>
            </Section>
          ) : (
            <p className="dg-empty">{t('diagrams.inspector.nada')}</p>
          ))}
        {current === 'element' && notation !== 'free' && (issues.length > 0 || (showValidation && notation !== 'bpmn')) && (
          <ValidationPanel doc={doc} issues={issues} typeLabel={typeLabel} onSelect={onSelect} onFix={onFix} onFixAll={onFixAll} />
        )}
        {current === 'style' && <StylePanel doc={doc} n={node} onChange={onChange} />}
        {current === 'layers' && <LayersPanel doc={doc} notation={notation} selection={selection} onChange={onChange} onSelect={onSelect} />}
        {current === 'lanes' && <LanesPanel doc={doc} selected={node} onChange={onChange} onSelect={onSelect} />}
        {current === 'validation' && <ValidationPanel doc={doc} issues={issues} typeLabel={typeLabel} onSelect={onSelect} onFix={onFix} onFixAll={onFixAll} />}
      </div>
      {footer}
    </div>
  )
}

// ---------------------------------------------------------------------------
//  Elemento
// ---------------------------------------------------------------------------

function MemberList({ label, items, placeholder, addLabel, onItems }: { label: string; items: string[]; placeholder: string; addLabel: string; onItems: (v: string[], coalesce?: string) => void }) {
  const { t } = useTranslation()
  const id = useId()
  return (
    <div className="dg-members" role="group" aria-labelledby={id}>
      <span id={id} className="dg-label">
        {label}
      </span>
      {items.map((m, i) => (
        <div key={i} className="dg-member">
          <TextInput code value={m} aria-label={`${label} ${i + 1}`} onChange={(e) => onItems(items.map((x, j) => (j === i ? e.target.value : x)), `${label}:${i}`)} />
          <IconButton icon="x" bare label={t('diagrams.inspector.remover', { item: m || label })} onClick={() => onItems(items.filter((_, j) => j !== i))} />
        </div>
      ))}
      <button type="button" className="dg-add" onClick={() => onItems([...items, placeholder])}>
        {addLabel}
      </button>
    </div>
  )
}

function NodePanel({ doc, n, onChange, onSelect, onDelete, onDuplicate }: { doc: DiagramDoc; n: DNode; onChange: Change; onSelect: (s: Sel | null) => void; onDelete: () => void; onDuplicate: () => void }) {
  const { t } = useTranslation()
  const set = (patch: Partial<DNode>, key?: string) => onChange(patchNode(doc, n.id, (x) => ({ ...x, ...patch })), key && `${n.id}:${key}`)
  const setProps = (patch: Partial<DNode['props']>, key?: string) =>
    onChange(patchNode(doc, n.id, (x) => ({ ...x, props: { ...x.props, ...patch } })), key && `${n.id}:${key}`)
  const bpmn = NODE_NOTATION[n.type] === 'bpmn'
  const isEvent = n.type === 'startEvent' || n.type === 'intermediateEvent' || n.type === 'endEvent'
  const nameIsText = n.type === 'note' || n.type === 'annotation'

  return (
    <>
      <Section
        title={
          <span className="dg-card__head">
            <span className="dg-dot" aria-hidden="true" />
            {t(`diagrams.tipos.${n.type}`)}
            {n.name && <span className="dg-card__name"> · {n.name}</span>}
          </span>
        }
      >
        {nameIsText ? (
          <Field label={t('diagrams.inspector.texto')}>
            <TextArea rows={4} value={n.props.text ?? n.name} onChange={(e) => setProps({ text: e.target.value }, 'text')} />
          </Field>
        ) : (
          <Field label={t('diagrams.inspector.nome')}>
            <TextInput value={n.name} onChange={(e) => set({ name: e.target.value }, 'name')} />
          </Field>
        )}

        {CLASSIFIERS.has(n.type) && (
          <>
            <Field label={t('diagrams.inspector.estereotipo')}>
              <TextInput code list="dg-stereotypes" value={n.props.stereotype ?? ''} onChange={(e) => setProps({ stereotype: e.target.value.replace(/[«»<>]/g, '') }, 'st')} />
            </Field>
            <datalist id="dg-stereotypes">
              {['entity', 'service', 'controller', 'boundary', 'value object', 'aggregate root', 'interface', 'enumeration'].map((s) => (
                <option key={s} value={s} />
              ))}
            </datalist>
            <Field label={t('diagrams.inspector.pacote')}>
              <TextInput code value={n.props.package ?? ''} onChange={(e) => setProps({ package: e.target.value }, 'pkg')} />
            </Field>
            <MemberList
              label={n.type === 'enum' ? t('diagrams.inspector.literais') : t('diagrams.inspector.atributos')}
              items={n.props.attributes ?? []}
              placeholder={n.type === 'enum' ? t('diagrams.inspector.novoLiteral') : t('diagrams.inspector.novoAtributo')}
              addLabel={n.type === 'enum' ? t('diagrams.inspector.acrescentarLiteral') : t('diagrams.inspector.acrescentarAtributo')}
              onItems={(v, k) => setProps({ attributes: v }, k)}
            />
            {n.type !== 'enum' && (
              <MemberList
                label={t('diagrams.inspector.operacoes')}
                items={n.props.operations ?? []}
                placeholder={t('diagrams.inspector.novaOperacao')}
                addLabel={t('diagrams.inspector.acrescentarOperacao')}
                onItems={(v, k) => setProps({ operations: v }, k)}
              />
            )}
          </>
        )}

        {n.type === 'fragment' && (
          <Field label={t('diagrams.inspector.operador')}>
            <Select value={n.props.operator ?? 'alt'} onChange={(e) => setProps({ operator: e.target.value as FragmentOperator })}>
              {FRAGMENT_OPERATORS.map((o) => (
                <option key={o} value={o}>
                  {o}
                </option>
              ))}
            </Select>
          </Field>
        )}
        {n.type === 'object' && (
          <>
            <Field label={t('diagrams.inspector.instanciaDe')}>
              <TextInput code list="dg-classes" value={n.props.instanceOf ?? ''} onChange={(e) => setProps({ instanceOf: e.target.value }, 'inst')} />
            </Field>
            <datalist id="dg-classes">
              {doc.nodes.filter((x) => CLASSIFIERS.has(x.type) && x.name.trim()).map((x) => (
                <option key={x.id} value={x.name.trim()} />
              ))}
            </datalist>
            <MemberList
              label={t('diagrams.inspector.slots')}
              items={n.props.attributes ?? []}
              placeholder={t('diagrams.inspector.novoSlot')}
              addLabel={t('diagrams.inspector.acrescentarSlot')}
              onItems={(v, k) => setProps({ attributes: v }, k)}
            />
          </>
        )}
        {n.type === 'state' && (
          <MemberList
            label={t('diagrams.inspector.actividadesInternas')}
            items={n.props.attributes ?? []}
            placeholder={t('diagrams.inspector.novaActividade')}
            addLabel={t('diagrams.inspector.acrescentarActividade')}
            onItems={(v, k) => setProps({ attributes: v }, k)}
          />
        )}
        {n.type === 'history' && <Toggle label={t('diagrams.inspector.historicoProfundo')} checked={!!n.props.deep} onChange={(e) => setProps({ deep: e.target.checked })} />}
        {(n.type === 'component' || n.type === 'deviceNode' || n.type === 'artifact') && (
          <>
            <Field label={t('diagrams.inspector.estereotipo')}>
              <TextInput code list={`dg-st-${n.type}`} value={n.props.stereotype ?? ''} onChange={(e) => setProps({ stereotype: e.target.value.replace(/[«»<>]/g, '') }, 'st')} />
            </Field>
            <datalist id={`dg-st-${n.type}`}>
              {(n.type === 'component' ? ['component', 'subsystem', 'service'] : n.type === 'deviceNode' ? ['device', 'executionEnvironment', 'container'] : ['artifact', 'file', 'library', 'executable']).map((x) => (
                <option key={x} value={x} />
              ))}
            </datalist>
          </>
        )}
        {n.type === 'lifeline' && (
          <Field label={t('diagrams.inspector.comprimento')}>
            <TextInput type="number" min={60} step={20} value={n.props.length ?? 240} onChange={(e) => setProps({ length: Math.max(60, Number(e.target.value) || 60) }, 'len')} />
          </Field>
        )}
        {(n.type === 'service' || n.type === 'client' || n.type === 'external') && (
          <Field label={t('diagrams.inspector.tecnologia')}>
            <TextInput code value={n.props.stereotype ?? ''} onChange={(e) => setProps({ stereotype: e.target.value }, 'tech')} />
          </Field>
        )}

        {isEvent && (
          <>
            <Field label={t('diagrams.inspector.tipo')}>
              <Select
                value={n.type}
                onChange={(e) => {
                  const type = e.target.value as DNode['type']
                  const trigger = type === 'intermediateEvent' && (n.props.trigger ?? 'none') === 'none' ? 'timer' : n.props.trigger
                  onChange(patchNode(doc, n.id, (x) => ({ ...x, type, props: { ...x.props, trigger } })))
                }}
              >
                {(['startEvent', 'intermediateEvent', 'endEvent'] as const).map((k) => (
                  <option key={k} value={k}>
                    {t(`diagrams.opcoes.evento.${k}`)}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label={t('diagrams.inspector.gatilho')}>
              <Select value={n.props.trigger ?? 'none'} onChange={(e) => setProps({ trigger: e.target.value as EventTrigger })}>
                {(['none', 'message', 'timer', 'signal'] as const)
                  .filter((k) => !(n.type === 'intermediateEvent' && k === 'none') && !(n.type === 'endEvent' && k === 'timer'))
                  .map((k) => (
                    <option key={k} value={k}>
                      {t(`diagrams.opcoes.gatilho.${k}`)}
                    </option>
                  ))}
              </Select>
            </Field>
          </>
        )}
        {(n.type === 'task' || n.type === 'subProcess') && (
          <>
            <Field label={t('diagrams.inspector.tipo')}>
              <Select
                value={n.type === 'subProcess' ? 'subProcess' : n.props.taskKind ?? 'none'}
                onChange={(e) => {
                  const v = e.target.value
                  onChange(
                    patchNode(doc, n.id, (x) =>
                      v === 'subProcess'
                        ? { ...x, type: 'subProcess', w: Math.max(x.w, 150), h: Math.max(x.h, 80) }
                        : { ...x, type: 'task', props: { ...x.props, taskKind: v as TaskKind } },
                    ),
                  )
                }}
              >
                {(['none', 'user', 'service', 'script', 'manual', 'send', 'receive'] as const).map((k) => (
                  <option key={k} value={k}>
                    {t(`diagrams.opcoes.tarefa.${k}`)}
                  </option>
                ))}
                <option value="subProcess">{t('diagrams.tipos.subProcess')}</option>
              </Select>
            </Field>
            {n.type === 'task' && (
              <>
                <Field label={t('diagrams.inspector.executor')}>
                  <TextInput code value={n.props.implementation ?? ''} onChange={(e) => setProps({ implementation: e.target.value }, 'impl')} />
                </Field>
                <Field label={t('diagrams.inspector.multiInstancia')}>
                  <Select value={n.props.multiInstance ?? 'none'} onChange={(e) => setProps({ multiInstance: e.target.value as MultiInstance })}>
                    {(['none', 'parallel', 'sequential'] as const).map((k) => (
                      <option key={k} value={k}>
                        {t(`diagrams.opcoes.multi.${k}`)}
                      </option>
                    ))}
                  </Select>
                </Field>
              </>
            )}
          </>
        )}
        {n.type === 'gateway' && (
          <Field label={t('diagrams.inspector.tipo')}>
            <Select value={n.props.gatewayKind ?? 'exclusive'} onChange={(e) => setProps({ gatewayKind: e.target.value as GatewayKind })}>
              {(['exclusive', 'parallel', 'inclusive', 'eventBased'] as const).map((k) => (
                <option key={k} value={k}>
                  {t(`diagrams.opcoes.gateway.${k}`)}
                </option>
              ))}
            </Select>
          </Field>
        )}
        {bpmn && n.type !== 'pool' && <LaneField doc={doc} n={n} onChange={onChange} />}

        <div className="dg-actions">
          <Button size="sm" variant="ghost" icon="copy" onClick={onDuplicate}>
            {t('diagrams.inspector.duplicar')}
          </Button>
          <Button size="sm" variant="ghost" icon="trash" onClick={onDelete}>
            {t('diagrams.inspector.eliminar')}
          </Button>
        </div>
      </Section>

      {bpmn && BPMN_FLOW_NODES.has(n.type) ? (
        <FlowsPanel doc={doc} n={n} onChange={onChange} onSelect={onSelect} />
      ) : (
        n.type !== 'text' && n.type !== 'pool' && <RelationsPanel doc={doc} n={n} onChange={onChange} onSelect={onSelect} />
      )}
    </>
  )
}

function LaneField({ doc, n, onChange }: { doc: DiagramDoc; n: DNode; onChange: Change }) {
  const { t } = useTranslation()
  const pools = doc.nodes.filter((p) => p.type === 'pool' && (p.props.lanes?.length ?? 0) > 0)
  if (pools.length === 0) return null
  const cur = laneOf(doc, n)
  const move = (value: string) => {
    const [poolId, laneId] = value.split('/')
    const pool = doc.nodes.find((p) => p.id === poolId)
    const lane = pool?.props.lanes?.find((l) => l.id === laneId)
    if (!pool || !lane) return
    const top = laneTop(pool, lane.id)!
    const b = nodeBox(n)
    const y = Math.round(top + lane.size / 2 - b.h / 2)
    const x = cur?.pool.id === pool.id ? n.x : Math.max(n.x, pool.x + 70)
    onChange(patchNode(doc, n.id, (x0) => ({ ...x0, x, y })))
  }
  return (
    <Field label={t('diagrams.inspector.pista')}>
      <Select value={cur ? `${cur.pool.id}/${cur.lane.id}` : ''} onChange={(e) => move(e.target.value)}>
        {!cur && <option value="">{t('diagrams.inspector.semPista')}</option>}
        {pools.map((p) => (
          <optgroup key={p.id} label={p.name || t('diagrams.tipos.pool')}>
            {(p.props.lanes ?? []).map((l) => (
              <option key={l.id} value={`${p.id}/${l.id}`}>
                {l.name || t('diagrams.semNome')}
              </option>
            ))}
          </optgroup>
        ))}
      </Select>
    </Field>
  )
}

const label = (t: (k: string) => string, n: DNode | undefined) => (n ? n.name || n.props.text || t('diagrams.semNome') : '?')

function RelationsPanel({ doc, n, onChange, onSelect }: { doc: DiagramDoc; n: DNode; onChange: Change; onSelect: (s: Sel | null) => void }) {
  const { t } = useTranslation()
  const [adding, setAdding] = useState(false)
  const [target, setTarget] = useState('')
  const [type, setType] = useState<EdgeType | ''>('')
  const byId = new Map(doc.nodes.map((x) => [x.id, x]))
  const rels = doc.edges.filter((e) => e.from === n.id || e.to === n.id)
  const candidates = doc.nodes.filter((x) => x.id !== n.id && edgeTypesFor(n, x).length > 0)
  const targetNode = byId.get(target)
  const types = targetNode ? edgeTypesFor(n, targetNode) : []

  function create() {
    if (!targetNode || !type) return
    const e: DEdge = { id: uid('e'), type, from: n.id, to: targetNode.id, label: '' }
    onChange({ ...doc, edges: [...doc.edges, e] })
    setAdding(false)
    setTarget('')
    setType('')
    onSelect({ kind: 'edge', id: e.id })
  }

  return (
    <Section title={t('diagrams.inspector.relacoes')}>
      {rels.length === 0 ? (
        <p className="dg-muted">{t('diagrams.inspector.semRelacoes')}</p>
      ) : (
        <ul className="dg-list">
          {rels.map((e) => {
            const other = byId.get(e.from === n.id ? e.to : e.from)
            const mult = [e.srcMult, e.dstMult].filter(Boolean).join(' → ')
            return (
              <li key={e.id}>
                <button type="button" className="dg-row" onClick={() => onSelect({ kind: 'edge', id: e.id })}>
                  <span className="dg-row__k dx-num">{mult || (e.from === n.id ? '→' : '←')}</span>
                  <span className="dg-row__t">
                    {label(t, other)} · {t(`diagrams.arestas.${e.type}`)}
                  </span>
                </button>
              </li>
            )
          })}
        </ul>
      )}
      {adding ? (
        <div className="dg-form">
          <Field label={t('diagrams.inspector.destino')}>
            <Select
              value={target}
              onChange={(e) => {
                setTarget(e.target.value)
                const x = byId.get(e.target.value)
                setType(x ? edgeTypesFor(n, x)[0] ?? '' : '')
              }}
            >
              <option value="">—</option>
              {candidates.map((x) => (
                <option key={x.id} value={x.id}>
                  {label(t, x)} · {t(`diagrams.tipos.${x.type}`)}
                </option>
              ))}
            </Select>
          </Field>
          {types.length > 0 && (
            <Field label={t('diagrams.inspector.tipoRelacao')}>
              <Select value={type} onChange={(e) => setType(e.target.value as EdgeType)}>
                {types.map((k) => (
                  <option key={k} value={k}>
                    {t(`diagrams.arestas.${k}`)}
                  </option>
                ))}
              </Select>
            </Field>
          )}
          <div className="dg-actions">
            <Button size="sm" variant="primary" disabled={!targetNode || !type} onClick={create}>
              {t('diagrams.inspector.criar')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setAdding(false)}>
              {t('ui.cancelar')}
            </Button>
          </div>
        </div>
      ) : (
        candidates.length > 0 && (
          <button type="button" className="dg-add" onClick={() => setAdding(true)}>
            {t('diagrams.inspector.novaRelacao')}
          </button>
        )
      )}
    </Section>
  )
}

function FlowsPanel({ doc, n, onChange, onSelect }: { doc: DiagramDoc; n: DNode; onChange: Change; onSelect: (s: Sel | null) => void }) {
  const { t } = useTranslation()
  const [adding, setAdding] = useState<null | 'plain' | 'conditional'>(null)
  const [target, setTarget] = useState('')
  const [condition, setCondition] = useState('')
  const byId = new Map(doc.nodes.map((x) => [x.id, x]))
  const outs = doc.edges.filter((e) => e.from === n.id && (e.type === 'sequenceFlow' || e.type === 'messageFlow'))
  const candidates = doc.nodes.filter((x) => x.id !== n.id && BPMN_FLOW_NODES.has(x.type))
  const canDefault = n.type === 'task' || n.type === 'subProcess' || (n.type === 'gateway' && (n.props.gatewayKind ?? 'exclusive') !== 'parallel' && n.props.gatewayKind !== 'eventBased')

  function create() {
    const to = byId.get(target)
    if (!to) return
    const e: DEdge = { id: uid('e'), type: 'sequenceFlow', from: n.id, to: to.id, label: '', condition: adding === 'conditional' ? condition.trim() : undefined }
    onChange({ ...doc, edges: [...doc.edges, e] })
    setAdding(null)
    setTarget('')
    setCondition('')
  }

  return (
    <Section title={t('diagrams.inspector.fluxosSaida')}>
      {outs.length === 0 ? (
        <p className="dg-muted">{t('diagrams.inspector.semFluxos')}</p>
      ) : (
        <ul className="dg-list">
          {outs.map((e) => (
            <li key={e.id} className="dg-flow">
              <button type="button" className="dg-row" onClick={() => onSelect({ kind: 'edge', id: e.id })}>
                <span className="dg-row__k">{e.type === 'messageFlow' ? '⇢' : '→'}</span>
                <span className="dg-row__t">
                  {label(t, byId.get(e.to))}
                  {e.condition?.trim() && <span className="dg-row__cond dx-num"> [{e.condition.trim()}]</span>}
                </span>
              </button>
              {canDefault && e.type === 'sequenceFlow' && (
                <label className="dg-default" title={t('diagrams.inspector.porOmissao')}>
                  <input
                    type="radio"
                    name={`default-${n.id}`}
                    checked={!!e.isDefault}
                    aria-label={t('diagrams.inspector.porOmissaoDe', { destino: label(t, byId.get(e.to)) })}
                    onChange={() => onChange({ ...doc, edges: doc.edges.map((x) => (x.from === n.id && x.type === 'sequenceFlow' ? { ...x, isDefault: x.id === e.id, condition: x.id === e.id ? '' : x.condition } : x)) })}
                  />
                </label>
              )}
            </li>
          ))}
        </ul>
      )}
      {adding ? (
        <div className="dg-form">
          <Field label={t('diagrams.inspector.destino')}>
            <Select value={target} onChange={(e) => setTarget(e.target.value)}>
              <option value="">—</option>
              {candidates.map((x) => (
                <option key={x.id} value={x.id}>
                  {label(t, x)} · {t(`diagrams.tipos.${x.type}`)}
                </option>
              ))}
            </Select>
          </Field>
          {adding === 'conditional' && (
            <Field label={t('diagrams.inspector.condicao')}>
              <TextInput code value={condition} onChange={(e) => setCondition(e.target.value)} />
            </Field>
          )}
          <div className="dg-actions">
            <Button size="sm" variant="primary" disabled={!target || (adding === 'conditional' && !condition.trim())} onClick={create}>
              {t('diagrams.inspector.criar')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setAdding(null)}>
              {t('ui.cancelar')}
            </Button>
          </div>
        </div>
      ) : (
        candidates.length > 0 && (
          <div className="dg-add-row">
            <button type="button" className="dg-add" onClick={() => setAdding('plain')}>
              {t('diagrams.inspector.fluxo')}
            </button>
            {n.type !== 'endEvent' && (
              <button type="button" className="dg-add" onClick={() => setAdding('conditional')}>
                {t('diagrams.inspector.fluxoCondicional')}
              </button>
            )}
          </div>
        )
      )}
    </Section>
  )
}

// ---------------------------------------------------------------------------
//  Aresta
// ---------------------------------------------------------------------------

function EdgePanel({ doc, e, onChange, onDelete }: { doc: DiagramDoc; e: DEdge; onChange: Change; onDelete: () => void }) {
  const { t } = useTranslation()
  const byId = new Map(doc.nodes.map((x) => [x.id, x]))
  const a = byId.get(e.from)
  const b = byId.get(e.to)
  const set = (patch: Partial<DEdge>, key?: string) => onChange(patchEdge(doc, e.id, patch), key && `${e.id}:${key}`)
  const types = a && b ? edgeTypesFor(a, b) : [e.type]
  const assoc = e.type === 'association' || e.type === 'aggregation' || e.type === 'composition'
  const src = a?.type
  const canCondition = e.type === 'controlFlow' || (e.type === 'sequenceFlow' && src !== 'startEvent') && !(src === 'gateway' && (a?.props.gatewayKind === 'parallel' || a?.props.gatewayKind === 'eventBased'))
  const canDefault = e.type === 'sequenceFlow' && (src === 'task' || src === 'subProcess' || (src === 'gateway' && (a?.props.gatewayKind ?? 'exclusive') !== 'parallel' && a?.props.gatewayKind !== 'eventBased'))

  return (
    <Section
      title={
        <span className="dg-card__head">
          <span className="dg-dot" aria-hidden="true" />
          {t(`diagrams.arestas.${e.type}`)}
        </span>
      }
    >
      <p className="dg-muted">{t('diagrams.inspector.ligacao', { de: label(t, a), para: label(t, b) })}</p>
      <Field label={t('diagrams.inspector.tipo')}>
        <Select value={e.type} onChange={(ev) => set({ type: ev.target.value as EdgeType })}>
          {[...new Set([e.type, ...types])].map((k) => (
            <option key={k} value={k}>
              {t(`diagrams.arestas.${k}`)}
            </option>
          ))}
        </Select>
      </Field>
      {e.type !== 'anchor' && (
        <Field label={t('diagrams.inspector.etiqueta')}>
          <TextInput value={e.label} placeholder={e.type === 'transition' ? t('diagrams.inspector.transicaoAjuda') : undefined} onChange={(ev) => set({ label: ev.target.value }, 'label')} />
        </Field>
      )}
      {assoc && (
        <div className="dg-pair">
          <Field label={t('diagrams.inspector.multOrigem')}>
            <TextInput code list="dg-mults" value={e.srcMult ?? ''} onChange={(ev) => set({ srcMult: ev.target.value }, 'src')} />
          </Field>
          <Field label={t('diagrams.inspector.multDestino')}>
            <TextInput code list="dg-mults" value={e.dstMult ?? ''} onChange={(ev) => set({ dstMult: ev.target.value }, 'dst')} />
          </Field>
          <datalist id="dg-mults">
            {['1', '0..1', '*', '0..*', '1..*'].map((m) => (
              <option key={m} value={m} />
            ))}
          </datalist>
        </div>
      )}
      {canCondition && (
        <Field label={t(e.type === 'controlFlow' ? 'diagrams.inspector.guarda' : 'diagrams.inspector.condicao')}>
          <TextInput code value={e.condition ?? ''} disabled={!!e.isDefault} onChange={(ev) => set({ condition: ev.target.value }, 'cond')} />
        </Field>
      )}
      {canDefault && (
        <Toggle
          label={t('diagrams.inspector.porOmissao')}
          checked={!!e.isDefault}
          onChange={(ev) =>
            onChange({
              ...doc,
              edges: doc.edges.map((x) =>
                x.id === e.id ? { ...x, isDefault: ev.target.checked, condition: ev.target.checked ? '' : x.condition } : ev.target.checked && x.from === e.from && x.type === 'sequenceFlow' ? { ...x, isDefault: false } : x,
              ),
            })
          }
        />
      )}
      <div className="dg-actions">
        <Button size="sm" variant="ghost" icon="repeat" onClick={() => set({ from: e.to, to: e.from, srcMult: e.dstMult, dstMult: e.srcMult })}>
          {t('diagrams.inspector.inverter')}
        </Button>
        <Button size="sm" variant="ghost" icon="trash" onClick={onDelete}>
          {t('diagrams.inspector.eliminar')}
        </Button>
      </div>
    </Section>
  )
}

// ---------------------------------------------------------------------------
//  Estilo, camadas, pistas, validação
// ---------------------------------------------------------------------------

function StylePanel({ doc, n, onChange }: { doc: DiagramDoc; n: DNode | undefined; onChange: Change }) {
  const { t } = useTranslation()
  if (!n) return <p className="dg-empty">{t('diagrams.inspector.nada')}</p>
  return (
    <Section title={t('diagrams.inspector.preenchimento')}>
      <div className="dg-swatches" role="group" aria-label={t('diagrams.inspector.preenchimento')}>
        <button type="button" className="dg-swatch is-none" aria-pressed={!n.fill} aria-label={t('diagrams.inspector.cores.auto')} title={t('diagrams.inspector.cores.auto')} onClick={() => onChange(patchNode(doc, n.id, (x) => ({ ...x, fill: undefined })))}>
          <Icon name="ban" size={14} />
        </button>
        {Object.entries(FILLS).map(([k, c]) => (
          <button key={k} type="button" className="dg-swatch" style={{ background: c }} aria-pressed={n.fill === k} aria-label={t(`diagrams.inspector.cores.${k}`)} title={t(`diagrams.inspector.cores.${k}`)} onClick={() => onChange(patchNode(doc, n.id, (x) => ({ ...x, fill: k })))} />
        ))}
      </div>
      <Toggle label={t('diagrams.inspector.emDestaque')} checked={!!n.props.emphasis} onChange={(e) => onChange(patchNode(doc, n.id, (x) => ({ ...x, props: { ...x.props, emphasis: e.target.checked } })))} />
    </Section>
  )
}

function LayersPanel({ doc, notation, selection, onChange, onSelect }: { doc: DiagramDoc; notation: Notation; selection: Sel | null; onChange: Change; onSelect: (s: Sel | null) => void }) {
  const { t } = useTranslation()
  const ordered = [...doc.nodes].reverse()
  const move = (id: string, dir: 1 | -1) => {
    const i = doc.nodes.findIndex((n) => n.id === id)
    const j = i + dir
    if (i < 0 || j < 0 || j >= doc.nodes.length) return
    const nodes = [...doc.nodes]
    ;[nodes[i], nodes[j]] = [nodes[j], nodes[i]]
    onChange({ ...doc, nodes })
  }
  if (doc.nodes.length === 0) return <p className="dg-empty">{t('diagrams.inspector.semElementos')}</p>
  return (
    <Section title={t('diagrams.inspector.tabs.layers')}>
      <p className="dg-muted">{t('diagrams.inspector.camadasAjuda')}</p>
      <ul className="dg-list">
        {ordered.map((n) => (
          <li key={n.id} className={cx('dg-layer', NODE_NOTATION[n.type] !== notation && 'is-other')}>
            <button type="button" className="dg-row" aria-pressed={selection?.kind === 'node' && selection.id === n.id} onClick={() => onSelect({ kind: 'node', id: n.id })}>
              <span className="dg-row__k">{t(`diagrams.notacoes.${NODE_NOTATION[n.type]}`)}</span>
              <span className="dg-row__t">
                {label(t, n)} · {t(`diagrams.tipos.${n.type}`)}
              </span>
            </button>
            <IconButton icon="upload" bare label={t('diagrams.inspector.subir', { nome: label(t, n) })} onClick={() => move(n.id, 1)} />
            <IconButton icon="download" bare label={t('diagrams.inspector.descer', { nome: label(t, n) })} onClick={() => move(n.id, -1)} />
          </li>
        ))}
      </ul>
    </Section>
  )
}

function LanesPanel({ doc, selected, onChange, onSelect }: { doc: DiagramDoc; selected: DNode | undefined; onChange: Change; onSelect: (s: Sel | null) => void }) {
  const { t } = useTranslation()
  const pools = doc.nodes.filter((n) => n.type === 'pool')
  const fromSel = selected ? (selected.type === 'pool' ? selected : laneOf(doc, selected)?.pool) : undefined
  const [chosen, setChosen] = useState<string>('')
  const pool = pools.find((p) => p.id === chosen) ?? fromSel ?? pools[0]
  if (!pool) return <p className="dg-empty">{t('diagrams.inspector.semPiscinas')}</p>
  const lanes = pool.props.lanes ?? []
  const setLanes = (next: Lane[], key?: string) =>
    onChange(
      patchNode(doc, pool.id, (p) => ({ ...p, h: next.length ? p.h : Math.max(p.h, poolHeight(p)), props: { ...p.props, lanes: next } })),
      key && `${pool.id}:${key}`,
    )
  const swap = (i: number, j: number) => {
    if (j < 0 || j >= lanes.length) return
    const next = [...lanes]
    ;[next[i], next[j]] = [next[j], next[i]]
    setLanes(next)
  }
  return (
    <Section title={t('diagrams.inspector.tabs.lanes')}>
      {pools.length > 1 && (
        <Field label={t('diagrams.inspector.piscina')}>
          <Select value={pool.id} onChange={(e) => setChosen(e.target.value)}>
            {pools.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name || t('diagrams.semNome')}
              </option>
            ))}
          </Select>
        </Field>
      )}
      <Field label={t('diagrams.inspector.nomePiscina')}>
        <TextInput value={pool.name} onChange={(e) => onChange(patchNode(doc, pool.id, (p) => ({ ...p, name: e.target.value })), `${pool.id}:name`)} />
      </Field>
      <ul className="dg-list">
        {lanes.map((l, i) => (
          <li key={l.id} className="dg-lane">
            <TextInput aria-label={t('diagrams.inspector.nomePista', { n: i + 1 })} value={l.name} onChange={(e) => setLanes(lanes.map((x) => (x.id === l.id ? { ...x, name: e.target.value } : x)), `${l.id}:name`)} />
            <TextInput
              type="number"
              min={60}
              step={20}
              className="dg-lane__size"
              aria-label={t('diagrams.inspector.alturaPista', { nome: l.name || i + 1 })}
              value={l.size}
              onChange={(e) => setLanes(lanes.map((x) => (x.id === l.id ? { ...x, size: Math.max(60, Number(e.target.value) || 60) } : x)), `${l.id}:size`)}
            />
            <IconButton icon="upload" bare label={t('diagrams.inspector.pistaSubir', { nome: l.name || i + 1 })} disabled={i === 0} onClick={() => swap(i, i - 1)} />
            <IconButton icon="x" bare label={t('diagrams.inspector.pistaRemover', { nome: l.name || i + 1 })} onClick={() => setLanes(lanes.filter((x) => x.id !== l.id))} />
          </li>
        ))}
      </ul>
      <button
        type="button"
        className="dg-add"
        onClick={() => {
          const size = lanes.length ? 140 : Math.max(140, pool.h)
          setLanes([...lanes, { id: uid('l'), name: t('diagrams.inspector.pistaN', { n: lanes.length + 1 }), size }])
          onSelect({ kind: 'node', id: pool.id })
        }}
      >
        {t('diagrams.inspector.novaPista')}
      </button>
    </Section>
  )
}

function ValidationPanel({ doc, issues, typeLabel, onSelect, onFix, onFixAll }: { doc: DiagramDoc; issues: Issue[]; typeLabel: (n: DNode) => string; onSelect: (s: Sel | null) => void; onFix: (i: Issue) => void; onFixAll: () => void }) {
  const { t } = useTranslation()
  const fixable = issues.filter((i) => i.fixable).length
  const selectFor = (i: Issue) => {
    const id = i.elements.find((x) => doc.nodes.some((n) => n.id === x) || doc.edges.some((e) => e.id === x))
    if (!id) return
    onSelect({ kind: doc.nodes.some((n) => n.id === id) ? 'node' : 'edge', id })
  }
  if (issues.length === 0) {
    return (
      <Section title={t('diagrams.validacao.titulo')}>
        <p className="dg-ok">
          <Icon name="check" size={14} />
          {t('diagrams.validacao.semProblemas')}
        </p>
      </Section>
    )
  }
  return (
    <Section title={t('diagrams.validacao.titulo')} tone="warning">
      <ul className="dg-issues">
        {issues.map((i) => (
          <li key={i.id} className={cx('dg-issue', i.severity === 'error' && 'is-error')}>
            <Icon name="alert" size={13} />
            <span className="dg-sr">{t(`diagrams.validacao.${i.severity === 'error' ? 'erro' : 'aviso'}`)}</span>
            <span className="dg-issue__text">{t(`diagrams.regras.${i.code}`, issueParams(doc, i, typeLabel))}</span>
            <span className="dg-issue__actions">
              <button type="button" className="dg-link" onClick={() => selectFor(i)}>
                {t('diagrams.validacao.ver')}
              </button>
              {i.fixable && (
                <button type="button" className="dg-link" onClick={() => onFix(i)}>
                  {t('diagrams.validacao.corrigir')}
                </button>
              )}
            </span>
          </li>
        ))}
      </ul>
      {fixable > 0 ? (
        <Button variant="primary" size="sm" icon="wand" block onClick={onFixAll}>
          {t('diagrams.validacao.corrigirTudo')}
        </Button>
      ) : (
        <p className="dg-muted">{t('diagrams.validacao.nadaACorrigir')}</p>
      )}
    </Section>
  )
}

