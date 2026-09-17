/**
 * Coluna esquerda, na ordem do template: separadores Fontes/Marca/Som, a
 * «GRAVAÇÃO DA SESSÃO» (o bin) e, em baixo, a «IA NO BROWSER» — pausas e
 * palavras de preenchimento.
 */
import { useState } from 'react'
import type { ReactNode } from 'react'
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, recordingObjectUrl } from '../../api'
import type { RecordingLibraryItem } from '../../api'
import { cx, Dialog, IconButton, Select, Spinner } from '../../ui/kit'
import { recordingsFallback } from '../../pages/recordings/search'
import { SearchBar, SearchResults } from '../../ui/search/SearchResults'
import { useResourceSearch } from '../../ui/search/useResourceSearch'
import { relogio } from '../captions/legendas'
import { tamanhoLegivel } from '../exports/predefinicoes'
import { CartaoDeMistura } from './Inspector'
import type { Canto, Edicao, FaixaDeClipe, Fonte, OrigemDaFonte, Projecto } from './projecto'
import type { FonteEmBruto } from './useProjecto'

export type AbaDoBin = 'fontes' | 'marca' | 'som'

export interface ResumoDePausas {
  n: number
  poupanca: number
}

function etiqueta(f: Fonte): string {
  return f.tipo === 'audio' ? 'A' : 'V'
}

export function Biblioteca({ onFechar, onEscolher }: { onFechar: () => void; onEscolher: (r: RecordingLibraryItem) => Promise<void> }) {
  const { t, i18n } = useTranslation()
  const [aImportar, setAImportar] = useState<string | null>(null)
  // Pesquisa estilo Odoo sobre a biblioteca (recurso `recordings`, ou a lista
  // inteira no browser); o estado fica no diálogo, não na URL do Estúdio.
  const rs = useResourceSearch<RecordingLibraryItem>({ resource: 'recordings', ns: null, fallback: recordingsFallback })
  return (
    <Dialog title={t('editor.bin.biblioteca')} onClose={onFechar} wide>
      <div className="ed-lib-search">
        <SearchBar rs={rs} label={t('search.rotulos.recordings')} />
        <SearchResults
          rs={rs}
          emptyIcon="film"
          emptyTitle={t('editor.bin.bibliotecaVazia')}
          skeleton={<Spinner label={t('editor.bin.aCarregar')} />}
          renderItems={(lista) => (
            <ul className="ed-lib" data-studio="biblioteca">
              {lista.map((r) => (
                <li key={r.id}>
                  <button
                    type="button"
                    className="ed-lib__item"
                    disabled={!!aImportar}
                    onClick={async () => {
                      setAImportar(r.id)
                      try {
                        await onEscolher(r)
                        onFechar()
                      } finally {
                        setAImportar(null)
                      }
                    }}
                  >
                    <span className="ed-lib__name">{r.filename}</span>
                    <span className="dx-num dx-muted">
                      {new Date(r.created_at).toLocaleString(i18n.language)} · {tamanhoLegivel(r.size_bytes, i18n.language)}
                    </span>
                    {aImportar === r.id && <Spinner label={t('editor.bin.aImportar')} />}
                  </button>
                </li>
              ))}
            </ul>
          )}
        />
      </div>
    </Dialog>
  )
}

export default function Bin({
  p,
  aba,
  onAba,
  aplicar,
  acrescentar,
  onErro,
  pausas,
  aProcurarPausas,
  onProcurarPausas,
  onAplicarPausas,
  onCancelarPausas,
  preenchimento,
  onIrParaLegendas,
  marcaDeAgua,
  assistente,
}: {
  /** O cartão da IA do servidor, por baixo da IA no browser. */
  assistente?: ReactNode
  p: Projecto
  aba: AbaDoBin
  onAba: (a: AbaDoBin) => void
  aplicar: (e: Edicao, chave?: string | null) => void
  acrescentar: (fs: FonteEmBruto[]) => Promise<Fonte[]>
  onErro: (msg: string) => void
  pausas: ResumoDePausas | null
  aProcurarPausas: boolean
  onProcurarPausas: () => void
  onAplicarPausas: () => void
  onCancelarPausas: () => void
  preenchimento: { termo: string; n: number }[] | null
  onIrParaLegendas: () => void
  marcaDeAgua: string
}) {
  const { t, i18n } = useTranslation()
  const [biblioteca, setBiblioteca] = useState(false)
  const [aImportar, setAImportar] = useState(false)
  const [escolhida, setEscolhida] = useState<string | null>(null)
  const ficheiro = useRef<HTMLInputElement>(null)
  const musica = useRef<HTMLInputElement>(null)
  const temAudio = p.fontes.some((f) => f.tipo !== 'video')

  async function importar(blob: Blob, nome: string, origem: OrigemDaFonte, faixaMusica = false, gravacao?: string) {
    setAImportar(true)
    try {
      const [f] = await acrescentar([{ blob, nome, origem, ...(gravacao ? { gravacao } : {}) }])
      if (f && faixaMusica && f.tipo !== 'video') aplicar({ tipo: 'inserir', fonteId: f.id, faixa: 'A2', inicio: 0 })
    } catch (e) {
      onErro(apiErrorMessage(e, t('editor.bin.importarErro')))
    } finally {
      setAImportar(false)
    }
  }

  const inserir = (f: Fonte, faixa: FaixaDeClipe) => aplicar({ tipo: 'inserir', fonteId: f.id, faixa })

  return (
    <aside className="ed-col ed-col--left" aria-label={t('editor.bin.rotulo')}>
      <div className="ed-seg" role="group" aria-label={t('editor.bin.rotulo')}>
        {(['fontes', 'marca', 'som'] as const).map((a) => (
          <button key={a} type="button" aria-pressed={aba === a} onClick={() => onAba(a)}>
            {t(`editor.bin.abas.${a}`)}
          </button>
        ))}
      </div>

      {aba === 'fontes' && (
        <section className="ed-group" data-studio="bin">
          <h2 className="ed-label">{t('editor.bin.titulo')}</h2>
          {!p.fontes.length && <p className="st-note">{t('editor.bin.vazio')}</p>}
          {p.fontes.map((f) => {
            const on = escolhida === f.id
            return (
              <div key={f.id} className={cx('ed-src', on && 'ed-src--on')} data-fonte={f.origem}>
                <button type="button" className="ed-src__main" aria-pressed={on} onClick={() => setEscolhida(on ? null : f.id)}>
                  <span className={cx('ed-src__tag', `ed-src__tag--${f.tipo}`)} aria-hidden="true">
                    {etiqueta(f)}
                  </span>
                  <span className="ed-src__meta">
                    <span className="ed-src__name">{f.nome}</span>
                    <span className="dx-num ed-src__sub">
                      {f.altura ? `${f.altura}p · ` : ''}
                      {relogio(f.duracao)}
                    </span>
                  </span>
                </button>
                {on && (
                  <div className="ed-src__acts">
                    {f.tipo !== 'audio' && (
                      <>
                        <button type="button" className="ed-mini dx-num" aria-label={t('editor.bin.inserirEm', { faixa: 'V1', nome: f.nome })} onClick={() => inserir(f, 'V1')}>
                          V1
                        </button>
                        <button type="button" className="ed-mini dx-num" aria-label={t('editor.bin.inserirEm', { faixa: 'V2', nome: f.nome })} onClick={() => inserir(f, 'V2')}>
                          V2
                        </button>
                      </>
                    )}
                    {f.tipo !== 'video' && (
                      <>
                        <button type="button" className="ed-mini dx-num" aria-label={t('editor.bin.inserirEm', { faixa: 'A1', nome: f.nome })} onClick={() => inserir(f, 'A1')}>
                          A1
                        </button>
                        <button type="button" className="ed-mini dx-num" aria-label={t('editor.bin.inserirEm', { faixa: 'A2', nome: f.nome })} onClick={() => inserir(f, 'A2')}>
                          A2
                        </button>
                      </>
                    )}
                    <span className="dx-spacer" />
                    <IconButton icon="trash" bare label={t('editor.bin.remover', { nome: f.nome })} onClick={() => aplicar({ tipo: 'remover-fonte', fonteId: f.id })} />
                  </div>
                )}
              </div>
            )
          })}
          <div className="ed-grid2">
            <button type="button" className="ed-btn" disabled={aImportar} onClick={() => setBiblioteca(true)} data-studio="importar-biblioteca">
              {t('editor.bin.daBiblioteca')}
            </button>
            <button type="button" className="ed-btn" disabled={aImportar} onClick={() => ficheiro.current?.click()}>
              {t('editor.bin.ficheiro')}
            </button>
          </div>
          <input
            ref={ficheiro}
            type="file"
            accept="video/*,audio/*"
            hidden
            onChange={(e) => {
              const f = e.target.files?.[0]
              e.target.value = ''
              if (f) void importar(f, f.name, 'ficheiro')
            }}
          />
          {aImportar && <Spinner label={t('editor.bin.aImportar')} />}
          <p className="dx-num ed-src__sub">{t('editor.bin.bytes', { tamanho: tamanhoLegivel(p.fontes.reduce((n, f) => n + f.bytes, 0), i18n.language) })}</p>
        </section>
      )}

      {aba === 'marca' && (
        <section className="ed-group">
          <h2 className="ed-label">{t('editor.marca.titulo')}</h2>
          <button
            type="button"
            className={cx('ed-btn', p.marca.marcaDeAgua && 'ed-btn--on')}
            aria-pressed={p.marca.marcaDeAgua}
            onClick={() => aplicar({ tipo: 'marca', patch: { marcaDeAgua: !p.marca.marcaDeAgua } })}
          >
            {t('editor.marca.marcaDeAgua')}
          </button>
          <span className="dx-num ed-src__sub">{marcaDeAgua}</span>
          <label className="ed-label" htmlFor="ed-marca-canto">
            {t('editor.marca.canto')}
          </label>
          <Select id="ed-marca-canto" value={p.marca.canto} onChange={(e) => aplicar({ tipo: 'marca', patch: { canto: e.target.value as Canto } })}>
            {(['superior-esquerdo', 'superior-direito', 'inferior-esquerdo', 'inferior-direito'] as const).map((c) => (
              <option key={c} value={c}>
                {t(`editor.marca.cantos.${c.replace(/-(\w)/, (_, l: string) => l.toUpperCase())}`)}
              </option>
            ))}
          </Select>
          <label className="ed-label" htmlFor="ed-marca-op">
            {t('editor.marca.opacidade')}
          </label>
          <input
            id="ed-marca-op"
            type="range"
            className="ed-range"
            min={0.1}
            max={1}
            step={0.05}
            value={p.marca.opacidade}
            style={{ ['--pct' as string]: `${((p.marca.opacidade - 0.1) / 0.9) * 100}%` }}
            onChange={(e) => aplicar({ tipo: 'marca', patch: { opacidade: Number(e.target.value) } }, 'marca:opacidade')}
          />
          <p className="st-note">{t('editor.marca.nota')}</p>
        </section>
      )}

      {aba === 'som' && (
        <section className="ed-group">
          <h2 className="ed-label">{t('editor.som.titulo')}</h2>
          <button type="button" className="ed-btn" disabled={aImportar} onClick={() => musica.current?.click()}>
            {t('editor.som.musica')}
          </button>
          <input
            ref={musica}
            type="file"
            accept="audio/*"
            hidden
            onChange={(e) => {
              const f = e.target.files?.[0]
              e.target.value = ''
              if (f) void importar(f, f.name, 'ficheiro', true)
            }}
          />
          <p className="st-note">{t('editor.som.musicaNota')}</p>
          <CartaoDeMistura p={p} c={null} aplicar={aplicar} />
        </section>
      )}

      <section className="ed-group ed-group--end" data-studio="ia">
        <h2 className="ed-label">{t('editor.ia.titulo')}</h2>
        <div className={cx('ed-card', pausas && pausas.n > 0 && 'ed-card--accent')} data-studio="pausas">
          <span className="ed-card__title">{t('editor.ia.silencios')}</span>
          {!temAudio ? (
            <span className="dx-num ed-src__sub">{t('studio.erros.semAudio')}</span>
          ) : !pausas ? (
            <>
              <span className="dx-num ed-src__sub">{t('editor.ia.silenciosNota')}</span>
              <button type="button" className="ed-btn ed-btn--primary" disabled={aProcurarPausas} onClick={onProcurarPausas} data-studio="procurar-pausas">
                {aProcurarPausas ? t('studio.edicao.pausas.aAnalisar') : t('studio.edicao.pausas.procurar')}
              </button>
            </>
          ) : pausas.n === 0 ? (
            <span className="dx-num ed-src__sub st-note--ok">{t('studio.edicao.pausas.nenhuma')}</span>
          ) : (
            <>
              <span className="dx-num ed-src__sub">{t('editor.ia.cortes', { count: pausas.n, t: relogio(pausas.poupanca) })}</span>
              <button type="button" className="ed-btn ed-btn--primary" onClick={onAplicarPausas} data-studio="aplicar-pausas">
                {t('editor.ia.aplicarCortes')}
              </button>
              <button type="button" className="ed-link" onClick={onCancelarPausas}>
                {t('studio.edicao.pausas.cancelar')}
              </button>
            </>
          )}
        </div>
        <button type="button" className="ed-card ed-card--link" onClick={onIrParaLegendas}>
          <span className="ed-card__title">{t('editor.ia.preenchimento')}</span>
          <span className="dx-num ed-src__sub">
            {preenchimento === null
              ? t('editor.ia.transcreverPrimeiro')
              : preenchimento.length
                ? preenchimento
                    .slice(0, 3)
                    .map((x) => t('editor.ia.ocorrencia', { n: x.n, termo: x.termo }))
                    .join(', ')
                : t('editor.ia.semPreenchimento')}
          </span>
        </button>
      </section>
      {assistente}

      {biblioteca && (
        <Biblioteca
          onFechar={() => setBiblioteca(false)}
          onEscolher={async (r) => {
            let url = ''
            try {
              url = await recordingObjectUrl(r)
              const blob = await fetch(url).then((x) => x.blob())
              await importar(blob, r.filename, 'biblioteca', false, r.id)
            } catch (e) {
              onErro(apiErrorMessage(e, t('editor.bin.importarErro')))
            } finally {
              if (url) URL.revokeObjectURL(url)
            }
          }}
        />
      )}
    </aside>
  )
}
