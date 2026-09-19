import { MutableRefObject, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { HeadTracker } from '../../media'
import { webgl2Supported } from '../../media/glUtil'
import { IMMERSIVE_BUDGET, type BudgetVerdict } from '../../media/sharpen'
import type { OverlayRenderer, OverlayStats } from '../../media/videoOverlay'
import { readPref, saveDataOn, useBatteryLow, useReducedMotion, writePref } from '../enhance/deviceEnv'
import { createStore, type EnhanceTarget } from '../enhance/target'
import { useOverlay } from '../enhance/useOverlay'
import { ImmersiveRenderer } from './immersiveGl'
import {
  chooseTiltSource,
  computeParallax,
  immersiveBlock,
  microMotion,
  orientationToTilt,
  pointerToTilt,
  sceneTransition,
  smoothTilt,
  speechEnvelope,
  type Tilt,
  type TiltSource,
} from './parallax'
import { PersonSegmenter } from './personSegmenter'

export interface ImmersiveStats extends OverlayStats {
  segMs: number
  masks: number
  source: TiltSource
  gpu: boolean
}

export type ImmersiveOff = {
  why: 'budget' | 'lost' | 'unsupported' | 'segmenter'
  ms: number | null
  fps?: number
  src?: number
  /** Desligou por não acompanhar o ritmo do vídeo (e não por tempo por frame). */
  lagging?: boolean
} | null

export interface ImmersiveDeps {
  /** A minha câmara (para seguir a cabeça). */
  cameraTrackRef: MutableRefObject<MediaStreamTrack | null>
  /** O seguidor de cabeça da «sala 3D», se já estiver a correr — reutiliza-se. */
  headRef: MutableRefObject<HeadTracker | null>
  camOn: boolean
  speaking: Set<string>
  dataSaver: boolean
}

/** Segmentar um frame em cada dois: a máscara a 15 Hz chega e poupa metade do custo. */
const SEGMENT_EVERY = 2

/**
 * Palco imersivo «4D» (simulado) para o orador em destaque, por quem vê.
 * Pura composição local: o vídeo recebido é recortado e desenhado em camadas
 * neste dispositivo, e nada sai daqui.
 */
export function useImmersiveStage(target: EnhanceTarget | null, deps: ImmersiveDeps) {
  const [wanted, setWanted] = useState(() => readPref('dx_palco_imersivo', '0') === '1')
  /**
   * Seguir a cabeça com a MINHA câmara é opt-in. Medido a 2026-09-16: o
   * `HeadTracker` detecta caras a cada frame no thread principal e, com a
   * câmara falsa a 4K, levou o thread a 99,9 % — o custo do palco imersivo
   * inteiro é pequeno ao lado disso. Se a «sala 3D» já o tem ligado, reutiliza-se.
   */
  const [followHead, setFollowHeadState] = useState(() => readPref('dx_palco_cabeca', '0') === '1')
  const [off, setOff] = useState<ImmersiveOff>(null)
  const [stats] = useState(() => createStore<ImmersiveStats | null>(null))
  const reducedMotion = useReducedMotion()
  const batteryLow = useBatteryLow()
  const [webgl2] = useState(() => webgl2Supported())
  const block = immersiveBlock({ reducedMotion, saveData: deps.dataSaver || saveDataOn(), batteryLow, webgl2 })
  const running = wanted && !off && !block && !!target

  const segRef = useRef<PersonSegmenter | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const rendRef = useRef<ImmersiveRenderer | null>(null)
  const segReady = useRef(false)
  const speakingRef = useRef(false)
  speakingRef.current = !!target && deps.speaking.has(target.peerId)
  const depsRef = useRef(deps)
  depsRef.current = deps

  // ── Entradas de inclinação ────────────────────────────────────────────────
  const input = useRef({
    pointer: { x: 0, y: 0 } as Tilt,
    orientation: null as Tilt | null,
    orientationAt: null as number | null,
    orientationBase: null as { beta: number; gamma: number } | null,
    headAt: null as number | null,
    headLast: { x: Number.NaN, y: Number.NaN },
    ownHead: null as HeadTracker | null,
    tilt: { x: 0, y: 0 } as Tilt,
    env: 0,
    lastT: 0,
    sceneStart: 0,
    source: 'pointer' as TiltSource,
  })

  useEffect(() => {
    if (!running) return
    const i = input.current
    const onPointer = (e: PointerEvent) => {
      const area = document.querySelector('.rm-stagearea')
      if (!area) return
      i.pointer = pointerToTilt(e.clientX, e.clientY, area.getBoundingClientRect())
    }
    const onOrientation = (e: DeviceOrientationEvent) => {
      if (e.beta == null || e.gamma == null) return
      if (!i.orientationBase) i.orientationBase = { beta: e.beta, gamma: e.gamma }
      i.orientation = orientationToTilt(e.beta, e.gamma, i.orientationBase)
      i.orientationAt = performance.now()
    }
    window.addEventListener('pointermove', onPointer, { passive: true })
    window.addEventListener('deviceorientation', onOrientation)

    // A cabeça: reutiliza o seguidor da «sala 3D» se estiver ligado; senão, só
    // se a pessoa o pediu e tem câmara, arranca um próprio. Sem cara detectada
    // não manda (ver `chooseTiltSource`) — o rato ou o telemóvel continuam a guiar.
    let cancelled = false
    const cam = depsRef.current.cameraTrackRef.current
    if (followHead && !depsRef.current.headRef.current && cam && cam.readyState === 'live' && depsRef.current.camOn) {
      const ht = new HeadTracker()
      ht.start(new MediaStream([cam]))
        .then(() => {
          if (cancelled) ht.stop()
          else i.ownHead = ht
        })
        .catch(() => {})
    }
    return () => {
      cancelled = true
      window.removeEventListener('pointermove', onPointer)
      window.removeEventListener('deviceorientation', onOrientation)
      i.ownHead?.stop()
      i.ownHead = null
      i.orientationBase = null
      i.orientationAt = null
      i.headAt = null
    }
  }, [running, followHead])

  // Orador novo: transição de cena.
  const key = target ? target.peerId : ''
  useEffect(() => {
    input.current.sceneStart = performance.now()
  }, [key])

  // Canvas, contexto WebGL e segmentador vivem JUNTOS (o segmentador desenha no
  // contexto do canvas) e sobrevivem às remontagens do retrato. Libertam-se
  // quando o efeito deixa de ser pedido — o modelo pesa ~10 MB.
  const release = useCallback(() => {
    segRef.current?.close()
    segRef.current = null
    segReady.current = false
    rendRef.current?.destroy()
    rendRef.current = null
    canvasRef.current = null
  }, [])
  useEffect(() => {
    if (!wanted) release()
  }, [wanted, release])
  useEffect(() => release, [release])

  const handlers = useMemo(
    () => ({
      canvas: () => {
        if (!canvasRef.current || rendRef.current?.lost) {
          release()
          canvasRef.current = document.createElement('canvas')
        }
        return canvasRef.current
      },
      make: (c: HTMLCanvasElement): OverlayRenderer | null => {
        if (!rendRef.current) {
          const r = ImmersiveRenderer.create(c)
          if (!r) return null
          rendRef.current = r
          c.dataset.imersivo = 'on'
          const s = new PersonSegmenter()
          segRef.current = s
          s.init(c)
            .then(() => {
              if (segRef.current === s) segReady.current = true
            })
            .catch((e) => {
              console.warn('[imersivo] segmentador indisponível', e)
              if (segRef.current === s) {
                segRef.current = null
                setOff({ why: 'segmenter', ms: null })
              }
            })
        }
        const r = rendRef.current
        input.current.sceneStart = performance.now()
        // O overlay destrói o renderer ao sair; este é partilhado entre
        // remontagens e só o `release` o destrói.
        return {
          render: (v, w, h) => r.render(v, w, h),
          finish: () => r.finish(),
          get lost() {
            return r.lost
          },
          destroy: () => {},
        }
      },
      beforeRender: (_o: OverlayRenderer, video: HTMLVideoElement, now: number, frame: number) => {
        const i = input.current
        const r = rendRef.current
        if (!r) return
        const seg = segRef.current
        if (seg && segReady.current && frame % SEGMENT_EVERY === 0) {
          seg.segment(video, (m) => r.acceptMask(m))
        }
        // Cabeça: conta como «a detectar» quando os valores mudam.
        const head = depsRef.current.headRef.current ?? i.ownHead
        if (head && (head.x !== i.headLast.x || head.y !== i.headLast.y)) {
          if (!Number.isNaN(i.headLast.x)) i.headAt = now
          i.headLast = { x: head.x, y: head.y }
        }
        const source = chooseTiltSource(now, i.headAt, i.orientationAt)
        i.source = source
        const goal = source === 'head' && head ? { x: head.x, y: head.y } : source === 'orientation' && i.orientation ? i.orientation : i.pointer
        const dt = i.lastT ? Math.min(100, now - i.lastT) : 16
        i.lastT = now
        i.tilt = smoothTilt(i.tilt, goal, dt)
        i.env = speechEnvelope(i.env, speakingRef.current, dt)
        const offsets = computeParallax(i.tilt)
        const micro = microMotion(i.env, now / 1000)
        const trans = sceneTransition(now - i.sceneStart)
        r.frame = {
          offsets,
          fgScale: 1.02 * micro.scale * trans.scale,
          lift: micro.lift,
          fade: trans.fade,
          light: { x: 0.35 + i.tilt.x * 0.08, y: 0.22 + i.tilt.y * 0.05 },
          dof: 2.5,
        }
        // Prova legível de fora (e2e, depuração): camadas e deslocamento actuais.
        if (frame % 6 === 0) {
          const c = canvasRef.current
          if (c) {
            c.dataset.bgX = offsets.bg.x.toFixed(4)
            c.dataset.bgY = offsets.bg.y.toFixed(4)
            c.dataset.fonte = source
            c.dataset.fala = i.env.toFixed(2)
            c.dataset.escala = r.frame.fgScale.toFixed(4)
            c.dataset.mascaras = String(r.masksUploaded)
            c.dataset.camadas = r.masksUploaded > 0 ? 'fundo,sombra,pessoa' : 'fundo'
          }
        }
      },
      onStats: (s: OverlayStats | null) => {
        const seg = segRef.current
        stats.set(
          s
            ? { ...s, segMs: Math.round((seg?.lastMs ?? 0) * 10) / 10, masks: seg?.masks ?? 0, source: input.current.source, gpu: !!seg?.usingGpu }
            : null,
        )
      },
      onEnd: (why: 'budget' | 'lost' | 'unsupported', last: OverlayStats | null, verdict: BudgetVerdict | null) =>
        setOff({ why, ms: last?.p95Ms ?? null, fps: last?.fps ?? 0, src: last?.sourceFps ?? 0, lagging: verdict?.why === 'fps' }),
      // Só se julga o custo depois de o recorte estar a produzir máscaras.
      ready: () => segReady.current && (segRef.current?.masks ?? 0) >= 3,
    }),
    [stats, release],
  )
  // Palco a 1080p no máximo: o fundo é desfocado por desenho e a pessoa vem de
  // um vídeo que raramente passa disso.
  useOverlay(running, target, IMMERSIVE_BUDGET, handlers, 1920 * 1080)

  const toggle = useCallback(() => {
    // iOS só entrega a orientação depois de uma permissão pedida num gesto.
    const DOE = (window as { DeviceOrientationEvent?: { requestPermission?: () => Promise<string> } }).DeviceOrientationEvent
    if (!wanted && DOE?.requestPermission) void DOE.requestPermission().catch(() => {})
    setWanted((v) => {
      writePref('dx_palco_imersivo', v ? '0' : '1')
      return !v
    })
    setOff((o) => (o?.why === 'unsupported' ? o : null))
  }, [wanted])

  const setFollowHead = useCallback((v: boolean) => {
    setFollowHeadState(v)
    writePref('dx_palco_cabeca', v ? '1' : '0')
  }, [])

  return { wanted, running, toggle, off, block, stats, target, followHead, setFollowHead }
}

export type ImmersiveStage = ReturnType<typeof useImmersiveStage>
