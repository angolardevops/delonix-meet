import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { deveTrocarFonte, escolherFontePip, type EstadoPip } from '../pipPolicy'
import type { RoomCore } from './useRoomCore'

/**
 * Janela flutuante (Picture-in-Picture, W3.5). Numa reunião de trabalho
 * ninguém fica no separador da reunião; a janela mostra UM vídeo, e quem lá
 * está decide-se no `pipPolicy.ts` — puro e testado à parte. Aqui fica só o
 * que precisa mesmo do browser.
 */
export function usePip(core: RoomCore, pinnedId: string | null) {
  const { t } = useTranslation()
  const { peers, speaking, presentation } = core
  const [pipOn, setPipOn] = useState(false)
  const [pipErro, setPipErro] = useState<string | null>(null)
  const pipVideo = useRef<HTMLVideoElement>(null)
  const pipFonte = useRef<string | null>(null)
  /** Quem falou por último, mesmo calado — o silêncio é a maior parte da reunião. */
  const ultimoAFalar = useRef<string | null>(null)
  // Só abre a partir de um gesto da pessoa. Firefox e Safari de iOS não têm a
  // API: o botão não aparece, em vez de falhar em silêncio.
  const pipDisponivel = typeof document !== 'undefined' && document.pictureInPictureEnabled === true

  function estado(aFalar: boolean): EstadoPip {
    return {
      apresentacao: presentation ? presentation.peerId : null,
      afixado: pinnedId && pinnedId !== 'me' ? pinnedId : null,
      ultimoAFalar: ultimoAFalar.current,
      // O próprio nunca é candidato: a nossa cara numa janela não serve.
      candidatos: core.peersRef.current
        .filter((p) => p.peerId !== 'me')
        .map((p) => ({
          peerId: p.peerId,
          temVideo: !!p.stream && p.stream.getVideoTracks().some((tr) => tr.enabled),
          aFalar: aFalar && speaking.has(p.peerId),
        })),
    }
  }

  function streamDe(id: string): MediaStream | null {
    return presentation && id === presentation.peerId
      ? presentation.stream
      : core.peersRef.current.find((p) => p.peerId === id)?.stream ?? null
  }

  /**
   * Abre ou fecha. O pedido EXIGE um gesto e exige que o elemento já tenha
   * imagem — a fonte liga-se aqui, antes de pedir.
   */
  async function alternarPip() {
    setPipErro(null)
    const v = pipVideo.current
    if (!v) return
    if (document.pictureInPictureElement) {
      await document.exitPictureInPicture().catch(() => {})
      setPipOn(false)
      pipFonte.current = null
      return
    }
    const escolhido = escolherFontePip(estado(false))
    if (!escolhido) {
      // Sala só de áudio: uma janela preta a flutuar é pior do que nenhuma.
      setPipErro(t('room.pip.nadaParaMostrar'))
      return
    }
    const stream = streamDe(escolhido)
    if (!stream) {
      setPipErro(t('room.pip.nadaParaMostrar'))
      return
    }
    v.srcObject = stream
    try {
      await v.play()
      await v.requestPictureInPicture()
      pipFonte.current = escolhido
      setPipOn(true)
    } catch {
      // Política do browser, ou já há outra janela. Recusa silenciosa é um
      // botão partido.
      setPipErro(t('room.pip.recusada'))
    }
  }

  // Fechar pelo botão do browser tem de apagar o estado — senão o visto do
  // menu ficava aceso e o carregar seguinte não reabria nada.
  useEffect(() => {
    const v = pipVideo.current
    if (!v) return
    const onSai = () => {
      setPipOn(false)
      pipFonte.current = null
    }
    v.addEventListener('leavepictureinpicture', onSai)
    return () => v.removeEventListener('leavepictureinpicture', onSai)
  }, [])

  useEffect(() => {
    const aFalar = peers.find((p) => speaking.has(p.peerId))
    if (aFalar) ultimoAFalar.current = aFalar.peerId
  }, [speaking, peers])

  // Segue a fonte com a janela aberta; `deveTrocarFonte` evita o pisca-pisca.
  useEffect(() => {
    if (!pipOn) return
    const v = pipVideo.current
    if (!v) return
    const e = estado(true)
    if (!deveTrocarFonte(pipFonte.current, e)) return
    const escolhido = escolherFontePip(e)
    if (!escolhido) return
    const stream = streamDe(escolhido)
    if (!stream) return
    pipFonte.current = escolhido
    v.srcObject = stream
    void v.play().catch(() => {})
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pipOn, peers, speaking, presentation, pinnedId])

  return { pipVideo, pipOn, pipErro, pipDisponivel, alternarPip, clearPipErro: () => setPipErro(null) }
}
