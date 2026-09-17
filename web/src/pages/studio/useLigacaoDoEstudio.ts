/**
 * A ligação do Estúdio a uma SALA — para receber convidados, a sondagem, o
 * chat interno e as perguntas.
 *
 * PORQUE NÃO É O `useCallSession` DA SALA: aquele é dono de uma chamada
 * inteira (pré-entrada, denoiser, medidores, E2EE, salas paralelas) e, quando
 * o socket cai, RECARREGA A PÁGINA. No Estúdio isso destruía a gravação e o
 * directo que estão a decorrer. Aqui a queda é um estado — «ligação perdida,
 * religar» — e o palco continua.
 *
 * O que é igual, de propósito: entra-se pelo mesmo `joinRoom` (o servidor
 * autentica e decide), a chamada SFU só arranca DEPOIS de `joined` (a guarda
 * de `sfuLifecycle`, R1/R2), e o resto da sala regista-se no barramento
 * (`core.signal`) — por isso os hooks `useParticipants`, `useChat` e
 * `useMeetingTools` servem aqui sem alterações.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, ApiError, currentUser, iceServers, isAbort, joinRoom } from '../../api'
import { audioConstraints } from '../../media'
import { Signaling } from '../../signaling'
import { makeCallHolderStart } from '../../sfuLifecycle'
import { MeshCall, SfuCall } from '../../webrtc'
import type { RoomCore } from '../../room/useRoomCore'

export type EstadoDaLigacao = 'a-ligar' | 'a-espera' | 'ligada' | 'caiu' | 'recusada' | 'erro'

export function useLigacaoDoEstudio(
  core: RoomCore,
  { micId, obterCamara }: { micId: string; obterCamara: () => MediaStreamTrack | null },
) {
  const { t } = useTranslation()
  const { code, signal: bus, setPeers } = core
  const [estado, setEstado] = useState<EstadoDaLigacao>('a-ligar')
  const [erro, setErro] = useState('')
  const [tentativa, setTentativa] = useState(0)

  useEffect(() => {
    let cancelado = false
    let s: Signaling | null = null
    setEstado('a-ligar')
    setErro('')

    async function ligar() {
      try {
        const [{ room, room_token }, rtc] = await Promise.all([joinRoom(code), iceServers()])
        if (cancelado) return
        // Uma sala cifrada não pode emitir (o servidor recusa o directo) e o
        // palco não sabe decifrar sem a frase — dizê-lo já, e não depois.
        if (room.e2ee) {
          setEstado('erro')
          setErro(t('studio.sala.erros.e2ee'))
          return
        }
        core.roomTokenRef.current = room_token
        core.setTopology(room.topology)
        core.setIsHost(room.owner_id === currentUser()?.id)

        // O que os convidados recebem de quem está no estúdio: a voz (o mesmo
        // microfone da mistura) e, se estiver ligada, a câmara. Sem microfone
        // entra-se só a ouvir — um estúdio sem voz ainda pode pôr convidados no ar.
        const local = await navigator.mediaDevices
          .getUserMedia({ audio: audioConstraints(micId || undefined) })
          .catch(() => new MediaStream())
        const cam = obterCamara()
        if (cam) local.addTrack(cam.clone())
        if (cancelado) {
          local.getTracks().forEach((tr) => tr.stop())
          return
        }
        core.localStreamRef.current = local

        s = new Signaling(room_token, code)
        const sinal = s
        const holder = { start: () => {} }
        sinal.on('waiting', () => setEstado('a-espera'))
        sinal.on('denied', () => setEstado('recusada'))
        sinal.on('kicked', () => {
          setEstado('recusada')
          core.callRef.current?.hangup()
          core.callRef.current = null
        })
        sinal.on('error', (m) => setErro(m.message))
        sinal.on('joined', (m) => {
          core.meuPeerIdRef.current = m.peer_id
          if (m.reconnect) Signaling.guardarSegredo(code, m.reconnect)
          core.setRoomState('in')
          setEstado('ligada')
          holder.start() // SÓ agora, depois da admissão
        })
        sinal.onclose = () => {
          if (cancelado) return
          // A sala caiu; o palco NÃO. Nada de recarregar a página.
          setEstado('caiu')
          core.setRoomState('connecting')
        }
        holder.start = makeCallHolderStart({
          ref: core.callRef,
          isCancelled: () => cancelado,
          create: () => {
            const callbacks = {
              onStream: (peerId: string, remote: MediaStream) => {
                // O ecrã partilhado de um convidado não substitui a câmara dele.
                if (peerId.endsWith('-screen')) return
                setPeers((ps) => ps.map((p) => (p.peerId === peerId ? { ...p, stream: remote } : p)))
              },
              onPeerLeft: (peerId: string) =>
                setPeers((ps) => ps.map((p) => (p.peerId === peerId ? { ...p, stream: null } : p))),
            }
            return room.topology === 'sfu'
              ? new SfuCall(sinal, local, rtc, callbacks)
              : new MeshCall(sinal, local, rtc, callbacks)
          },
        })
        bus.attach(sinal)
      } catch (e) {
        if (cancelado || isAbort(e)) return
        setEstado('erro')
        setErro(
          e instanceof ApiError && e.status === 404 ? t('studio.sala.erros.naoExiste') : apiErrorMessage(e, t('studio.sala.erros.ligar')),
        )
      }
    }

    void ligar()
    return () => {
      cancelado = true
      if (s) bus.detach(s)
      if (!core.callRef.current) s?.close()
      core.callRef.current?.hangup()
      core.callRef.current = null
      core.localStreamRef.current?.getTracks().forEach((tr) => tr.stop())
      core.localStreamRef.current = null
      setPeers([])
    }
    // `micId`/`obterCamara` valem no momento de ligar; mudar de microfone a
    // meio não deita a ligação abaixo.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code, tentativa])

  return { estado, erro, religar: () => setTentativa((n) => n + 1) }
}
