import { useCallback, useEffect, useRef, useState } from 'react'
import type { StatEntry } from '../../callQuality'
import type { LocalConditions } from '../../layerPolicy'
import {
  contentHintFor,
  decideProfile,
  degradationFor,
  downgradeReason,
  encodingsFor,
  INITIAL_PROFILE_STATE,
  parseSendStats,
  SHARP_CAMERA_CONSTRAINTS,
  TUNED_FIELDS,
  type DowngradeReason,
  type ProfileState,
  type SendProfile,
  type SendStats,
  type SenderSnapshot,
} from '../../media/sendProfile'
import type { RoomCore } from '../useRoomCore'
import { createStore } from './target'

export interface SharpSendView {
  /** Amostra tirada ANTES de o perfil ser aplicado — o termo de comparação. */
  before: SendStats | null
  now: SendStats | null
}

const TICK_MS = 2000

function sameTuning(a: RTCRtpEncodingParameters[], b: RTCRtpEncodingParameters[]): boolean {
  if (a.length !== b.length) return false
  return a.every((e, i) => TUNED_FIELDS.every((k) => e[k] === b[i][k]))
}

async function statsOf(sender: RTCRtpSender): Promise<SendStats | null> {
  try {
    const report = await sender.getStats()
    const entries: StatEntry[] = []
    report.forEach((v) => entries.push(v as unknown as StatEntry))
    return parseSendStats(entries)
  } catch {
    return null
  }
}

/**
 * Perfil de envio «Aula/apresentação nítida» — a decisão vive em
 * `media/sendProfile.ts`; aqui só se mede, decide e aplica, a cada 2 s.
 *
 * Não toca em nada enquanto nunca foi pedido: quem não liga o perfil continua
 * com o sender exactamente como `webrtc.ts` o criou. Ao desligar, repõe o
 * instantâneo tirado antes da primeira alteração.
 */
export function useSharpSend(core: RoomCore, conditions: LocalConditions) {
  const [wanted, setWanted] = useState(false)
  const [profile, setProfile] = useState<{ active: SendProfile; reason: DowngradeReason | null; refused: boolean }>({ active: 'normal', reason: null, refused: false })
  const [store] = useState(() => createStore<SharpSendView>({ before: null, now: null }))
  const stateRef = useRef<ProfileState>(INITIAL_PROFILE_STATE)
  const touchedRef = useRef(false)
  const snapshots = useRef(new WeakMap<RTCRtpSender, SenderSnapshot>())
  const camOriginal = useRef(new WeakMap<MediaStreamTrack, MediaTrackConstraints>())
  const camApplied = useRef(new WeakMap<MediaStreamTrack, SendProfile>())
  const condRef = useRef(conditions)
  condRef.current = conditions
  const warnedRef = useRef(false)
  const wantedRef = useRef(wanted)
  wantedRef.current = wanted

  /** Devolve `false` se o browser recusou os parâmetros de algum sender. */
  const apply = useCallback(async (senders: RTCRtpSender[], p: SendProfile): Promise<boolean> => {
    let accepted = true
    for (const sender of senders) {
      // O contentHint é da TRACK e não depende do `setParameters` passar.
      const track = sender.track
      const hint = contentHintFor(p)
      if (track && track.contentHint !== hint) track.contentHint = hint
      try {
        const params = sender.getParameters()
        if (!params.encodings?.length) continue
        if (!snapshots.current.has(sender)) {
          snapshots.current.set(sender, { encodings: params.encodings.map((e) => ({ ...e })), degradationPreference: params.degradationPreference })
        }
        const snap = snapshots.current.get(sender)!
        const want = encodingsFor(p, snap.encodings)
        const deg = degradationFor(p, snap)
        if (!sameTuning(params.encodings, want) || params.degradationPreference !== deg) {
          params.encodings = params.encodings.map((e, i) => {
            const next = { ...e }
            for (const k of TUNED_FIELDS) {
              if (want[i][k] === undefined) delete next[k]
              else next[k] = want[i][k]
            }
            return next
          })
          if (deg) params.degradationPreference = deg
          else delete params.degradationPreference
          await sender.setParameters(params)
        }
      } catch (e) {
        accepted = false
        if (!warnedRef.current) console.warn('[nitidez] o browser recusou os parâmetros do sender', e)
        warnedRef.current = true
      }
    }
    // A câmara em si: 1080p/30 quando a câmara o dá. Só se pede UMA vez por
    // track e perfil — `applyConstraints` reconfigura a câmara e, repetido a
    // cada 2 s, piscava a imagem.
    const cam = core.cameraTrackRef.current
    if (cam && cam.readyState === 'live' && camApplied.current.get(cam) !== p) {
      try {
        if (p === 'sharp') {
          if (!camOriginal.current.has(cam)) camOriginal.current.set(cam, cam.getConstraints())
          const orig = camOriginal.current.get(cam)!
          await cam.applyConstraints({ ...(orig.deviceId ? { deviceId: orig.deviceId } : {}), ...SHARP_CAMERA_CONSTRAINTS })
        } else if (camOriginal.current.has(cam)) {
          await cam.applyConstraints(camOriginal.current.get(cam)!)
        }
        camApplied.current.set(cam, p)
      } catch (e) {
        console.warn('[nitidez] a câmara recusou as constraints', e)
        camApplied.current.set(cam, p)
      }
    }
    return accepted
  }, [core.cameraTrackRef])

  const tick = useCallback(async () => {
    const call = core.callRef.current
    if (!call?.cameraSenders) return
    const w = wantedRef.current
    if (!w && !touchedRef.current) return
    // Mesh a partilhar ecrã: o sender da câmara transporta o ECRÃ.
    if (core.sharing && core.topology !== 'sfu') return
    const senders = call.cameraSenders()
    if (!senders.length) return
    const stats = await statsOf(senders[0])
    const view = store.get()
    if (w && !view.before && stateRef.current.active === 'normal') store.set({ before: stats, now: stats })
    else store.set({ ...view, now: stats })

    const reason = downgradeReason(condRef.current, stats?.availableUpKbps ?? null)
    const next = decideProfile(w, stateRef.current, reason, Date.now())
    stateRef.current = next
    const accepted = await apply(senders, next.active)
    touchedRef.current = w || next.active !== 'normal'
    const refused = next.active === 'sharp' && !accepted
    setProfile((cur) =>
      cur.active === next.active && cur.reason === next.reason && cur.refused === refused ? cur : { active: next.active, reason: next.reason, refused },
    )
  }, [apply, core.callRef, core.sharing, core.topology, store])

  useEffect(() => {
    if (core.roomState !== 'in') return
    if (!wanted && !touchedRef.current) return
    void tick()
    const id = window.setInterval(() => void tick(), TICK_MS)
    return () => clearInterval(id)
  }, [core.roomState, wanted, tick])

  const toggle = useCallback(() => {
    // A guarda notifica componentes: nunca dentro do actualizador de estado
    // (o React avisa «Cannot update a component while rendering»).
    if (!wantedRef.current) store.set({ before: null, now: store.get().now })
    setWanted((v) => !v)
  }, [store])

  return { wanted, toggle, active: profile.active, reason: profile.reason, refused: profile.refused, store }
}

export type SharpSend = ReturnType<typeof useSharpSend>
