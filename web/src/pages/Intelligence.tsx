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
import { Icon, IconName } from '../ui/icons'
import { Button, Card, StatusBadge, Toggle } from '../ui/kit'
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
  icon,
  title,
  where,
  children,
  badge,
}: {
  icon: IconName
  title: string
  where: string
  children?: ReactNode
  badge: ReactNode
}) {
  return (
    <li className="ai-engine">
      <span className="ai-engine__icon" aria-hidden="true">
        <Icon name={icon} />
      </span>
      <div className="ai-engine__main">
        <div className="ai-engine__head">
          <strong>{title}</strong>
          {badge}
        </div>
        <div className="dx-muted ai-engine__where">{where}</div>
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
          <span data-testid="ai-meta">
            {local ? t('consola.ia.metaLocal') : t('consola.ia.metaGoogle')}
          </span>
        }
      />
      <div className="page ai-page">
        <div className="ai-grid">
          <Card title={t('consola.ia.ondeCorre')} eyebrow={t('consola.ia.motores', { count: 4 })} className="ai-card">
            <p className="dx-muted ai-note" data-testid="ai-active">
              {active === 'server' ? t('consola.ia.activoServer') : active === 'webspeech' ? t('consola.ia.activoWebspeech') : t('consola.ia.activoWasm')}
            </p>
            <ul className="ai-engines" role="list">
              <EngineRow
                icon="globe"
                title={t('consola.ia.webspeech')}
                where={t('consola.ia.webspeechOnde')}
                badge={
                  hasWebSpeech ? (
                    <StatusBadge tone="warning" icon="alert">
                      {t('consola.ia.enviaGoogle')}
                    </StatusBadge>
                  ) : (
                    <StatusBadge tone="neutral">{t('consola.ia.naoExisteBrowser')}</StatusBadge>
                  )
                }
              />
              <EngineRow
                icon="cpu"
                title={t('consola.ia.wasm')}
                where={t('consola.ia.wasmOnde')}
                badge={
                  model.state.s === 'loading' ? (
                    <StatusBadge tone="neutral">{t('consola.ia.aMedir')}</StatusBadge>
                  ) : m?.installed ? (
                    <StatusBadge tone="success">{t('consola.ia.instalado', { tamanho: fmtMb(m.bytes, locale) })}</StatusBadge>
                  ) : (
                    <StatusBadge tone="warning">{t('consola.ia.naoInstalado')}</StatusBadge>
                  )
                }
              >
                {m && (
                  <div className="dx-muted ai-engine__detail" data-testid="ai-cache">
                    {m.cached ? t('consola.ia.emCache') : t('consola.ia.semCache')}
                    {m.originUsage !== null && ` · ${t('consola.ia.usoOrigem', { tamanho: fmtMb(m.originUsage, locale) })}`}
                  </div>
                )}
              </EngineRow>
              <EngineRow
                icon="server"
                title={t('consola.ia.whisperServer')}
                where={t('consola.ia.whisperServerOnde')}
                badge={
                  serverAsr ? (
                    <StatusBadge tone="success">{t('consola.ia.preferido')}</StatusBadge>
                  ) : (
                    <StatusBadge tone="neutral">{t('consola.ia.desligadoAqui')}</StatusBadge>
                  )
                }
              >
                <div className="ai-engine__actions">
                  <Toggle
                    label={t('consola.ia.usarServidor')}
                    hint={t('consola.ia.usarServidorDica')}
                    checked={serverAsr}
                    onChange={(e) => chooseServerAsr(e.target.checked)}
                  />
                  <Button size="sm" variant="secondary" icon="signal" busy={asrProbe.s === 'busy'} onClick={() => void testAsr()}>
                    {t('consola.ia.testar')}
                  </Button>
                  <ProbeResult probe={asrProbe} />
                </div>
              </EngineRow>
              <EngineRow
                icon="sparkles"
                title={t('consola.ia.ollama')}
                where={t('consola.ia.ollamaOnde')}
                badge={<StatusBadge tone="neutral">{t('consola.ia.noServidor')}</StatusBadge>}
              >
                <div className="ai-engine__actions">
                  <Button size="sm" variant="secondary" icon="signal" busy={llmProbe.s === 'busy'} onClick={() => void testLlm()}>
                    {t('consola.ia.testar')}
                  </Button>
                  <ProbeResult probe={llmProbe} />
                </div>
              </EngineRow>
            </ul>
            <p className="ai-external">
              <Icon name="ban" size={14} />
              <span>{t('consola.ia.externo')}</span>
            </p>
          </Card>

          <Card title={t('consola.ia.idiomas')} eyebrow={t('consola.ia.idiomasContagem', { count: STT_LANGS.length })} flush className="ai-card">
            <div className="dx-table-wrap">
              <table className="dx-table ai-langs" data-testid="ai-langs">
                <thead>
                  <tr>
                    <th scope="col">{t('consola.ia.colIdioma')}</th>
                    <th scope="col">{t('consola.ia.colTranscricao')}</th>
                    <th scope="col">{t('consola.ia.colLegendas')}</th>
                  </tr>
                </thead>
                <tbody>
                  {STT_LANGS.map((code) => (
                    <tr key={code}>
                      <td>
                        <span className="dx-num ai-code">{code}</span> {langName(code)}
                      </td>
                      <td>
                        <Mark on label={t('consola.ia.sim')} />
                      </td>
                      <td>
                        <Mark on={TRANSLATE_TARGETS.has(code)} label={TRANSLATE_TARGETS.has(code) ? t('consola.ia.sim') : t('consola.ia.nao')} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <dl className="dx-kv ai-kv">
              <dt>{t('consola.ia.origem')}</dt>
              <dd>{t('consola.ia.origemValor', { lingua: sttLang })}</dd>
              <dt>{t('consola.ia.legendasAqui')}</dt>
              <dd>{ccLang ? langName(ccLang) : t('consola.ia.semTraducao')}</dd>
            </dl>
          </Card>

          <Card title={t('consola.ia.funcionalidades')} className="ai-card">
            <ul className="ai-features" role="list">
              <Feature icon="captions" title={t('consola.ia.fLegendas')} state={t('consola.ia.fLegendasEstado')} on />
              <Feature icon="globe" title={t('consola.ia.fTraducaoLegendas')} state={t('consola.ia.fTraducaoLegendasEstado')} on />
              <Feature icon="notes" title={t('consola.ia.fResumo')} state={t('consola.ia.fResumoEstado')} on />
              <Feature icon="scissors" title={t('consola.ia.fSilencios')} state={t('consola.ia.fSilenciosEstado')} on />
              <Feature icon="chat" title={t('consola.ia.fChat')} state={t('consola.ia.fChatEstado')} on={false} />
            </ul>
          </Card>

          <Card title={t('consola.ia.privacidade')} className="ai-card">
            <ul className="ai-privacy" role="list">
              <li>
                <Icon name="alert" size={14} />
                <span>{t('consola.ia.pGoogle')}</span>
              </li>
              <li>
                <Icon name="check" size={14} />
                <span>{t('consola.ia.pLocal')}</span>
              </li>
              <li>
                <Icon name="check" size={14} />
                <span>{t('consola.ia.pOllama')}</span>
              </li>
              <li>
                <Icon name="check" size={14} />
                <span>{t('consola.ia.pActa')}</span>
              </li>
            </ul>
            <p className="dx-muted ai-note">{t('consola.ia.semCusto')}</p>
          </Card>
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

function Feature({ icon, title, state, on }: { icon: IconName; title: string; state: string; on: boolean }) {
  const { t } = useTranslation()
  return (
    <li className="ai-feature">
      <Icon name={icon} size={16} />
      <span className="ai-feature__text">
        <strong>{title}</strong>
        <small className="dx-muted">{state}</small>
      </span>
      <StatusBadge tone={on ? 'success' : 'neutral'}>{on ? t('consola.ia.existe') : t('consola.ia.porLigar')}</StatusBadge>
    </li>
  )
}
