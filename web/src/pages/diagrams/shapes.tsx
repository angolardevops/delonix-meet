/**
 * Desenho SVG de cada elemento e de cada aresta.
 *
 * Tudo com atributos de apresentação (fill, stroke, font-*) e as cores do
 * papel (`paint.ts`), para que o SVG serializado seja um documento autónomo.
 * O que é interface (zona de toque das arestas, contorno de selecção,
 * puxadores) leva `data-ui` e sai da exportação.
 */
import { memo, ReactNode } from 'react'
import { CLASS, classifierHeight, LANE_HEADER, objectHeight, poolHeight, Pt, POOL_HEADER } from './geometry'
import { DEdge, DNode, parseMember } from './model'
import { FILLS, INK, MONO } from './paint'

const SW = 1.5

/** Largura aproximada de um carácter — chega para partir linhas. */
export function wrapText(text: string, maxWidth: number, fontSize: number, maxLines = 4): string[] {
  const perLine = Math.max(4, Math.floor(maxWidth / (fontSize * 0.55)))
  const out: string[] = []
  for (const para of text.split('\n')) {
    let line = ''
    for (const word of para.split(/\s+/)) {
      if (!word) continue
      const next = line ? `${line} ${word}` : word
      if (next.length > perLine && line) {
        out.push(line)
        line = word
      } else line = next
    }
    out.push(line)
  }
  if (out.length > maxLines) {
    const cut = out.slice(0, maxLines)
    cut[maxLines - 1] = cut[maxLines - 1].replace(/.{0,1}$/, '…')
    return cut
  }
  return out
}

function Lines({ lines, x, y, size, anchor = 'middle', weight, color = INK.ink, font, lh = 1.3 }: {
  lines: string[]
  x: number
  y: number
  size: number
  anchor?: 'start' | 'middle' | 'end'
  weight?: number
  color?: string
  font?: string
  lh?: number
}) {
  const top = y - ((lines.length - 1) * size * lh) / 2
  return (
    <text x={x} y={top} fontSize={size} fontWeight={weight} fill={color} textAnchor={anchor} dominantBaseline="middle" fontFamily={font}>
      {lines.map((l, i) => (
        <tspan key={i} x={x} y={top + i * size * lh}>
          {l}
        </tspan>
      ))}
    </text>
  )
}

const fillOf = (n: DNode, base: string) => (n.fill && FILLS[n.fill]) || base
const strokeOf = (n: DNode) => (n.props.emphasis ? INK.accent : INK.ink)

function Classifier({ n }: { n: DNode }) {
  const h = classifierHeight(n)
  const st = n.props.stereotype
  const headH = CLASS.pad * 2 + CLASS.name + (st ? CLASS.stereo : 0)
  const attrs = n.props.attributes ?? []
  const ops = n.props.operations ?? []
  const attrTop = headH
  const attrH = CLASS.pad * 2 + Math.max(1, attrs.length) * CLASS.line
  const stroke = strokeOf(n)
  const headFill = n.props.emphasis ? INK.accentTint : INK.header
  const member = (raw: string) => {
    const m = parseMember(raw)
    const vis = m.visibility === '-' ? '−' : m.visibility
    if (n.type === 'enum') return m.name
    return `${vis}${vis ? ' ' : ''}${m.name}${m.isOperation ? `(${m.params})` : ''}${m.type ? `: ${m.type}` : ''}`
  }
  return (
    <>
      <rect width={n.w} height={h} rx={2} fill={fillOf(n, INK.surface)} stroke={stroke} strokeWidth={SW} />
      <rect x={0.75} y={0.75} width={n.w - 1.5} height={headH - 0.75} fill={headFill} />
      <line x1={0} y1={headH} x2={n.w} y2={headH} stroke={stroke} strokeWidth={SW} />
      {st && (
        <text x={9} y={CLASS.pad + 9} fontSize={8.5} fill={INK.muted} fontFamily={MONO}>
          {`«${st}»`}
        </text>
      )}
      <text x={9} y={CLASS.pad + (st ? CLASS.stereo : 0) + 14} fontSize={12} fontWeight={700} fill={n.props.emphasis ? INK.accent : INK.ink} fontStyle={n.type === 'interface' ? 'italic' : undefined}>
        {n.name}
      </text>
      {attrs.map((a, i) => (
        <text key={`a${i}`} x={9} y={attrTop + CLASS.pad + 10 + i * CLASS.line} fontSize={9.5} fill={INK.ink} fontFamily={MONO}>
          {member(a)}
        </text>
      ))}
      {!(n.type === 'enum' && ops.length === 0) && (
        <>
          <line x1={0} y1={attrTop + attrH} x2={n.w} y2={attrTop + attrH} stroke={INK.grid} strokeWidth={1} />
          {ops.map((o, i) => (
            <text key={`o${i}`} x={9} y={attrTop + attrH + CLASS.pad + 10 + i * CLASS.line} fontSize={9.5} fill={INK.ink} fontFamily={MONO}>
              {member(o)}
            </text>
          ))}
        </>
      )}
    </>
  )
}

function EventMarker({ trigger, r, color, filled = false }: { trigger: string | undefined; r: number; color: string; filled?: boolean }) {
  // Lançar desenha o marcador cheio; apanhar desenha-o a traço (BPMN 2.0, 10.4.2).
  const fill = filled ? color : 'none'
  const inner = filled ? INK.surface : color
  if (trigger === 'message') {
    const w = r * 0.9
    const h = r * 0.62
    return (
      <g stroke={filled ? INK.surface : color} strokeWidth={1.3} fill={fill}>
        <rect x={r - w / 2} y={r - h / 2} width={w} height={h} stroke={color} />
        <path d={`M${r - w / 2} ${r - h / 2}L${r} ${r + h * 0.1}L${r + w / 2} ${r - h / 2}`} stroke={inner} fill="none" />
      </g>
    )
  }
  if (trigger === 'error') {
    return <path d={`M${r - r * 0.45} ${r + r * 0.5}L${r - r * 0.2} ${r - r * 0.45}L${r + r * 0.08} ${r + r * 0.12}L${r + r * 0.45} ${r - r * 0.5}L${r + r * 0.22} ${r + r * 0.45}L${r - r * 0.06} ${r - r * 0.1}Z`} stroke={color} strokeWidth={1.2} fill={fill} strokeLinejoin="round" />
  }
  if (trigger === 'escalation') {
    return <path d={`M${r} ${r - r * 0.5}L${r + r * 0.38} ${r + r * 0.45}L${r} ${r + r * 0.12}L${r - r * 0.38} ${r + r * 0.45}Z`} stroke={color} strokeWidth={1.2} fill={fill} strokeLinejoin="round" />
  }
  if (trigger === 'compensation') {
    const k = r * 0.3
    return <path d={`M${r} ${r - k}V${r + k}L${r - k * 1.3} ${r}ZM${r + k * 1.3} ${r - k}V${r + k}L${r} ${r}Z`} stroke={color} strokeWidth={1.2} fill={fill} strokeLinejoin="round" />
  }
  if (trigger === 'conditional') {
    const w = r * 0.62
    const h = r * 0.8
    return (
      <g stroke={color} strokeWidth={1.1} fill="none">
        <rect x={r - w / 2} y={r - h / 2} width={w} height={h} />
        <path d={`M${r - w / 2 + 2} ${r - h / 4}h${w - 4}M${r - w / 2 + 2} ${r}h${w - 4}M${r - w / 2 + 2} ${r + h / 4}h${w - 4}`} />
      </g>
    )
  }
  if (trigger === 'link') {
    const k = r * 0.42
    return <path d={`M${r - k} ${r - k * 0.4}H${r + k * 0.1}V${r - k * 0.9}L${r + k} ${r}L${r + k * 0.1} ${r + k * 0.9}V${r + k * 0.4}H${r - k}Z`} stroke={color} strokeWidth={1.2} fill={fill} strokeLinejoin="round" />
  }
  if (trigger === 'terminate') return <circle cx={r} cy={r} r={r * 0.55} fill={color} />
  if (trigger === 'timer') {
    return (
      <g stroke={color} strokeWidth={1.3} fill="none">
        <circle cx={r} cy={r} r={r * 0.55} />
        <path d={`M${r} ${r - r * 0.4}V${r}L${r + r * 0.3} ${r + r * 0.18}`} />
      </g>
    )
  }
  if (trigger === 'signal') {
    return <path d={`M${r} ${r - r * 0.5}L${r + r * 0.48} ${r + r * 0.35}H${r - r * 0.48}Z`} stroke={color} strokeWidth={1.3} fill={fill} />
  }
  return null
}

function TaskMarker({ kind }: { kind: string | undefined }) {
  const c = INK.muted
  switch (kind) {
    case 'user':
      return <path d="M11 9a2.4 2.4 0 1 0 0-4.8A2.4 2.4 0 0 0 11 9ZM6.5 15a4.5 4.5 0 0 1 9 0Z" stroke={c} strokeWidth={1.1} fill="none" />
    case 'service':
      return (
        <g stroke={c} strokeWidth={1.1} fill="none">
          <circle cx={11} cy={10} r={2.2} />
          <path d="M11 5v1.8M11 13.2V15M6 10h1.8M14.2 10H16M7.5 6.5l1.3 1.3M13.2 11.2l1.3 1.3M7.5 13.5l1.3-1.3M13.2 8.8l1.3-1.3" />
        </g>
      )
    case 'script':
      return <path d="M7 5h7c-1.5 1.5-1.5 3.5 0 5s1.5 3.5 0 5H7c1.5-1.5 1.5-3.5 0-5S5.5 6.5 7 5ZM8.5 8h4M8.5 12h4" stroke={c} strokeWidth={1.1} fill="none" />
    case 'manual':
      return <path d="M6 13V9.5l2.5-3h2.5l-1 2h5v1.5h-2.5v1.2h2v1.3h-2V13Z" stroke={c} strokeWidth={1.1} fill="none" />
    case 'send':
      return <path d="M6 6.5h10v7H6ZM6 6.5l5 3.5 5-3.5" stroke={INK.ink} strokeWidth={1.1} fill={INK.ink} />
    case 'businessRule':
      return <path d="M5 6h12v9H5ZM5 9h12M9 9v6" stroke={c} strokeWidth={1.1} fill="none" />
    case 'receive':
      return <path d="M6 6.5h10v7H6ZM6 6.5l5 3.5 5-3.5" stroke={c} strokeWidth={1.1} fill="none" />
    default:
      return null
  }
}

/**
 * Marcadores na base de uma actividade BPMN, lado a lado e centrados:
 * ciclo, multi-instância, compensação, ad-hoc e o «+» do subprocesso.
 */
function ActivityMarkers({ n }: { n: DNode }) {
  const marks: string[] = []
  if (n.props.loop && (n.props.multiInstance ?? 'none') === 'none') marks.push('loop')
  if (n.type === 'task' && n.props.multiInstance === 'parallel') marks.push('par')
  if (n.type === 'task' && n.props.multiInstance === 'sequential') marks.push('seq')
  if (n.props.compensation) marks.push('comp')
  if (n.type === 'subProcess' && n.props.adHoc) marks.push('adhoc')
  if (n.type === 'subProcess') marks.push('plus')
  const step = 15
  const x0 = n.w / 2 - ((marks.length - 1) * step) / 2
  return (
    <g stroke={INK.ink} strokeWidth={1.2} fill="none">
      {marks.map((m, i) => {
        const cx = x0 + i * step
        const y = n.h - 8
        switch (m) {
          case 'loop':
            return <path key={m} d={`M${cx - 3} ${y + 4}A5 5 0 1 1 ${cx + 3.5} ${y + 3.5}M${cx - 3} ${y + 4}l-2.4 -0.2M${cx - 3} ${y + 4}l0.3 -2.4`} />
          case 'par':
            return <path key={m} d={`M${cx - 4} ${y - 5}v10M${cx} ${y - 5}v10M${cx + 4} ${y - 5}v10`} strokeWidth={1.4} />
          case 'seq':
            return <path key={m} d={`M${cx - 5} ${y - 4}h10M${cx - 5} ${y}h10M${cx - 5} ${y + 4}h10`} strokeWidth={1.4} />
          case 'comp':
            return <path key={m} d={`M${cx} ${y - 4}v8l-5-4ZM${cx + 5} ${y - 4}v8l-5-4Z`} />
          case 'adhoc':
            return <path key={m} d={`M${cx - 5} ${y + 1}c1.6-3 3.4-3 5 0s3.4 3 5 0`} strokeWidth={1.4} />
          default:
            return (
              <g key={m}>
                <rect x={cx - 6} y={y - 6} width={12} height={12} />
                <path d={`M${cx} ${y - 3.5}v7M${cx - 3.5} ${y}h7`} />
              </g>
            )
        }
      })}
    </g>
  )
}

/** Nome por baixo de uma forma pequena (pseudo-estados, portos, interfaces). */
function Below({ n }: { n: DNode }) {
  if (!n.name) return null
  return <Lines lines={wrapText(n.name, 110, 8.5, 2)} x={n.w / 2} y={n.h + 11} size={8.5} color={INK.muted} />
}

function Stereo({ n, y, x, anchor = 'middle' }: { n: DNode; y: number; x?: number; anchor?: 'start' | 'middle' }) {
  if (!n.props.stereotype) return null
  return (
    <text x={x ?? n.w / 2} y={y} fontSize={8.5} fill={INK.muted} textAnchor={anchor} fontFamily={MONO}>
      {`«${n.props.stereotype}»`}
    </text>
  )
}

function NodeBody({ n, sub }: { n: DNode; sub?: string }): ReactNode {
  const stroke = strokeOf(n)
  const surface = fillOf(n, INK.surface)
  switch (n.type) {
    case 'class':
    case 'interface':
    case 'enum':
      return <Classifier n={n} />
    case 'package':
      return (
        <>
          <path d={`M0 0H${Math.min(n.w * 0.45, 160)}V18H0Z`} fill={fillOf(n, INK.header)} stroke={stroke} strokeWidth={SW} />
          <rect y={18} width={n.w} height={n.h - 18} fill={fillOf(n, INK.surface)} fillOpacity={0.55} stroke={stroke} strokeWidth={SW} />
          <text x={8} y={13} fontSize={10.5} fontWeight={700} fill={INK.ink}>
            {n.name}
          </text>
        </>
      )
    case 'note':
      return (
        <>
          <path d={`M0 0H${n.w - 12}L${n.w} 12V${n.h}H0Z`} fill={fillOf(n, INK.noteFill)} stroke={INK.muted} strokeWidth={SW} strokeDasharray="5 3" />
          <path d={`M${n.w - 12} 0V12H${n.w}`} fill="none" stroke={INK.muted} strokeWidth={1} />
          <text x={10} y={14} fontSize={8.5} fill={INK.muted} fontFamily={MONO}>
            {'«note»'}
          </text>
          <Lines lines={wrapText(n.props.text ?? n.name, n.w - 20, 10, 4)} x={10} y={24 + ((Math.min(4, wrapText(n.props.text ?? n.name, n.w - 20, 10, 4).length) - 1) * 13) / 2 + 6} size={10} anchor="start" color={INK.noteInk} />
        </>
      )
    case 'lifeline': {
      const len = n.props.length ?? 240
      return (
        <>
          <line x1={n.w / 2} y1={n.h} x2={n.w / 2} y2={n.h + len} stroke={INK.muted} strokeWidth={1.5} strokeDasharray="6 6" />
          <rect width={n.w} height={n.h} rx={2} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Lines lines={wrapText(n.name, n.w - 10, 10, 2)} x={n.w / 2} y={n.h / 2} size={10} weight={700} />
        </>
      )
    }
    case 'fragment': {
      const op = n.props.operator ?? 'alt'
      const tagW = Math.max(36, op.length * 7 + 18)
      return (
        <>
          <rect width={n.w} height={n.h} fill="none" stroke={stroke} strokeWidth={SW} />
          <path d={`M0 0H${tagW}V12L${tagW - 8} 20H0Z`} fill={INK.header} stroke={stroke} strokeWidth={1.2} />
          <text x={6} y={14} fontSize={10} fontWeight={700} fill={INK.ink} fontFamily={MONO}>
            {op}
          </text>
          {n.name && (
            <text x={tagW + 8} y={14} fontSize={10} fill={INK.ink} fontFamily={MONO}>
              {`[${n.name}]`}
            </text>
          )}
        </>
      )
    }
    case 'actor': {
      const cx = n.w / 2
      return (
        <>
          <rect width={n.w} height={n.h} fill="transparent" />
          <g stroke={stroke} strokeWidth={SW} fill="none" strokeLinecap="round">
            <circle cx={cx} cy={10} r={8} fill={INK.surface} />
            <path d={`M${cx} 18V46M${cx - 16} 28H${cx + 16}M${cx} 46L${cx - 13} 66M${cx} 46L${cx + 13} 66`} />
          </g>
          <Lines lines={wrapText(n.name, 110, 10, 2)} x={cx} y={78} size={10} />
        </>
      )
    }
    case 'usecase':
      return (
        <>
          <ellipse cx={n.w / 2} cy={n.h / 2} rx={n.w / 2} ry={n.h / 2} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Lines lines={wrapText(n.name, n.w - 30, 10, 2)} x={n.w / 2} y={n.h / 2} size={10} />
        </>
      )
    case 'boundary':
      return (
        <>
          <rect width={n.w} height={n.h} fill={fillOf(n, INK.surface)} fillOpacity={0.5} stroke={stroke} strokeWidth={SW} />
          <text x={n.w / 2} y={18} fontSize={11} fontWeight={700} fill={INK.ink} textAnchor="middle">
            {n.name}
          </text>
        </>
      )
    case 'activation':
      return <rect width={n.w} height={n.h} fill={fillOf(n, INK.surface)} stroke={stroke} strokeWidth={1.3} />
    case 'initialNode':
    case 'stateInitial':
      return (
        <>
          <circle cx={n.w / 2} cy={n.h / 2} r={n.w / 2} fill={n.props.emphasis ? INK.accent : INK.ink} />
          <Below n={n} />
        </>
      )
    case 'activityFinal':
    case 'stateFinal':
      return (
        <>
          <circle cx={n.w / 2} cy={n.h / 2} r={n.w / 2 - 0.75} fill={INK.surface} stroke={stroke} strokeWidth={SW} />
          <circle cx={n.w / 2} cy={n.h / 2} r={n.w / 2 - 5.5} fill={stroke} />
          <Below n={n} />
        </>
      )
    case 'flowFinal': {
      const r = n.w / 2
      const k = r * 0.7
      return (
        <>
          <circle cx={r} cy={r} r={r - 0.75} fill={INK.surface} stroke={stroke} strokeWidth={SW} />
          <path d={`M${r - k} ${r - k}L${r + k} ${r + k}M${r + k} ${r - k}L${r - k} ${r + k}`} stroke={stroke} strokeWidth={SW} />
          <Below n={n} />
        </>
      )
    }
    case 'action':
      return (
        <>
          <rect width={n.w} height={n.h} rx={12} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Lines lines={wrapText(n.name, n.w - 20, 10.5, 3)} x={n.w / 2} y={n.h / 2} size={10.5} />
        </>
      )
    case 'decisionNode':
    case 'choice':
      return (
        <>
          <path d={`M${n.w / 2} 0L${n.w} ${n.h / 2}L${n.w / 2} ${n.h}L0 ${n.h / 2}Z`} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Below n={n} />
        </>
      )
    case 'forkNode':
      return (
        <>
          <rect width={n.w} height={n.h} rx={1.5} fill={n.props.emphasis ? INK.accent : INK.ink} />
          {n.name && (
            <text x={n.w >= n.h ? n.w + 6 : n.w / 2} y={n.w >= n.h ? n.h / 2 : n.h + 12} fontSize={8.5} fill={INK.muted} dominantBaseline="middle" textAnchor={n.w >= n.h ? 'start' : 'middle'}>
              {n.name}
            </text>
          )}
        </>
      )
    case 'partition':
      return (
        <>
          <rect width={n.w} height={n.h} fill={fillOf(n, INK.surface)} fillOpacity={0.5} stroke={stroke} strokeWidth={SW} />
          <rect x={0.75} y={0.75} width={n.w - 1.5} height={24} fill={INK.header} />
          <line x1={0} y1={25} x2={n.w} y2={25} stroke={stroke} strokeWidth={SW} />
          <text x={n.w / 2} y={16} fontSize={10.5} fontWeight={700} fill={INK.ink} textAnchor="middle">
            {n.name}
          </text>
        </>
      )
    case 'objectNode':
      return (
        <>
          <rect width={n.w} height={n.h} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Lines lines={wrapText(n.name, n.w - 16, 10.5, 2)} x={n.w / 2} y={n.h / 2} size={10.5} />
        </>
      )
    case 'state': {
      const acts = n.props.attributes ?? []
      return (
        <>
          <rect width={n.w} height={n.h} rx={12} fill={surface} stroke={stroke} strokeWidth={SW} />
          <text x={n.w / 2} y={acts.length ? 17 : n.h / 2} fontSize={11} fontWeight={700} fill={INK.ink} textAnchor="middle" dominantBaseline="middle">
            {n.name}
          </text>
          {acts.length > 0 && (
            <>
              <line x1={0} y1={28} x2={n.w} y2={28} stroke={stroke} strokeWidth={1} />
              {acts.slice(0, Math.max(1, Math.floor((n.h - 34) / 13))).map((a, i) => (
                <text key={i} x={9} y={42 + i * 13} fontSize={9} fill={INK.ink} fontFamily={MONO}>
                  {a}
                </text>
              ))}
            </>
          )}
        </>
      )
    }
    case 'compositeState':
      return (
        <>
          <rect width={n.w} height={n.h} rx={14} fill={fillOf(n, INK.surface)} fillOpacity={0.55} stroke={stroke} strokeWidth={SW} />
          <text x={12} y={17} fontSize={11} fontWeight={700} fill={INK.ink}>
            {n.name}
          </text>
          <line x1={0} y1={26} x2={n.w} y2={26} stroke={stroke} strokeWidth={1} />
        </>
      )
    case 'history':
      return (
        <>
          <circle cx={n.w / 2} cy={n.h / 2} r={n.w / 2 - 0.75} fill={surface} stroke={stroke} strokeWidth={SW} />
          <text x={n.w / 2} y={n.h / 2 + 0.5} fontSize={12} fontWeight={700} fill={INK.ink} textAnchor="middle" dominantBaseline="middle">
            {n.props.deep ? 'H*' : 'H'}
          </text>
          <Below n={n} />
        </>
      )
    case 'component':
      return (
        <>
          <rect width={n.w} height={n.h} rx={2} fill={surface} stroke={stroke} strokeWidth={SW} />
          <g transform={`translate(${n.w - 24} 7)`} stroke={stroke} strokeWidth={1.1} fill={INK.surface}>
            <rect x={3} y={0} width={13} height={16} />
            <rect x={0} y={3} width={7} height={3.5} />
            <rect x={0} y={9.5} width={7} height={3.5} />
          </g>
          <Stereo n={n} y={n.h / 2 - 8} />
          <Lines lines={wrapText(n.name, n.w - 40, 11, 2)} x={n.w / 2} y={n.h / 2 + 6} size={11} weight={700} />
        </>
      )
    case 'port':
      return (
        <>
          <rect width={n.w} height={n.h} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Below n={n} />
        </>
      )
    case 'providedInterface':
      return (
        <>
          <circle cx={n.w / 2} cy={n.h / 2} r={n.w / 2 - 0.75} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Below n={n} />
        </>
      )
    case 'requiredInterface':
      return (
        <>
          <rect width={n.w} height={n.h} fill="transparent" />
          <path d={`M${n.w / 2} 0.75A${n.w / 2 - 0.75} ${n.h / 2 - 0.75} 0 0 0 ${n.w / 2} ${n.h - 0.75}`} fill="none" stroke={stroke} strokeWidth={SW} />
          <Below n={n} />
        </>
      )
    case 'deviceNode': {
      const d = 12
      return (
        <>
          <path d={`M0 ${d}L${d} 0H${n.w}V${n.h - d}L${n.w - d} ${n.h}`} fill={INK.header} stroke={stroke} strokeWidth={SW} strokeLinejoin="round" />
          <path d={`M${n.w - d} ${d}L${n.w} 0`} stroke={stroke} strokeWidth={SW} />
          <rect y={d} width={n.w - d} height={n.h - d} fill={fillOf(n, INK.surface)} fillOpacity={0.6} stroke={stroke} strokeWidth={SW} />
          <Stereo n={n} y={d + 14} x={10} anchor="start" />
          <text x={10} y={d + (n.props.stereotype ? 30 : 18)} fontSize={11} fontWeight={700} fill={INK.ink}>
            {n.name}
          </text>
        </>
      )
    }
    case 'artifact':
      return (
        <>
          <rect width={n.w} height={n.h} rx={2} fill={surface} stroke={stroke} strokeWidth={SW} />
          <path d={`M${n.w - 22} 6H${n.w - 12}L${n.w - 8} 10V22H${n.w - 22}ZM${n.w - 12} 6V10H${n.w - 8}`} fill="none" stroke={stroke} strokeWidth={1.1} />
          <Stereo n={n} y={n.h / 2 - 8} />
          <Lines lines={wrapText(n.name, n.w - 40, 10.5, 2)} x={n.w / 2} y={n.h / 2 + 6} size={10.5} weight={700} />
        </>
      )
    case 'object': {
      const h = objectHeight(n)
      const headH = CLASS.pad * 2 + CLASS.name
      const slots = n.props.attributes ?? []
      const title = `${n.name}${n.props.instanceOf?.trim() ? `: ${n.props.instanceOf.trim()}` : ''}`
      const tw = Math.min(n.w - 16, title.length * 6.6)
      return (
        <>
          <rect width={n.w} height={h} rx={2} fill={surface} stroke={stroke} strokeWidth={SW} />
          <text x={n.w / 2} y={CLASS.pad + 14} fontSize={11.5} fontWeight={700} fill={INK.ink} textAnchor="middle">
            {title}
          </text>
          <line x1={n.w / 2 - tw / 2} y1={CLASS.pad + 17} x2={n.w / 2 + tw / 2} y2={CLASS.pad + 17} stroke={INK.ink} strokeWidth={1} />
          {slots.length > 0 && <line x1={0} y1={headH} x2={n.w} y2={headH} stroke={stroke} strokeWidth={SW} />}
          {slots.map((a, i) => (
            <text key={i} x={9} y={headH + CLASS.pad + 10 + i * CLASS.line} fontSize={9.5} fill={INK.ink} fontFamily={MONO}>
              {a}
            </text>
          ))}
        </>
      )
    }
    case 'startEvent':
    case 'intermediateEvent':
    case 'endEvent': {
      const r = n.w / 2
      const color = n.type === 'startEvent' ? INK.start : n.type === 'intermediateEvent' ? INK.amber : INK.ink
      const dash = n.props.boundary && n.props.nonInterrupting ? '4 2.5' : undefined
      return (
        <>
          <circle cx={r} cy={r} r={r} fill={surface} stroke={n.props.emphasis ? INK.accent : color} strokeWidth={n.type === 'endEvent' ? 3 : 1.8} strokeDasharray={dash} />
          {n.type === 'intermediateEvent' && <circle cx={r} cy={r} r={r - 3.5} fill="none" stroke={color} strokeWidth={1.2} strokeDasharray={dash} />}
          <EventMarker trigger={n.props.trigger} r={r} color={color} filled={n.type === 'endEvent' || (n.type === 'intermediateEvent' && !!n.props.throwing)} />
          {n.name && <Lines lines={wrapText(n.name, 90, 8.5, 2)} x={r} y={n.h + 12} size={8.5} color={INK.muted} />}
        </>
      )
    }
    case 'task':
    case 'subProcess': {
      const accentBorder = n.props.emphasis
      return (
        <>
          <rect width={n.w} height={n.h} rx={6} fill={accentBorder ? INK.accentTint : surface} stroke={accentBorder ? INK.accent : INK.ink} strokeWidth={n.type === 'task' && n.props.taskKind === 'call' ? 3.5 : SW} />
          <g transform="translate(2 1)">
            <TaskMarker kind={n.type === 'task' ? n.props.taskKind : undefined} />
          </g>
          <Lines
            lines={wrapText(n.name, n.w - 16, 10, sub ? 2 : 3)}
            x={n.w / 2}
            y={sub ? n.h / 2 - 6 : n.h / 2}
            size={10}
            weight={600}
            color={accentBorder ? INK.accent : INK.ink}
          />
          {sub && (
            <text x={n.w / 2} y={n.h - 16} fontSize={8} fill={accentBorder ? INK.accent : INK.muted} textAnchor="middle" fontFamily={MONO}>
              {sub}
            </text>
          )}
          <ActivityMarkers n={n} />
        </>
      )
    }
    case 'gateway': {
      const w = n.w
      const h = n.h
      const cx = w / 2
      const cy = h / 2
      const k = n.props.gatewayKind ?? 'exclusive'
      const color = INK.amber
      return (
        <>
          <path d={`M${cx} 0L${w} ${cy}L${cx} ${h}L0 ${cy}Z`} fill={surface} stroke={n.props.emphasis ? INK.accent : color} strokeWidth={SW} />
          {k === 'exclusive' && <path d={`M${cx - 7} ${cy - 7}l14 14M${cx + 7} ${cy - 7}l-14 14`} stroke={color} strokeWidth={2.4} />}
          {k === 'parallel' && <path d={`M${cx} ${cy - 10}v20M${cx - 10} ${cy}h20`} stroke={color} strokeWidth={2.4} />}
          {k === 'inclusive' && <circle cx={cx} cy={cy} r={9} fill="none" stroke={color} strokeWidth={2.2} />}
          {k === 'eventBased' && (
            <g stroke={color} strokeWidth={1.1} fill="none">
              <circle cx={cx} cy={cy} r={11} />
              <circle cx={cx} cy={cy} r={8.5} />
              <path d={`M${cx} ${cy - 5.5}l5.2 3.8-2 6.2h-6.4l-2-6.2Z`} />
            </g>
          )}
          {k === 'eventInstantiate' && (
            <g stroke={color} strokeWidth={1.1} fill="none">
              <circle cx={cx} cy={cy} r={11} />
              <path d={`M${cx} ${cy - 5.5}l5.2 3.8-2 6.2h-6.4l-2-6.2Z`} />
            </g>
          )}
          {k === 'eventParallel' && (
            <g stroke={color} strokeWidth={1.1} fill="none">
              <circle cx={cx} cy={cy} r={11} />
              <path d={`M${cx - 1.8} ${cy - 7}h3.6v5.2h5.2v3.6h-5.2v5.2h-3.6v-5.2h-5.2v-3.6h5.2Z`} />
            </g>
          )}
          {k === 'complex' && <path d={`M${cx} ${cy - 10}v20M${cx - 10} ${cy}h20M${cx - 7} ${cy - 7}l14 14M${cx + 7} ${cy - 7}l-14 14`} stroke={color} strokeWidth={2.2} />}
          {n.name && <Lines lines={wrapText(n.name, 100, 8.5, 2)} x={cx} y={h + 12} size={8.5} color={INK.muted} />}
        </>
      )
    }
    case 'pool': {
      const lanes = n.props.lanes ?? []
      const h = poolHeight(n)
      let top = 0
      return (
        <>
          <rect width={n.w} height={h} fill={fillOf(n, INK.surface)} stroke={stroke} strokeWidth={SW} />
          <rect x={0.75} y={0.75} width={POOL_HEADER - 0.75} height={h - 1.5} fill={INK.header} />
          <line x1={POOL_HEADER} y1={0} x2={POOL_HEADER} y2={h} stroke={stroke} strokeWidth={SW} />
          <text transform={`translate(${POOL_HEADER / 2 + 4} ${h / 2}) rotate(-90)`} fontSize={10.5} fontWeight={700} fill={INK.ink} textAnchor="middle">
            {n.name}
          </text>
          {lanes.map((l, i) => {
            const y = top
            top += l.size
            return (
              <g key={l.id}>
                {i > 0 && <line x1={POOL_HEADER} y1={y} x2={n.w} y2={y} stroke={INK.ink} strokeWidth={SW} />}
                <rect x={POOL_HEADER + 0.75} y={y + (i === 0 ? 0.75 : 0.75)} width={LANE_HEADER - 1.5} height={l.size - 1.5} fill={INK.paper} />
                <line x1={POOL_HEADER + LANE_HEADER} y1={y} x2={POOL_HEADER + LANE_HEADER} y2={y + l.size} stroke={INK.ink} strokeWidth={1.2} />
                <text transform={`translate(${POOL_HEADER + LANE_HEADER / 2 + 3.5} ${y + l.size / 2}) rotate(-90)`} fontSize={9.5} fontWeight={600} fill={INK.ink} textAnchor="middle">
                  {l.name}
                </text>
              </g>
            )
          })}
        </>
      )
    }
    case 'dataStore': {
      const ry = 7
      return (
        <>
          <path d={`M0 ${ry}V${n.h - ry}A${n.w / 2} ${ry} 0 0 0 ${n.w} ${n.h - ry}V${ry}`} fill={surface} stroke={stroke} strokeWidth={SW} />
          <ellipse cx={n.w / 2} cy={ry} rx={n.w / 2} ry={ry} fill={surface} stroke={stroke} strokeWidth={SW} />
          <path d={`M0 ${ry + 5}A${n.w / 2} ${ry} 0 0 0 ${n.w} ${ry + 5}M0 ${ry + 10}A${n.w / 2} ${ry} 0 0 0 ${n.w} ${ry + 10}`} fill="none" stroke={stroke} strokeWidth={1} />
          <Below n={n} />
        </>
      )
    }
    case 'group':
      return (
        <>
          <rect width={n.w} height={n.h} rx={10} fill="none" stroke={INK.muted} strokeWidth={1.5} strokeDasharray="10 4 2 4" />
          <text x={10} y={16} fontSize={10} fontWeight={600} fill={INK.muted}>
            {n.name}
          </text>
        </>
      )
    case 'dataObject':
      return (
        <>
          <path d={`M0 0H${n.w - 10}L${n.w} 10V${n.h}H0Z`} fill={surface} stroke={stroke} strokeWidth={SW} />
          <path d={`M${n.w - 10} 0V10H${n.w}`} fill="none" stroke={stroke} strokeWidth={1} />
          {n.props.dataRole === 'input' && <path d="M4 7h5V4l5 5-5 5v-3H4Z" fill="none" stroke={stroke} strokeWidth={1.1} />}
          {n.props.dataRole === 'output' && <path d="M4 7h5V4l5 5-5 5v-3H4Z" fill={stroke} stroke={stroke} strokeWidth={1.1} />}
          {n.props.collection && <path d={`M${n.w / 2 - 4} ${n.h - 12}v8M${n.w / 2} ${n.h - 12}v8M${n.w / 2 + 4} ${n.h - 12}v8`} stroke={stroke} strokeWidth={1.4} />}
          {n.name && <Lines lines={wrapText(n.name, 100, 8.5, 2)} x={n.w / 2} y={n.h + 12} size={8.5} color={INK.muted} />}
        </>
      )
    case 'annotation':
      return (
        <>
          <rect width={n.w} height={n.h} fill="transparent" />
          <path d={`M14 0H0V${n.h}H14`} fill="none" stroke={INK.muted} strokeWidth={SW} />
          <Lines lines={wrapText(n.props.text ?? n.name, n.w - 14, 9.5, 3)} x={8} y={n.h / 2} size={9.5} anchor="start" color={INK.noteInk} />
        </>
      )
    case 'service':
    case 'client':
    case 'external':
      return (
        <>
          <rect
            width={n.w}
            height={n.h}
            rx={n.type === 'client' ? 10 : 3}
            fill={surface}
            stroke={stroke}
            strokeWidth={SW}
            strokeDasharray={n.type === 'external' ? '6 4' : undefined}
          />
          <Lines lines={wrapText(n.name, n.w - 16, 11, 2)} x={n.w / 2} y={n.props.stereotype ? n.h / 2 - 6 : n.h / 2} size={11} weight={700} />
          {n.props.stereotype && (
            <text x={n.w / 2} y={n.h - 12} fontSize={8.5} fill={INK.muted} textAnchor="middle" fontFamily={MONO}>
              {n.props.stereotype}
            </text>
          )}
        </>
      )
    case 'database': {
      const ry = 9
      return (
        <>
          <path d={`M0 ${ry}V${n.h - ry}A${n.w / 2} ${ry} 0 0 0 ${n.w} ${n.h - ry}V${ry}`} fill={surface} stroke={stroke} strokeWidth={SW} />
          <ellipse cx={n.w / 2} cy={ry} rx={n.w / 2} ry={ry} fill={INK.header} stroke={stroke} strokeWidth={SW} />
          <Lines lines={wrapText(n.name, n.w - 12, 10.5, 2)} x={n.w / 2} y={n.h / 2 + 5} size={10.5} weight={700} />
        </>
      )
    }
    case 'queue':
      return (
        <>
          <rect width={n.w} height={n.h} rx={3} fill={surface} stroke={stroke} strokeWidth={SW} />
          <path d={`M${n.w - 30} 6V${n.h - 6}M${n.w - 20} 6V${n.h - 6}M${n.w - 10} 6V${n.h - 6}`} stroke={INK.muted} strokeWidth={1.2} />
          <Lines lines={wrapText(n.name, n.w - 46, 10.5, 2)} x={(n.w - 34) / 2} y={n.h / 2} size={10.5} weight={700} />
        </>
      )
    case 'zone':
      return (
        <>
          <rect width={n.w} height={n.h} rx={4} fill={fillOf(n, INK.paper)} fillOpacity={0.6} stroke={INK.muted} strokeWidth={1.4} strokeDasharray="8 5" />
          <text x={10} y={18} fontSize={10.5} fontWeight={700} fill={INK.muted} fontFamily={MONO}>
            {n.name}
          </text>
        </>
      )
    case 'terminator':
      return (
        <>
          <rect width={n.w} height={n.h} rx={n.h / 2} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Lines lines={wrapText(n.name, n.w - 24, 10.5, 2)} x={n.w / 2} y={n.h / 2} size={10.5} weight={600} />
        </>
      )
    case 'process':
      return (
        <>
          <rect width={n.w} height={n.h} rx={2} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Lines lines={wrapText(n.name, n.w - 16, 10.5, 3)} x={n.w / 2} y={n.h / 2} size={10.5} />
        </>
      )
    case 'decision':
      return (
        <>
          <path d={`M${n.w / 2} 0L${n.w} ${n.h / 2}L${n.w / 2} ${n.h}L0 ${n.h / 2}Z`} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Lines lines={wrapText(n.name, n.w * 0.55, 10, 2)} x={n.w / 2} y={n.h / 2} size={10} />
        </>
      )
    case 'io':
      return (
        <>
          <path d={`M16 0H${n.w}L${n.w - 16} ${n.h}H0Z`} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Lines lines={wrapText(n.name, n.w - 40, 10.5, 2)} x={n.w / 2} y={n.h / 2} size={10.5} />
        </>
      )
    case 'document':
      return (
        <>
          <path d={`M0 0H${n.w}V${n.h - 10}C${n.w * 0.75} ${n.h - 22} ${n.w * 0.5} ${n.h + 4} 0 ${n.h - 8}Z`} fill={surface} stroke={stroke} strokeWidth={SW} />
          <Lines lines={wrapText(n.name, n.w - 16, 10.5, 2)} x={n.w / 2} y={(n.h - 10) / 2} size={10.5} />
        </>
      )
    case 'text':
      return (
        <>
          <rect width={n.w} height={n.h} fill="transparent" />
          <text x={0} y={n.h / 2} fontSize={Math.max(11, Math.round(n.h * 0.58))} fontWeight={900} fill={INK.ink} dominantBaseline="middle" letterSpacing={-0.3}>
            {n.name}
          </text>
        </>
      )
  }
}

/** `sub`: linha secundária já traduzida (tipo de tarefa ou executor). */
export const NodeShape = memo(function NodeShape({ n, sub }: { n: DNode; sub?: string }) {
  return (
    <g transform={`translate(${n.x} ${n.y})`}>
      <NodeBody n={n} sub={sub} />
    </g>
  )
})

// ---------------------------------------------------------------------------
//  Arestas
// ---------------------------------------------------------------------------

type Head = 'filled' | 'open' | 'hollow' | 'diamondFilled' | 'diamondHollow' | 'circle' | 'slash' | 'smallDiamond' | 'dot' | null

function headPath(tip: Pt, from: Pt, head: Head, color: string): ReactNode {
  if (!head) return null
  const ang = Math.atan2(tip.y - from.y, tip.x - from.x)
  const rot = (dx: number, dy: number) => ({
    x: tip.x + dx * Math.cos(ang) - dy * Math.sin(ang),
    y: tip.y + dx * Math.sin(ang) + dy * Math.cos(ang),
  })
  const p = (pts: { x: number; y: number }[]) => pts.map((q, i) => `${i ? 'L' : 'M'}${q.x.toFixed(1)} ${q.y.toFixed(1)}`).join('') + 'Z'
  switch (head) {
    case 'filled':
      return <path d={p([tip, rot(-10, -5), rot(-10, 5)])} fill={color} stroke={color} strokeWidth={1} />
    case 'hollow':
      return <path d={p([tip, rot(-13, -7), rot(-13, 7)])} fill={INK.surface} stroke={color} strokeWidth={SW} />
    case 'open': {
      const a = rot(-10, -5)
      const b = rot(-10, 5)
      return <path d={`M${a.x} ${a.y}L${tip.x} ${tip.y}L${b.x} ${b.y}`} fill="none" stroke={color} strokeWidth={SW} />
    }
    case 'diamondFilled':
    case 'diamondHollow':
      return <path d={p([tip, rot(-8, -6), rot(-16, 0), rot(-8, 6)])} fill={head === 'diamondFilled' ? color : INK.surface} stroke={color} strokeWidth={SW} />
    case 'smallDiamond':
      return <path d={p([tip, rot(-6, -4.5), rot(-12, 0), rot(-6, 4.5)])} fill={INK.surface} stroke={color} strokeWidth={1.2} />
    case 'circle': {
      const c = rot(-4, 0)
      return <circle cx={c.x} cy={c.y} r={4} fill={INK.surface} stroke={color} strokeWidth={1.2} />
    }
    case 'dot':
      return <circle cx={tip.x} cy={tip.y} r={4.5} fill={color} />
    case 'slash': {
      const a = rot(-14, -6)
      const b = rot(-8, 6)
      return <path d={`M${a.x} ${a.y}L${b.x} ${b.y}`} stroke={color} strokeWidth={SW} />
    }
  }
}

interface EdgeStyle {
  dash?: string
  start: Head
  end: Head
  color: string
  keyword?: string
}

export function edgeStyle(e: DEdge, sourceType?: DNode['type']): EdgeStyle {
  const ink = INK.ink
  switch (e.type) {
    case 'association':
      return { start: null, end: null, color: ink }
    case 'aggregation':
      return { start: 'diamondHollow', end: null, color: ink }
    case 'composition':
      return { start: 'diamondFilled', end: null, color: ink }
    case 'generalization':
      return { start: null, end: 'hollow', color: ink }
    case 'realization':
      return { dash: '6 4', start: null, end: 'hollow', color: ink }
    case 'dependency':
      return { dash: '6 4', start: null, end: 'open', color: ink }
    case 'anchor':
      return { dash: '3 3', start: null, end: null, color: INK.muted }
    case 'message':
      return { start: null, end: 'filled', color: ink }
    case 'reply':
      return { dash: '6 4', start: null, end: 'open', color: INK.muted }
    case 'include':
      return { dash: '6 4', start: null, end: 'open', color: ink, keyword: '«include»' }
    case 'extend':
      return { dash: '6 4', start: null, end: 'open', color: ink, keyword: '«extend»' }
    case 'lostMessage':
      return { start: null, end: 'dot', color: ink }
    case 'foundMessage':
      return { start: 'dot', end: 'filled', color: ink }
    case 'controlFlow':
    case 'transition':
      return { start: null, end: 'open', color: ink }
    case 'usage':
      return { dash: '6 4', start: null, end: 'open', color: ink, keyword: '«use»' }
    case 'deploy':
      return { dash: '6 4', start: null, end: 'open', color: ink, keyword: '«deploy»' }
    case 'manifest':
      return { dash: '6 4', start: null, end: 'open', color: ink, keyword: '«manifest»' }
    case 'link':
      return { start: null, end: null, color: ink }
    case 'sequenceFlow': {
      const fromGateway = sourceType === 'gateway'
      const start: Head = e.isDefault ? 'slash' : e.condition?.trim() && !fromGateway ? 'smallDiamond' : null
      return { start, end: 'filled', color: ink }
    }
    case 'messageFlow':
      return { dash: '7 4', start: 'circle', end: 'hollow', color: INK.muted }
    case 'dataAssociation':
      return { dash: '2 3', start: null, end: null, color: INK.muted }
    case 'sync':
      return { start: null, end: 'filled', color: ink }
    case 'async':
      return { dash: '7 4', start: null, end: 'open', color: ink }
    case 'dataFlow':
      return { dash: '2 3', start: null, end: 'filled', color: INK.muted }
    case 'flow':
      return { start: null, end: 'filled', color: ink }
  }
}

function Label({ at, text, color = INK.muted, italic, mono, size = 9 }: { at: Pt; text: string; color?: string; italic?: boolean; mono?: boolean; size?: number }) {
  return (
    <text
      x={at.x}
      y={at.y}
      fontSize={size}
      fill={color}
      textAnchor="middle"
      dominantBaseline="middle"
      fontStyle={italic ? 'italic' : undefined}
      fontFamily={mono ? MONO : undefined}
      stroke={INK.surface}
      strokeWidth={3}
      paintOrder="stroke"
      strokeLinejoin="round"
    >
      {text}
    </text>
  )
}

export const EdgeShape = memo(function EdgeShape({ e, a, b, sourceType, selfLoop }: { e: DEdge; a: Pt; b: Pt; sourceType?: DNode['type']; selfLoop?: boolean }) {
  const st = edgeStyle(e, sourceType)
  const d = selfLoop ? `M${a.x} ${a.y}h28V${b.y}H${b.x}` : `M${a.x} ${a.y}L${b.x} ${b.y}`
  const mid = selfLoop ? { x: a.x + 34, y: (a.y + b.y) / 2 } : { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }
  const len = Math.hypot(b.x - a.x, b.y - a.y) || 1
  const ux = (b.x - a.x) / len
  const uy = (b.y - a.y) / len
  const nearA = { x: a.x + ux * 18 - uy * 9, y: a.y + uy * 18 + ux * 9 }
  const nearB = { x: b.x - ux * 18 - uy * 9, y: b.y - uy * 18 + ux * 9 }
  const horizontalMsg = e.type === 'message' || e.type === 'reply' || e.type === 'lostMessage' || e.type === 'foundMessage'
  const labelAt = { x: mid.x - uy * 9, y: mid.y + ux * 9 - (horizontalMsg ? 16 : 0) }
  const text = [st.keyword, e.label.trim(), e.condition?.trim() ? `[${e.condition.trim()}]` : ''].filter(Boolean).join(' ')
  return (
    <>
      <path d={d} fill="none" stroke={st.color} strokeWidth={e.type === 'anchor' ? 1 : SW} strokeDasharray={st.dash} />
      {headPath(b, selfLoop ? { x: b.x + 28, y: b.y } : a, st.end, st.color)}
      {headPath(a, selfLoop ? { x: a.x + 28, y: a.y } : b, st.start, st.color)}
      {text && <Label at={labelAt} text={text} italic={e.type === 'association' || e.type === 'composition' || e.type === 'aggregation'} color={e.type === 'message' ? INK.ink : INK.muted} size={9.5} />}
      {e.srcMult?.trim() && <Label at={nearA} text={e.srcMult.trim()} mono />}
      {e.dstMult?.trim() && <Label at={nearB} text={e.dstMult.trim()} mono />}
    </>
  )
})

/** Traço livre: polilinha suave. */
export function strokePath(points: number[]): string {
  if (points.length < 2) return ''
  let d = `M${points[0]} ${points[1]}`
  if (points.length === 2) return `${d}l0.1 0`
  for (let i = 2; i < points.length; i += 2) d += `L${points[i]} ${points[i + 1]}`
  return d
}
