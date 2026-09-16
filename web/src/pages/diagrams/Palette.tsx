/**
 * Coluna esquerda: procurar elemento e as paletas da notação activa.
 *
 * Cada item é um botão: carregar põe o elemento no centro da vista (é o
 * caminho do teclado e do toque); arrastar para o quadro põe-no onde cair.
 * Os itens de ligação escolhem a ferramenta de aresta.
 */
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../../ui/icons'
import { cx, TextInput } from '../../ui/kit'
import type { Tool } from './Canvas'
import { PALETTE_MIME } from './Canvas'
import { DiagramDoc, DNode, Notation, PALETTES, PaletteItem } from './model'
import { PENS } from './paint'

/** Glifos da paleta, desenhados como no template (viewBox 22×18). */
const GLYPH: Record<string, string> = {
  class: 'M2 2h18v14H2zM2 7h18M2 11h18',
  interface: 'M2 3h18v12H2zM2 8h18',
  enum: 'M3 2h16v14H3zM3 6h16M7 10h8',
  package: 'M2 5h18v11H2zM2 5V2h8v3',
  note: 'M2 2h14l4 4v10H2z',
  generalization: 'M11 16V7M11 2 6 8h10z',
  association: 'M2 9h18',
  composition: 'M2 9l3-3 3 3-3 3zM8 9h12',
  lifeline: 'M4 2h14v5H4zM11 7v9',
  message: 'M2 9h16M14 5l4 4-4 4',
  reply: 'M20 9H4M8 5 4 9l4 4',
  fragment: 'M2 2h18v14H2zM2 6h7l2 2',
  actor: 'M11 5a2.4 2.4 0 1 0 0-4 2.4 2.4 0 0 0 0 4ZM11 5v7M6 8h10M11 12l-3 5M11 12l3 5',
  usecase: 'M11 3c5 0 9 2.7 9 6s-4 6-9 6-9-2.7-9-6 4-6 9-6Z',
  include: 'M2 9h16M14 6l4 3-4 3',
  boundary: 'M2 2h18v14H2z',
  startEvent: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14Z',
  messageEvent: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM7.6 7h6.8v4H7.6zM7.6 7 11 9.6 14.4 7',
  timerEvent: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 6v3.2l2.2 1.4',
  endEvent: 'M11 1.4a7.6 7.6 0 1 0 0 15.2 7.6 7.6 0 0 0 0-15.2ZM11 4a5 5 0 1 0 0 10 5 5 0 0 0 0-10Z',
  task: 'M2 3h18v12H2z',
  userTask: 'M2 3h18v12H2zM6 7.4a1.4 1.4 0 1 0 0-2.8 1.4 1.4 0 0 0 0 2.8',
  serviceTask: 'M2 3h18v12H2zM6 9a1.6 1.6 0 1 0 0-3.2A1.6 1.6 0 0 0 6 9',
  subProcess: 'M2 3h18v12H2zM8 12h6M11 9v6',
  exclusiveGateway: 'M11 2 20 9l-9 7-9-7zM8 6.4l6 5.2M14 6.4l-6 5.2',
  parallelGateway: 'M11 2 20 9l-9 7-9-7zM11 5.4v7.2M7.4 9h7.2',
  inclusiveGateway: 'M11 2 20 9l-9 7-9-7zM11 5.6a3.4 3.4 0 1 0 0 6.8 3.4 3.4 0 0 0 0-6.8',
  eventGateway: 'M11 2 20 9l-9 7-9-7zM11 5.4 13 9l-2 3.6L9 9z',
  pool: 'M2 3h18v12H2zM5 3v12',
  lane: 'M2 3h18v6H2zM2 9h18v6H2z',
  dataObject: 'M4 2h9l5 4v10H4z',
  annotation: 'M6 2v14M6 2h12M6 16h12',
  service: 'M2 3h18v12H2zM6 7h10M6 11h6',
  database: 'M11 2c4 0 8 1 8 2.5v9c0 1.5-4 2.5-8 2.5s-8-1-8-2.5v-9C3 3 7 2 11 2ZM3 4.5c0 1.5 4 2.5 8 2.5s8-1 8-2.5',
  queue: 'M2 4h18v10H2zM13 4v10M16 4v10',
  client: 'M3 3h16v10H3zM8 16h6M11 13v3',
  external: 'M2 3h18v12H2z',
  zone: 'M2 2h18v14H2z',
  sync: 'M2 9h16M14 5l4 4-4 4',
  async: 'M2 9h3M8 9h3M14 9h4M14 5l4 4-4 4',
  dataFlow: 'M2 9h1M5 9h1M8 9h1M11 9h1M14 9h4M14 5l4 4-4 4',
  terminator: 'M6 4h10a5 5 0 0 1 0 10H6A5 5 0 0 1 6 4Z',
  process: 'M2 4h18v10H2z',
  decision: 'M11 2 20 9l-9 7-9-7z',
  io: 'M6 4h14l-4 10H2z',
  document: 'M3 2h16v11c-4-2-8 3-16 1z',
  flow: 'M2 9h16M14 5l4 4-4 4',
  pen: 'M4 16l3-1 9-9-2-2-9 9zM12 6l2 2',
  eraser: 'M8 16h10M4 12l7-7 5 5-5 5H7z',
  text: 'M4 4h14M11 4v12M8 16h6',
}

const DASHED = new Set(['boundary', 'external', 'zone'])

export default function Palette({
  notation,
  doc,
  tool,
  penColor,
  onPenColor,
  onPick,
  onFind,
}: {
  notation: Notation
  doc: DiagramDoc
  tool: Tool
  penColor: string
  onPenColor: (c: string) => void
  onPick: (item: PaletteItem) => void
  onFind: (n: DNode) => void
}) {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const results = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return []
    return doc.nodes
      .filter((n) =>
        [n.name, n.props.text ?? '', n.props.stereotype ?? '', ...(n.props.attributes ?? []), ...(n.props.operations ?? [])].some((s) => s.toLowerCase().includes(q)),
      )
      .slice(0, 12)
  }, [doc.nodes, query])

  const isActive = (it: PaletteItem) =>
    (it.kind === 'edge' && tool.kind === 'edge' && tool.edge === it.edge) ||
    (it.kind === 'pen' && tool.kind === 'pen') ||
    (it.kind === 'eraser' && tool.kind === 'eraser')

  return (
    <div className="dg-palette">
      <div className="dg-find">
        <label className="dg-find__field">
          <Icon name="search" size={13} />
          <TextInput
            type="search"
            value={query}
            autoComplete="off"
            placeholder={t('diagrams.paleta.procurar')}
            aria-label={t('diagrams.paleta.procurarRotulo')}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && results[0]) onFind(results[0])
              if (e.key === 'Escape') setQuery('')
            }}
          />
        </label>
        {query.trim() && (
          <ul className="dg-find__results" aria-live="polite">
            {results.length === 0 ? (
              <li className="dg-find__none">{t('diagrams.paleta.semResultados')}</li>
            ) : (
              results.map((n) => (
                <li key={n.id}>
                  <button type="button" onClick={() => onFind(n)}>
                    <span className="dg-find__type">{t(`diagrams.tipos.${n.type}`)}</span>
                    <span className="dg-find__name">{n.name || n.props.text || t('diagrams.semNome')}</span>
                  </button>
                </li>
              ))
            )}
          </ul>
        )}
      </div>

      {PALETTES[notation].map((g) => (
        <section key={g.key} className="dg-group" aria-label={t(`diagrams.paleta.grupos.${g.key}`)}>
          <h3 className="dg-group__title">{t(`diagrams.paleta.grupos.${g.key}`)}</h3>
          <div className="dg-group__grid">
            {g.items.map((it) => (
              <button
                key={it.key}
                type="button"
                className={cx('dg-item', isActive(it) && 'is-active')}
                aria-pressed={it.kind === 'edge' || it.kind === 'pen' || it.kind === 'eraser' ? isActive(it) : undefined}
                draggable={it.kind === 'node'}
                onDragStart={(e) => {
                  e.dataTransfer.setData(PALETTE_MIME, `${notation}:${g.key}:${it.key}`)
                  e.dataTransfer.effectAllowed = 'copy'
                }}
                onClick={() => onPick(it)}
              >
                <svg viewBox="0 0 22 18" width={17} height={15} fill="none" stroke="currentColor" strokeWidth={1.3} strokeLinecap="round" strokeLinejoin="round" strokeDasharray={DASHED.has(it.key) ? '3 2' : undefined} aria-hidden="true">
                  <path d={GLYPH[it.key] ?? GLYPH.process} />
                </svg>
                <span>{t(`diagrams.paleta.itens.${it.key}`)}</span>
              </button>
            ))}
          </div>
        </section>
      ))}

      {tool.kind === 'edge' && <p className="dg-help">{t('diagrams.paleta.ajudaAresta')}</p>}
      {tool.kind === 'select' && notation !== 'free' && <p className="dg-help">{t('diagrams.paleta.ajuda')}</p>}

      {notation === 'free' && (
        <div className="dg-pens" role="group" aria-label={t('diagrams.paleta.cores')}>
          {Object.entries(PENS).map(([key, color]) => (
            <button
              key={key}
              type="button"
              className="dg-pen"
              aria-pressed={penColor === color}
              aria-label={t(`diagrams.paleta.cor.${key}`)}
              title={t(`diagrams.paleta.cor.${key}`)}
              style={{ color }}
              onClick={() => onPenColor(color)}
            >
              <span aria-hidden="true" />
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

/** Encontra o item de paleta pela chave usada no arrasto (`notação:grupo:item`). */
export function paletteItemByKey(key: string): PaletteItem | null {
  const [notation, group, item] = key.split(':')
  const g = PALETTES[notation as Notation]?.find((x) => x.key === group)
  return g?.items.find((x) => x.key === item) ?? null
}
