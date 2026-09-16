/**
 * Inspector à direita, na ordem do template: separadores Corte/Cor/Áudio/Texto
 * e, empilhados, o cartão do clipe (entrada/saída, velocidade, separar áudio,
 * congelar), a correcção de cor, a mistura de áudio, os textos e o destino.
 *
 * Os separadores levam ao cartão correspondente (e marcam-no) em vez de
 * esconderem os outros — o template mostra os três cartões ao mesmo tempo.
 */
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button, cx, Select, TextInput } from '../../ui/kit'
import { lerTimecode, relogio, timecode } from '../captions/legendas'
import { tamanhoLegivel } from '../exports/predefinicoes'
import { valorComSinal } from './cor'
import type { Clip, Cor, Edicao, Projecto, Texto } from './projecto'
import { clipsDaFaixa, estadoDaFaixa, faixaDeVideo, fonte, novoId } from './projecto'
import type { Ferramenta } from './Timeline'
import type { Leitor } from './useLeitor'

export type AbaDoInspector = 'corte' | 'cor' | 'audio' | 'texto'

function Barra({
  rotulo,
  valor,
  min,
  max,
  passo,
  mostrar,
  onChange,
  id,
  tom,
}: {
  rotulo: string
  valor: number
  min: number
  max: number
  passo: number
  mostrar: string
  onChange: (v: number) => void
  id: string
  tom?: 'ok' | 'aviso'
}) {
  return (
    <div className="ed-slider">
      <label htmlFor={id} className="ed-slider__label">
        {rotulo}
      </label>
      <input
        id={id}
        type="range"
        className={cx('ed-range', tom && `ed-range--${tom}`)}
        min={min}
        max={max}
        step={passo}
        value={valor}
        style={{ ['--pct' as string]: `${((valor - min) / (max - min)) * 100}%` }}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      <span className="dx-num ed-slider__val">{mostrar}</span>
    </div>
  )
}

/** Campo de tempo editável: escreve-se `16:04` ou `00:16:04:12` e Enter aplica. */
function CampoDeTempo({
  valor,
  fps,
  rotulo,
  activo,
  onAplicar,
  dataStudio,
}: {
  valor: number
  fps: number
  rotulo: string
  activo?: boolean
  onAplicar: (s: number) => void
  dataStudio: string
}) {
  const [texto, setTexto] = useState(timecode(valor, fps).slice(0, 8))
  useEffect(() => setTexto(timecode(valor, fps).slice(0, 8)), [valor, fps])
  const aplicar = () => {
    const s = lerTimecode(texto, fps)
    if (Number.isFinite(s) && Math.abs(s - valor) > 1e-3) onAplicar(s)
    else setTexto(timecode(valor, fps).slice(0, 8))
  }
  return (
    <label className="ed-io">
      <span className="st-label">{rotulo}</span>
      <input
        className={cx('ed-io__input dx-num', activo && 'ed-io__input--on')}
        value={texto}
        inputMode="numeric"
        spellCheck={false}
        data-studio={dataStudio}
        onChange={(e) => setTexto(e.target.value)}
        onBlur={aplicar}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            aplicar()
          }
        }}
      />
    </label>
  )
}

function CartaoDoClip({
  p,
  c,
  aplicar,
  leitor,
  ripple,
  ferramenta,
}: {
  p: Projecto
  c: Clip | null
  aplicar: (e: Edicao, chave?: string | null) => void
  leitor: Leitor
  ripple: boolean
  ferramenta: Ferramenta
}) {
  const { t } = useTranslation()
  if (!c) {
    return (
      <div className="ed-card" data-cartao="corte">
        <p className="st-note">{t('editor.inspector.nada')}</p>
      </div>
    )
  }
  const indice = clipsDaFaixa(p, c.faixa).findIndex((x) => x.id === c.id)
  const f = fonte(p, c.fonteId)
  const video = faixaDeVideo(c.faixa)
  const verTransicao = !!c.transicao || ferramenta === 'transicao'
  const verMascara = video && (!!c.mascara || ferramenta === 'mascara')
  return (
    <div className="ed-card" data-cartao="corte">
      <div className="ed-card__title">
        <span className="ed-dot" aria-hidden="true" />
        <span className="ed-card__clip" title={t('editor.inspector.seleccionado', { faixa: c.faixa, n: indice + 1, nome: f?.nome ?? '' })}>
          {t('editor.inspector.seleccionado', { faixa: c.faixa, n: indice + 1, nome: f?.nome ?? '' })}
        </span>
      </div>
      {estadoDaFaixa(p, c.faixa).bloqueada && <p className="st-note">{t('editor.inspector.bloqueada')}</p>}
      {c.congelado !== null ? (
        <p className="st-note">{t('editor.inspector.congeladoNota', { s: c.congelado.toFixed(1) })}</p>
      ) : (
        <div className="ed-io-grid" data-studio="corte">
          <CampoDeTempo
            rotulo={t('studio.edicao.entrada')}
            valor={c.entrada}
            fps={p.fps}
            dataStudio="corte-de"
            onAplicar={(s) => aplicar({ tipo: 'aparar', clipId: c.id, entrada: s, ripple })}
          />
          <CampoDeTempo
            rotulo={t('studio.edicao.saida')}
            valor={c.saida}
            fps={p.fps}
            activo
            dataStudio="corte-ate"
            onAplicar={(s) => aplicar({ tipo: 'aparar', clipId: c.id, saida: s, ripple })}
          />
        </div>
      )}
      {c.congelado === null && (
        <div className="ed-speed">
          <div className="ed-row ed-row--between">
            <label className="st-label" htmlFor="ed-vel">
              {t('editor.inspector.velocidade')}
            </label>
            <span className="dx-num ed-slider__val">{`${c.velocidade.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}×`}</span>
          </div>
          <input
            id="ed-vel"
            type="range"
            className="ed-range"
            min={0.25}
            max={4}
            step={0.05}
            value={c.velocidade}
            style={{ ['--pct' as string]: `${((c.velocidade - 0.25) / 3.75) * 100}%` }}
            onChange={(e) => aplicar({ tipo: 'velocidade', clipId: c.id, velocidade: Number(e.target.value) }, `vel:${c.id}`)}
          />
        </div>
      )}
      <div className="ed-grid2">
        <button type="button" className="ed-btn" disabled={!c.grupo} onClick={() => aplicar({ tipo: 'separar-audio', clipId: c.id })}>
          {t('editor.inspector.separarAudio')}
        </button>
        <button type="button" className="ed-btn" disabled={!video} onClick={() => aplicar({ tipo: 'congelar', t: leitor.tempoRef.current, duracao: 2 })}>
          {t('editor.inspector.congelar')}
        </button>
      </div>
      {verTransicao && (
        <div className="ed-row">
          <label className="st-label" htmlFor="ed-trans">
            {t('editor.inspector.transicao')}
          </label>
          <Select
            id="ed-trans"
            value={c.transicao?.tipo ?? ''}
            onChange={(e) => {
              const tipo = e.target.value as '' | 'dissolver' | 'negro'
              aplicar({ tipo: 'transicao', clipId: c.id, transicao: tipo ? { tipo, duracao: c.transicao?.duracao ?? 0.5 } : null })
            }}
          >
            <option value="">{t('editor.inspector.semTransicao')}</option>
            <option value="dissolver">{t('editor.inspector.dissolver')}</option>
            <option value="negro">{t('editor.inspector.negro')}</option>
          </Select>
          {c.transicao && (
            <TextInput
              type="number"
              aria-label={t('editor.inspector.duracaoTransicao')}
              min={0.1}
              max={5}
              step={0.1}
              value={c.transicao.duracao}
              className="ed-num-input"
              onChange={(e) => c.transicao && aplicar({ tipo: 'transicao', clipId: c.id, transicao: { ...c.transicao, duracao: Number(e.target.value) || 0.5 } }, `trans:${c.id}`)}
            />
          )}
        </div>
      )}
      {verMascara && (
        <div className="ed-sub">
          <div className="ed-row ed-row--between">
            <span className="st-label">{t('editor.inspector.mascara')}</span>
            <button
              type="button"
              className={cx('ed-chip', c.mascara && 'ed-chip--on')}
              aria-pressed={!!c.mascara}
              onClick={() => aplicar({ tipo: 'mascara', clipId: c.id, mascara: c.mascara ? null : { x: 0.1, y: 0.1, w: 0.8, h: 0.8 } })}
            >
              {c.mascara ? t('editor.inspector.mascaraTirar') : t('editor.inspector.mascaraPor')}
            </button>
          </div>
          {c.mascara &&
            (['x', 'y', 'w', 'h'] as const).map((k) => (
              <Barra
                key={k}
                id={`ed-mask-${k}`}
                rotulo={t(`editor.inspector.mascara_${k}`)}
                valor={c.mascara![k]}
                min={k === 'w' || k === 'h' ? 0.05 : 0}
                max={1}
                passo={0.01}
                mostrar={`${Math.round(c.mascara![k] * 100)}%`}
                onChange={(v) => aplicar({ tipo: 'mascara', clipId: c.id, mascara: { ...c.mascara!, [k]: v } }, `mask:${c.id}:${k}`)}
              />
            ))}
        </div>
      )}
    </div>
  )
}

function CartaoDeCor({ c, aplicar }: { c: Clip | null; aplicar: (e: Edicao, chave?: string | null) => void }) {
  const { t } = useTranslation()
  const linhas: { k: keyof Cor; min: number; max: number; passo: number; casas: number }[] = [
    { k: 'exposicao', min: -1, max: 1, passo: 0.05, casas: 1 },
    { k: 'contraste', min: -100, max: 100, passo: 1, casas: 0 },
    { k: 'saturacao', min: -100, max: 100, passo: 1, casas: 0 },
    { k: 'temperatura', min: -100, max: 100, passo: 1, casas: 0 },
  ]
  const video = !!c && faixaDeVideo(c.faixa)
  return (
    <div className="ed-card" data-cartao="cor">
      <div className="ed-card__title">{t('editor.cor.titulo')}</div>
      {!video && <p className="st-note">{t('editor.inspector.corSoVideo')}</p>}
      {video &&
        linhas.map((l) => (
          <Barra
            key={l.k}
            id={`ed-cor-${l.k}`}
            rotulo={t(`editor.cor.${l.k}`)}
            valor={c!.cor[l.k]}
            min={l.min}
            max={l.max}
            passo={l.passo}
            mostrar={valorComSinal(c!.cor[l.k], l.casas)}
            onChange={(v) => aplicar({ tipo: 'cor', clipId: c!.id, cor: { [l.k]: v } }, `cor:${c!.id}:${l.k}`)}
          />
        ))}
    </div>
  )
}

export function CartaoDeMistura({ p, c, aplicar }: { p: Projecto; c: Clip | null; aplicar: (e: Edicao, chave?: string | null) => void }) {
  const { t } = useTranslation()
  const alvo = p.mistura.alvoLufs !== null
  return (
    <div className="ed-card" data-cartao="audio">
      <div className="ed-card__title ed-row--between">
        {/* «aplica-se na exportação» vai no título: a nota por baixo empurrava
            o destino do projecto para fora do ecrã a 900 px. */}
        <span title={t('editor.audio.soNaExportacao')}>{t('editor.audio.mistura')}</span>
        <button
          type="button"
          className={cx('ed-lufs dx-num', alvo && 'ed-lufs--on')}
          aria-pressed={alvo}
          onClick={() => aplicar({ tipo: 'mistura', patch: { alvoLufs: alvo ? null : -14 } })}
        >
          {t('editor.audio.alvo')}
        </button>
      </div>
      {(['A1', 'A2'] as const).map((f) => {
        const e = estadoDaFaixa(p, f)
        return (
          <Barra
            key={f}
            id={`ed-fader-${f}`}
            tom="ok"
            rotulo={t(`editor.audio.fader${f}`)}
            valor={e.ganhoDb}
            min={-60}
            max={12}
            passo={1}
            mostrar={valorComSinal(e.ganhoDb)}
            onChange={(v) => aplicar({ tipo: 'faixa', id: f, patch: { ganhoDb: v } }, `fader:${f}`)}
          />
        )
      })}
      {c && !faixaDeVideo(c.faixa) && (
        <Barra
          id="ed-ganho-clip"
          tom="aviso"
          rotulo={t('editor.audio.ganhoClip')}
          valor={c.ganhoDb}
          min={-30}
          max={18}
          passo={0.5}
          mostrar={valorComSinal(c.ganhoDb, 1)}
          onChange={(v) => aplicar({ tipo: 'ganho', clipId: c.id, ganhoDb: v }, `ganho:${c.id}`)}
        />
      )}
      <div className="ed-grid2">
        <button
          type="button"
          className={cx('ed-btn', p.mistura.reduzirRuido && 'ed-btn--on')}
          aria-pressed={p.mistura.reduzirRuido}
          onClick={() => aplicar({ tipo: 'mistura', patch: { reduzirRuido: !p.mistura.reduzirRuido } })}
        >
          {t('editor.audio.reduzirRuido')}
        </button>
        <button
          type="button"
          className={cx('ed-btn', p.mistura.normalizar && 'ed-btn--on')}
          aria-pressed={p.mistura.normalizar}
          disabled={alvo}
          title={alvo ? t('editor.audio.normalizarComAlvo') : undefined}
          onClick={() => aplicar({ tipo: 'mistura', patch: { normalizar: !p.mistura.normalizar } })}
        >
          {t('editor.audio.normalizar')}
        </button>
      </div>
    </div>
  )
}

function CartaoDeTextos({ p, leitor, aplicar }: { p: Projecto; leitor: Leitor; aplicar: (e: Edicao, chave?: string | null) => void }) {
  const { t } = useTranslation()
  const novo = (): Texto => ({ id: novoId('t'), inicio: leitor.tempoRef.current, duracao: 4, texto: t('editor.texto.omisso'), x: 0.5, y: 0.8, tamanho: 0.06 })
  return (
    <div className="ed-card" data-cartao="texto">
      <div className="ed-card__title ed-row--between">
        <span>{t('editor.texto.titulo')}</span>
        <button type="button" className="ed-chip" onClick={() => aplicar({ tipo: 'texto', texto: novo() })}>
          {t('editor.texto.acrescentar')}
        </button>
      </div>
      {!p.textos.length && <p className="st-note">{t('editor.texto.vazio')}</p>}
      {p.textos.map((x) => (
        <div key={x.id} className="ed-sub">
          <div className="ed-row ed-row--between">
            <button type="button" className="ed-link dx-num" onClick={() => leitor.buscar(x.inicio)}>
              {relogio(x.inicio)} · {x.duracao.toFixed(1)} s
            </button>
            <Button size="sm" variant="ghost" icon="trash" aria-label={t('editor.texto.remover')} onClick={() => aplicar({ tipo: 'remover-texto', id: x.id })} />
          </div>
          <TextInput value={x.texto} aria-label={t('editor.texto.conteudo')} maxLength={140} onChange={(e) => aplicar({ tipo: 'texto', texto: { ...x, texto: e.target.value } }, `texto:${x.id}`)} />
          <Barra id={`ed-tx-d-${x.id}`} rotulo={t('editor.texto.duracao')} valor={x.duracao} min={0.5} max={30} passo={0.5} mostrar={`${x.duracao.toFixed(1)} s`} onChange={(v) => aplicar({ tipo: 'texto', texto: { ...x, duracao: v } }, `tx-d:${x.id}`)} />
          <Barra id={`ed-tx-y-${x.id}`} rotulo={t('editor.texto.altura')} valor={x.y} min={0.05} max={0.95} passo={0.01} mostrar={`${Math.round(x.y * 100)}%`} onChange={(v) => aplicar({ tipo: 'texto', texto: { ...x, y: v } }, `tx-y:${x.id}`)} />
          <Barra id={`ed-tx-s-${x.id}`} rotulo={t('editor.texto.tamanho')} valor={x.tamanho} min={0.02} max={0.2} passo={0.005} mostrar={`${Math.round(x.tamanho * 1080)} px`} onChange={(v) => aplicar({ tipo: 'texto', texto: { ...x, tamanho: v } }, `tx-s:${x.id}`)} />
        </div>
      ))}
    </div>
  )
}

export default function Inspector({
  p,
  seleccao,
  aba,
  onAba,
  aplicar,
  leitor,
  ripple,
  ferramenta,
}: {
  p: Projecto
  seleccao: string | null
  aba: AbaDoInspector
  onAba: (a: AbaDoInspector) => void
  aplicar: (e: Edicao, chave?: string | null) => void
  leitor: Leitor
  ripple: boolean
  ferramenta: Ferramenta
}) {
  const { t, i18n } = useTranslation()
  const c = p.clips.find((x) => x.id === seleccao) ?? null
  const ref = useRef<HTMLElement>(null)
  const abas: AbaDoInspector[] = ['corte', 'cor', 'audio', 'texto']

  // O separador leva ao cartão: rolar e dar-lhe o foco visual.
  useEffect(() => {
    const el = ref.current?.querySelector<HTMLElement>(`[data-cartao="${aba}"]`)
    el?.scrollIntoView?.({ block: 'nearest', behavior: 'smooth' })
  }, [aba])

  return (
    <aside ref={ref} className="ed-col ed-col--right" aria-label={t('editor.inspector.rotulo')} data-aba={aba}>
      <div className="ed-seg" role="tablist" aria-label={t('editor.inspector.rotulo')}>
        {abas.map((a) => (
          <button key={a} type="button" role="tab" aria-selected={aba === a} onClick={() => onAba(a)}>
            {t(`editor.inspector.abas.${a}`)}
          </button>
        ))}
      </div>
      <CartaoDoClip p={p} c={c} aplicar={aplicar} leitor={leitor} ripple={ripple} ferramenta={ferramenta} />
      <CartaoDeCor c={c} aplicar={aplicar} />
      <CartaoDeMistura p={p} c={c} aplicar={aplicar} />
      {(aba === 'texto' || p.textos.length > 0) && <CartaoDeTextos p={p} leitor={leitor} aplicar={aplicar} />}
      <div className="ed-card ed-card--end">
        <div className="ed-card__title">{t('editor.destino.titulo')}</div>
        <span className="dx-num ed-mono">
          {t('editor.destino.local', { tamanho: tamanhoLegivel(p.fontes.reduce((n, f) => n + f.bytes, 0), i18n.language) })}
          <br />
          {t('editor.destino.ao')}
        </span>
      </div>
    </aside>
  )
}
