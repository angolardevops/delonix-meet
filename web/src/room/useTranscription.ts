import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { accessTokenValue, currentUser, saveMinutesByRoom, translateCaption } from '../api'
import { Transcriber } from '../media'
import type { RoomCore } from './useRoomCore'

/**
 * Acta (minutes) a partir das linhas transcritas: resumo por tópicos e
 * extracção heurística de decisões e acções. Nota bruta assistida — não
 * substitui revisão humana.
 */
export function buildMoM(lines: string[], t: TFunction, locale: string): string {
  if (lines.length === 0) return ''
  const clean = lines.map((l) => l.replace(/^\[\d{2}:\d{2}\]\s*/, '').trim()).filter(Boolean)
  const actionRe = /(vamos|temos de|precisamos|fica responsável|ação|acção|action|we will|we need|decided|decidimos|ficou decidido|próximo passo|next step|nous allons|il faut|décidé)/i
  const actions = clean.filter((l) => actionRe.test(l))
  const date = new Date().toLocaleDateString(locale, { day: 'numeric', month: 'long', year: 'numeric' })
  const out: string[] = []
  out.push(`# ${t('room.notas.actaTitulo', { data: date })}`, '')
  out.push(`## ${t('room.notas.actaResumo')}`, ...clean.slice(0, 12).map((l) => `- ${l}`), '')
  if (actions.length) out.push(`## ${t('room.notas.actaDecisoes')}`, ...actions.map((l) => `- [ ] ${l}`), '')
  out.push(`_${t('room.notas.actaIntervencoes', { count: clean.length })}_`)
  return out.join('\n')
}

/**
 * Legendas (CC) e notas IA. Um ÚNICO motor de voz corre sempre que as legendas
 * OU a transcrição partilhada estão ligadas — a Web Speech não permite várias
 * instâncias ao mesmo tempo.
 */
export function useTranscription(core: RoomCore) {
  const { t, i18n } = useTranslation()
  const { signal, code, setStatus } = core
  const locale = i18n.language === 'en' ? 'en-GB' : i18n.language === 'fr' ? 'fr-FR' : 'pt-PT'
  const me = currentUser()?.username ?? ''

  const [ccOn, setCcOn] = useState(false)
  const [caption, setCaption] = useState<{ who: string; text: string } | null>(null)
  const [serverAsr, setServerAsrState] = useState(() => localStorage.getItem('dx_asr_server') === '1')
  const [ccLang, setCcLang] = useState(() => localStorage.getItem('dx_cc_lang') ?? '')
  const [sttLang, setSttLangState] = useState(() => localStorage.getItem('dx_stt_lang') ?? 'pt-PT')
  const [transcribing, setTranscribing] = useState(false)
  /** Transcrição PARTILHADA ligada pelo anfitrião: cada cliente capta o seu mic. */
  const [scribeBy, setScribeBy] = useState<string | null>(null)
  const [lines, setLines] = useState<string[]>([])
  const [interim, setInterim] = useState('')
  const [momSaved, setMomSaved] = useState(false)

  const transcriberRef = useRef<Transcriber | null>(null)
  const transcribingRef = useRef(false)
  const ccOnRef = useRef(false)
  const ccLangRef = useRef(ccLang)
  const ccSeqRef = useRef(0)
  const interimSentAtRef = useRef(0)
  const interimXlateRef = useRef<{ timer: number | null; busy: boolean }>({ timer: null, busy: false })
  const linesRef = useRef<string[]>([])
  const momSavedRef = useRef(false)
  linesRef.current = lines
  momSavedRef.current = momSaved
  transcribingRef.current = transcribing
  ccOnRef.current = ccOn

  useEffect(() => {
    ccLangRef.current = ccLang
    localStorage.setItem('dx_cc_lang', ccLang)
  }, [ccLang])

  // Cada frase fica 8 s no ecrã.
  useEffect(() => {
    if (!caption) return
    const id = setTimeout(() => setCaption(null), 8000)
    return () => clearTimeout(id)
  }, [caption])

  const stamp = () => new Date().toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })

  /**
   * Mostra já a legenda original e, com tradução, substitui pela traduzida —
   * só se ainda for a mais recente. As parciais traduzem-se com debounce e em
   * voo único, para não bombardear o LLM local com um pedido por palavra.
   */
  function showCaption(who: string, text: string, isInterim = false) {
    const mySeq = ++ccSeqRef.current
    setCaption({ who, text })
    const target = ccLangRef.current
    if (!target) return
    const x = interimXlateRef.current
    const apply = () => {
      x.busy = true
      void translateCaption(text, target)
        .then((r) => {
          if (ccSeqRef.current === mySeq) setCaption({ who, text: r.text })
        })
        .catch(() => {})
        .finally(() => {
          x.busy = false
        })
    }
    if (!isInterim) {
      apply()
      return
    }
    if (x.timer != null) window.clearTimeout(x.timer)
    x.timer = window.setTimeout(() => {
      x.timer = null
      if (!x.busy) apply()
    }, 400)
  }
  const showCaptionRef = useRef(showCaption)
  showCaptionRef.current = showCaption

  useEffect(() => {
    const offs = [
      signal.on('transcript', (m) => {
        // A legenda só aparece a quem tem CC; as notas acumulam sempre.
        if (ccOnRef.current) showCaptionRef.current(m.username, m.text)
        setLines((l) => [...l, `[${stamp()}] ${m.username}: ${m.text}`])
      }),
      signal.on('transcript-interim', (m) => {
        if (ccOnRef.current) showCaptionRef.current(m.username, m.text, true)
      }),
      // NÃO se abre o painel aos outros: basta avisar que a fala é captada.
      signal.on('transcription', (m) => {
        setScribeBy(m.on ? m.by : null)
        setTranscribing(m.on)
        if (m.on) setStatus(t('room.estado.transcricaoIniciadaPor', { nome: m.by }))
      }),
    ]
    return () => offs.forEach((off) => off())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signal])

  // Ciclo de vida do motor.
  useEffect(() => {
    const want = (ccOn || transcribing) && core.roomState === 'in'
    if (want && !transcriberRef.current) {
      const tr = new Transcriber()
      tr.onFinal = (text) => {
        if (ccOnRef.current) showCaptionRef.current(me, text)
        setInterim('')
        signal.send({ type: 'transcript', text })
        // Só acumula nas notas com a transcrição ligada (CC é efémera).
        if (transcribingRef.current) setLines((l) => [...l, `[${stamp()}] ${me}: ${text}`])
      }
      tr.onInterim = (text) => {
        setInterim(text)
        if (ccOnRef.current && text) showCaptionRef.current(me, text, true)
        // A parcial vai aos outros a cada ~250 ms: acaba com a sensação de
        // «standby» enquanto a frase não termina.
        if (transcribingRef.current && text) {
          const now = Date.now()
          if (now - interimSentAtRef.current >= 250) {
            interimSentAtRef.current = now
            signal.send({ type: 'transcript-interim', text })
          }
        }
      }
      tr.onError = (message) => {
        setStatus(message)
        setInterim('')
        transcriberRef.current?.stop()
        transcriberRef.current = null
        setTranscribing(false)
        setCcOn(false)
      }
      tr.start(sttLang, core.localStreamRef.current)
      transcriberRef.current = tr
      if (!core.localStreamRef.current?.getAudioTracks().length) setInterim(t('room.notas.aEsperaDoMicrofone'))
    } else if (!want && transcriberRef.current) {
      transcriberRef.current.stop()
      transcriberRef.current = null
      setInterim('')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ccOn, transcribing, sttLang, core.roomState, serverAsr])

  useEffect(
    () => () => {
      transcriberRef.current?.stop()
      transcriberRef.current = null
    },
    [],
  )

  // Fechar o separador sem «Sair»: a acta vai por `fetch keepalive`, que
  // completa depois do unload. Lê o token REAL (`accessTokenValue`) — a versão
  // anterior lia uma chave que não existe e nunca guardava nada.
  useEffect(() => {
    function handleUnload() {
      if (!core.isHostRef.current || linesRef.current.length === 0 || momSavedRef.current) return
      const token = accessTokenValue()
      if (!token) return
      void fetch(`/api/rooms/${code}/minutes`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ minutes: buildMoM(linesRef.current, t, locale), transcript: linesRef.current.join('\n') }),
        keepalive: true,
      })
    }
    window.addEventListener('beforeunload', handleUnload)
    return () => window.removeEventListener('beforeunload', handleUnload)
  }, [code, t, locale, core.isHostRef])

  /** Só o anfitrião liga a transcrição partilhada; o eco do servidor actualiza. */
  function toggleTranscription() {
    if (!core.isHost) return
    signal.send({ type: 'transcription-toggle', on: !transcribing })
  }

  async function saveMinutes() {
    try {
      await saveMinutesByRoom(code, buildMoM(lines, t, locale), lines.join('\n'))
      setMomSaved(true)
      setStatus(t('room.estado.actaGuardada'))
      setTimeout(() => setMomSaved(false), 3000)
    } catch {
      setStatus(t('room.estado.actaNaoGuardada'))
    }
  }

  /** Antes de sair: o anfitrião não perde as notas ao encerrar. */
  async function saveOnLeave() {
    if (!core.isHostRef.current || linesRef.current.length === 0 || momSavedRef.current) return
    setStatus(t('room.estado.aGuardarActa'))
    await saveMinutesByRoom(code, buildMoM(linesRef.current, t, locale), linesRef.current.join('\n'))
  }

  /** A gravação no servidor parou: guarda a acta sem bloquear a interface. */
  function saveOnServerRecordingStop() {
    if (!core.isHostRef.current || linesRef.current.length === 0 || momSavedRef.current) return
    saveMinutesByRoom(code, buildMoM(linesRef.current, t, locale), linesRef.current.join('\n'))
      .then(() => {
        setMomSaved(true)
        setStatus(t('room.estado.actaGuardadaAutomaticamente'))
      })
      .catch(() => {})
  }

  return {
    ccOn,
    toggleCc: () => setCcOn((v) => !v),
    caption,
    ccLang,
    setCcLang,
    sttLang,
    setSttLang: (v: string) => {
      setSttLangState(v)
      localStorage.setItem('dx_stt_lang', v)
    },
    serverAsr,
    setServerAsr: (on: boolean) => {
      setServerAsrState(on)
      localStorage.setItem('dx_asr_server', on ? '1' : '0')
    },
    transcribing,
    scribeBy,
    lines,
    interim,
    momSaved,
    toggleTranscription,
    saveMinutes,
    saveOnLeave,
    saveOnServerRecordingStop,
  }
}

export type Transcription = ReturnType<typeof useTranscription>
