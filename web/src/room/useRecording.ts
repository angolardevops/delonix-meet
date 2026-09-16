import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser, downloadRecording, listRecordings, Recording, uploadRecording } from '../api'
import { MeetingRecorder } from '../media'
import type { RoomCore } from './useRoomCore'

/**
 * Gravação local (grelha composta no browser, carregada no fim) e gravação no
 * servidor (SFU). O aviso a TODOS é obrigatório nos dois casos.
 */
export function useRecording(core: RoomCore, hooks: { onServerStopped: () => void; onUploaded: () => void }) {
  const { t, i18n } = useTranslation()
  const { signal, code, setStatus } = core
  const [recording, setRecording] = useState(false)
  const [recBusy, setRecBusy] = useState(false)
  const [remoteRecorder, setRemoteRecorder] = useState('')
  /** Aviso transitório de início — o indicador persistente continua. */
  const [recNotice, setRecNotice] = useState('')
  const [serverRec, setServerRec] = useState<{ by: string } | null>(null)
  const [recordings, setRecordings] = useState<Recording[]>([])
  const recorderRef = useRef<MeetingRecorder | null>(null)
  const hooksRef = useRef(hooks)
  hooksRef.current = hooks

  const refresh = () => void listRecordings(code).then(setRecordings).catch(() => {})

  useEffect(() => {
    const offs = [
      signal.on('joined', () => refresh()),
      signal.on('recording', (m) => {
        setRemoteRecorder(m.active ? m.username : '')
        if (m.active) setRecNotice(t('room.gravacao.comecouAGravar', { nome: m.username }))
        else refresh()
      }),
      signal.on('server-recording', (m) => {
        setServerRec(m.active ? { by: m.by } : null)
        if (m.active) setRecNotice(t('room.gravacao.comecouAGravarNoServidor', { nome: m.by }))
        else hooksRef.current.onServerStopped()
      }),
    ]
    return () => offs.forEach((off) => off())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signal, code])

  useEffect(() => {
    if (!recNotice) return
    const id = setTimeout(() => setRecNotice(''), 6000)
    return () => clearTimeout(id)
  }, [recNotice])

  // A gravação segue as entradas, saídas e streams.
  useEffect(() => {
    recorderRef.current?.setSources([
      { id: 'me', label: currentUser()?.username ?? '', stream: core.localStreamRef.current },
      ...core.peers.map((p) => ({ id: p.peerId, label: p.username, stream: p.stream })),
    ])
  }, [core.peers, recording, core.localStreamRef])

  // Sair a meio de uma gravação local não deixa o gravador a correr.
  useEffect(
    () => () => {
      void recorderRef.current?.stop().catch(() => {})
      recorderRef.current = null
    },
    [],
  )

  async function toggleLocal() {
    if (recBusy) return
    if (!recording) {
      core.levelsRef.current?.resume()
      recorderRef.current = new MeetingRecorder()
      recorderRef.current.setSources([
        { id: 'me', label: currentUser()?.username ?? '', stream: core.localStreamRef.current },
        ...core.peersRef.current.map((p) => ({ id: p.peerId, label: p.username, stream: p.stream })),
      ])
      setRecording(true)
      setRecNotice(t('room.gravacao.comecasteAGravar'))
      signal.send({ type: 'recording', active: true })
      return
    }
    setRecBusy(true)
    try {
      const blob = await recorderRef.current!.stop()
      recorderRef.current = null
      setRecording(false)
      signal.send({ type: 'recording', active: false })
      setStatus(t('room.estado.aCarregarGravacao'))
      const now = new Date()
      const locale = i18n.language
      const stamp = `${now.toLocaleDateString(locale)} ${now.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })}`
      await uploadRecording(code, blob, t('room.gravacao.nomeFicheiro', { code, stamp }))
      setStatus('')
      setRecordings(await listRecordings(code))
      hooksRef.current.onUploaded()
    } catch {
      setStatus(t('room.estado.gravacaoNaoGuardada'))
    } finally {
      setRecBusy(false)
    }
  }

  /**
   * Gravação no servidor. Numa sala E2EE gravar exige CEDER a chave ao
   * servidor — só com o consentimento explícito que a vista pede antes.
   */
  function setServerRecording(active: boolean, e2eeKey: string | null = null) {
    signal.send(active ? { type: 'server-record', active: true, e2ee_key: e2eeKey } : { type: 'server-record', active: false })
  }

  function download(r: Recording) {
    void downloadRecording(r).catch(() => setStatus(t('room.estado.descargaFalhou')))
  }

  return {
    recording,
    recBusy,
    remoteRecorder,
    anyoneRecording: recording || !!remoteRecorder,
    recNotice,
    serverRec,
    recordings,
    toggleLocal,
    setServerRecording,
    download,
  }
}

export type RecordingState = ReturnType<typeof useRecording>
