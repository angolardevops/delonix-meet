/**
 * Inteligência e idiomas (`#/ai`) — o ecrã DelonixAISettings com APENAS o que
 * existe no código, e o estado medido quando se pode medir.
 *
 * Motores reais:
 *  - Web Speech do browser (Chrome/Edge): é o motor por omissão da sala quando
 *    existe (media.ts Transcriber.start sem preferLocal) e ENVIA o áudio aos
 *    servidores da Google. Cai para o Whisper local em erro de rede. Por isso
 *    este ecrã NÃO diz «processamento local por omissão»: diz qual é o motor
 *    deste browser.
 *  - Whisper-tiny em WASM, no browser (whisperWorker.ts): modelo servido pela
 *    própria instalação em /models (deploy/fetch-whisper.sh). Mede-se se o
 *    modelo está instalado e quanto pesa, e se já está na cache do browser.
 *  - whisper-server (WebSocket /asr): opt-in por browser (dx_asr_server, a
 *    mesma preferência das definições da sala). Testa-se a ligação a pedido.
 *  - Ollama no cluster (ai.rs): tradução de legendas e resumo da acta. Testa-se
 *    a pedido com uma tradução curta; sem OLLAMA_URL o servidor diz que não há.
 *
 * Línguas: as da transcrição na sala (NotesPanel) e os alvos de tradução de
 * ai.rs. Não há políticas por perfil, registo de consumo, dobragem, nó GPU com
 * fila, nem tradução do chat ligada na sala — nada disso aparece como controlo.
 */
import { ReactNode, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, translateCaption } from '../api'
import { useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { Icon } from '../ui/icons'
import { Button, Toggle } from '../ui/kit'
import '../ui/ai.css'

const MODEL_BASE = '/models/Xenova/whisper-tiny'
const MODEL_FILES = ['onnx/encoder_model_quantized.onnx', 'onnx/decoder_model_merged_quantized.onnx']
/** Línguas da transcrição na sala (room/NotesPanel.tsx STT_LANGS). */
const STT_LANGS = ['pt', 'en', 'es', 'fr', 'de', 'it'] as const
/** Alvos de tradução do servidor (server/src/ai.rs translate). */
const TRANSLATE_TARGETS = new Set(['pt', 'en', 'fr', 'es', 'de'])

type Probe = { s: 'idle' } | { s: 'busy' } | { s: 'ok'; detail: string } | { s: 'fail'; detail: string }

function readPref(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

/** Um ficheiro estático existe se a resposta não for a página da app (fallback SPA). */
async function staticSize(url: string, signal: AbortSignal): Promise<number | null> {
  const r = await fetch(url, { method: 'HEAD', signal, cache: 'no-store' })
  const type = r.headers.get('content-type') ?? ''
  if (!r.ok || type.includes('text/html')) return null
  return Number(r.headers.get('content-length') ?? 0)
}

export interface BrowserModel {
  installed: boolean
  bytes: number
  cached: boolean
  originUsage: number | null
}

async function probeBrowserModel(signal: AbortSignal): Promise<BrowserModel> {
  const sizes = await Promise.all(MODEL_FILES.map((f) => staticSize(`${MODEL_BASE}/${f}`, signal).catch(() => null)))
  const installed = sizes.every((s) => s !== null)
  let cached = false
  try {
    cached = 'caches' in window && (await caches.has('transformers-cache'))
  } catch {
    cached = false
  }
  let originUsage: number | null = null
  try {
    originUsage = (await navigator.storage?.estimate?.())?.usage ?? null
  } catch {
    originUsage = null
  }
  return { installed, bytes: sizes.reduce<number>((n, s) => n + (s ?? 0), 0), cached, originUsage }
}

/** Abre o WebSocket /asr e fecha-o logo: só prova que o serviço atende. */
function probeAsr(): Promise<boolean> {
  return new Promise((resolve) => {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    let done = false
    const finish = (ok: boolean) => {
      if (done) return
      done = true
      try {
        ws.close()
      } catch {
        /* já fechado */
      }
      resolve(ok)
    }
    const ws = new WebSocket(`${proto}://${location.host}/asr?lang=pt`)
    ws.onopen = () => finish(true)
    ws.onerror = () => finish(false)
    ws.onclose = () => finish(false)
    setTimeout(() => finish(false), 5000)
  })
}

export function fmtMb(bytes: number, locale: string): string {
  return `${(bytes / (1024 * 1024)).toLocaleString(locale, { maximumFractionDigits: bytes < 10 * 1024 * 1024 ? 1 : 0 })} MB`
}

function EngineRow({
  title,
  where,
  children,
  badge,
  active,
  dashed,
}: {
  title: string
  where: string
  children?: ReactNode
  badge: ReactNode
  active?: boolean
  dashed?: boolean
}) {
  return (
    <li className={`ai-engine${active ? ' ai-engine--on' : ''}${dashed ? ' ai-engine--dashed' : ''}`} aria-current={active || undefined}>
      <span className="ai-engine__ring" aria-hidden="true" />
      <div className="ai-engine__main">
        <div className="ai-engine__head">
          <span className="ai-engine__title">
            <strong>{title}</strong>
            <small className="dx-num">{where}</small>
          </span>
          {badge}
        </div>
        {children}
      </div>
    </li>
  )
}

function ProbeResult({ probe }: { probe: Probe }) {
  if (probe.s === 'ok') return <span className="ai-probe ai-probe--ok">{probe.detail}</span>
  if (probe.s === 'fail') return <span className="ai-probe ai-probe--fail">{probe.detail}</span>
  return null
}

export default function Intelligence() {
  const { t, i18n } = useTranslation()
  const locale = i18n.language
  const model = useAsync(probeBrowserModel, [])
  const [serverAsr, setServerAsr] = useState(() => readPref('dx_asr_server') === '1')
  const [asrProbe, setAsrProbe] = useState<Probe>({ s: 'idle' })
  const [llmProbe, setLlmProbe] = useState<Probe>({ s: 'idle' })
  // A mesma verificação de media.ts (speechSupported), sem trazer media.ts para este chunk.
  const hasWebSpeech = useMemo(() => 'webkitSpeechRecognition' in window || 'SpeechRecognition' in window, [])
  const sttLang = readPref('dx_stt_lang') ?? 'pt-PT'
  const ccLang = readPref('dx_cc_lang') ?? ''

  function chooseServerAsr(on: boolean) {
    setServerAsr(on)
    try {
      localStorage.setItem('dx_asr_server', on ? '1' : '0')
    } catch {
      /* navegação privada: a escolha vale só nesta página */
    }
  }

  // O motor que a sala usa NESTE browser, pela mesma ordem do Transcriber.
  const active: 'server' | 'webspeech' | 'wasm' = serverAsr ? 'server' : hasWebSpeech ? 'webspeech' : 'wasm'
  const local = active !== 'webspeech'

  const names = useMemo(() => {
    try {
      return new Intl.DisplayNames([locale], { type: 'language' })
    } catch {
      return null
    }
  }, [locale])
  const langName = (code: string) => {
    const n = names?.of(code)
    return n ? n.charAt(0).toLocaleUpperCase(locale) + n.slice(1) : code
  }

  async function testAsr() {
    setAsrProbe({ s: 'busy' })
    const ok = await probeAsr()
    setAsrProbe(ok ? { s: 'ok', detail: t('consola.ia.asrOk') } : { s: 'fail', detail: t('consola.ia.asrFalhou') })
  }

  async function testLlm() {
    setLlmProbe({ s: 'busy' })
    try {
      const r = await translateCaption(t('consola.ia.fraseTeste'), locale.startsWith('en') ? 'pt' : 'en')
      setLlmProbe({ s: 'ok', detail: t('consola.ia.llmOk', { texto: r.text }) })
    } catch (e) {
      setLlmProbe({ s: 'fail', detail: apiErrorMessage(e, t('consola.ia.llmFalhou')) })
    }
  }

  const m = model.state.s === 'ready' ? model.state.d : null

  return (
    <>
      <PageBar
        title={t('consola.ia.titulo')}
        meta={
          <span className={local ? 'ai-metachip ai-metachip--ok' : 'ai-metachip ai-metachip--warn'} data-testid="ai-meta">
            {local ? t('consola.ia.metaLocal') : t('consola.ia.metaGoogle')}
          </span>
        }
      />
      <div className="page ai-page">
        <div className="ai-top">
          <section className="ai-card" aria-labelledby="ai-onde">
            <header className="ai-card__head">
              <h2 id="ai-onde">{t('consola.ia.ondeCorre')}</h2>
              <span className="dx-num dx-muted">{t('consola.ia.motores', { count: 4 })}</span>
            </header>
            <ul className="ai-engines" role="list" data-testid="ai-active" aria-label={active === 'server' ? t('consola.ia.activoServer') : active === 'webspeech' ? t('consola.ia.activoWebspeech') : t('consola.ia.activoWasm')}>
              <EngineRow
                active={active === 'wasm'}
                title={t('consola.ia.wasm')}
                where={t('consola.ia.wasmOnde')}
                badge={
                  model.state.s === 'loading' ? (
                    <span className="ai-tag">{t('consola.ia.aMedir')}</span>
                  ) : m?.installed ? (
                    <span className="ai-tag ai-tag--ok">{t('consola.ia.instaladoCurto')}</span>
                  ) : (
                    <span className="ai-tag ai-tag--warn">{t('consola.ia.naoInstalado')}</span>
                  )
                }
              >
                {m && (
                  <div className="ai-engine__models">
                    <span className="ai-model dx-num">
                      {m.installed ? t('consola.ia.modeloTamanho', { tamanho: fmtMb(m.bytes, locale) }) : t('consola.ia.modeloNome')}
                    </span>
                    <span className="dx-spacer" />
                    <span className="dx-num dx-muted ai-engine__cache" data-testid="ai-cache">
                      {m.cached ? t('consola.ia.emCache') : t('consola.ia.semCache')}
                      {m.originUsage !== null && ` · ${t('consola.ia.usoOrigem', { tamanho: fmtMb(m.originUsage, locale) })}`}
                    </span>
                  </div>
                )}
              </EngineRow>
              <EngineRow
                active={active === 'webspeech'}
                title={t('consola.ia.webspeech')}
                where={t('consola.ia.webspeechOnde')}
                badge={
                  hasWebSpeech ? (
                    <span className="ai-tag ai-tag--warn">{t('consola.ia.enviaGoogle')}</span>
                  ) : (
                    <span className="ai-tag">{t('consola.ia.naoExisteBrowser')}</span>
                  )
                }
              />
              <EngineRow
                active={active === 'server'}
                title={t('consola.ia.whisperServer')}
                where={t('consola.ia.whisperServerOnde')}
                badge={<span className="ai-tag">{serverAsr ? t('consola.ia.preferido') : t('consola.ia.desligadoAqui')}</span>}
              >
                <div className="ai-engine__actions">
                  <Toggle label={t('consola.ia.usarServidor')} checked={serverAsr} onChange={(e) => chooseServerAsr(e.target.checked)} />
                  <span className="dx-spacer" />
                  <ProbeResult probe={asrProbe} />
                  <Button size="sm" variant="secondary" busy={asrProbe.s === 'busy'} onClick={() => void testAsr()}>
                    {t('consola.ia.testar')}
                  </Button>
                </div>
              </EngineRow>
              <EngineRow title={t('consola.ia.ollama')} where={t('consola.ia.ollamaOnde')} badge={<span className="ai-tag">{t('consola.ia.noServidor')}</span>}>
                <div className="ai-engine__actions">
                  <span className="dx-spacer" />
                  <ProbeResult probe={llmProbe} />
                  <Button size="sm" variant="secondary" busy={llmProbe.s === 'busy'} onClick={() => void testLlm()}>
                    {t('consola.ia.testar')}
                  </Button>
                </div>
              </EngineRow>
              <EngineRow
                dashed
                title={t('consola.ia.externoTitulo')}
                where={t('consola.ia.externoOnde')}
                badge={<span className="ai-tag ai-tag--bad">{t('consola.ia.externoEstado')}</span>}
              />
            </ul>
            <p className="ai-warning">
              <Icon name="alert" size={13} />
              <span>{t('consola.ia.pGoogle')}</span>
            </p>
          </section>

          <section className="ai-card" aria-labelledby="ai-idiomas">
            <header className="ai-card__head">
              <h2 id="ai-idiomas">{t('consola.ia.idiomas')}</h2>
              <span className="dx-num dx-muted">{t('consola.ia.idiomasContagem', { count: STT_LANGS.length })}</span>
            </header>
            <div className="dx-table-wrap ai-box">
              <table className="ai-langs" data-testid="ai-langs">
                <thead>
                  <tr>
                    <th scope="col">{t('consola.ia.colIdioma')}</th>
                    <th scope="col">{t('consola.ia.colTranscricao')}</th>
                    <th scope="col">{t('consola.ia.colLegendas')}</th>
                    <th scope="col">{t('consola.ia.colChat')}</th>
                  </tr>
                </thead>
                <tbody>
                  {STT_LANGS.map((code) => (
                    <tr key={code}>
                      <td>
                        <span className="dx-num ai-code">{code}</span> <strong>{langName(code)}</strong>
                      </td>
                      <td>
                        <Mark on label={t('consola.ia.sim')} />
                      </td>
                      <td>
                        <Mark on={TRANSLATE_TARGETS.has(code)} label={TRANSLATE_TARGETS.has(code) ? t('consola.ia.sim') : t('consola.ia.nao')} />
                      </td>
                      <td>
                        <Mark on={false} label={t('consola.ia.fChatEstado')} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="ai-origin">
              <span className="dx-num dx-muted">{t('consola.ia.origem')}</span>
              <strong>{t('consola.ia.origemValor', { lingua: sttLang })}</strong>
            </div>
            <div className="ai-origin">
              <span className="dx-num dx-muted">{t('consola.ia.legendasAqui')}</span>
              <strong>{ccLang ? langName(ccLang) : t('consola.ia.semTraducao')}</strong>
            </div>
          </section>
        </div>

        <div className="ai-bottom">
          <section className="ai-card" aria-labelledby="ai-func">
            <header className="ai-card__head">
              <h2 id="ai-func">{t('consola.ia.funcionalidades')}</h2>
            </header>
            <ul className="ai-features" role="list">
              <Feature title={t('consola.ia.fLegendas')} state={t('consola.ia.fLegendasEstado')} on />
              <Feature title={t('consola.ia.fTraducaoLegendas')} state={t('consola.ia.fTraducaoLegendasEstado')} on />
              <Feature title={t('consola.ia.fResumo')} state={t('consola.ia.fResumoEstado')} on />
              <Feature title={t('consola.ia.fSilencios')} state={t('consola.ia.fSilenciosEstado')} on />
              <Feature title={t('consola.ia.fChat')} state={t('consola.ia.fChatEstado')} on={false} />
            </ul>
          </section>

          <section className="ai-card ai-card--grow" aria-labelledby="ai-priv">
            <header className="ai-card__head">
              <h2 id="ai-priv">{t('consola.ia.privacidade')}</h2>
            </header>
            <ul className="ai-privacy" role="list">
              <li className="ai-privacy--warn">
                <Icon name="alert" size={12} />
                <span>{t('consola.ia.pGoogleCurto')}</span>
              </li>
              <li>
                <Icon name="check" size={12} />
                <span>{t('consola.ia.pLocal')}</span>
              </li>
              <li>
                <Icon name="check" size={12} />
                <span>{t('consola.ia.pOllama')}</span>
              </li>
              <li>
                <Icon name="check" size={12} />
                <span>{t('consola.ia.pActa')}</span>
              </li>
            </ul>
            <div className="ai-box ai-note-box">
              <strong>{t('consola.ia.semCustoTitulo')}</strong>
              <span className="dx-num dx-muted">{t('consola.ia.semCusto')}</span>
            </div>
          </section>
          {/* A terceira coluna do template é «Consumo esta semana»: não há
              medição de consumo de IA no servidor, e o espaço fica vazio em
              vez de um gráfico inventado. */}
          <div aria-hidden="true" />
        </div>
      </div>
    </>
  )
}

function Mark({ on, label }: { on: boolean; label: string }) {
  return (
    <span className={on ? 'ai-mark ai-mark--on' : 'ai-mark'}>
      <Icon name={on ? 'check' : 'minus'} size={12} />
      <span className="dx-sr-only">{label}</span>
    </span>
  )
}

function Feature({ title, state, on }: { title: string; state: string; on: boolean }) {
  const { t } = useTranslation()
  return (
    <li className="ai-feature">
      <span className="ai-feature__text">
        <strong>{title}</strong>
        <small className="dx-num dx-muted">{state}</small>
      </span>
      <span className={on ? 'ai-tag ai-tag--ok' : 'ai-tag'}>{on ? t('consola.ia.existe') : t('consola.ia.porLigar')}</span>
    </li>
  )
}
