import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getAppName } from '../branding'
import { COR_DA_MARCA } from '../cena/desenhoDaMoldura'
import { participanteLocal } from '../convidado'
import { aparadoAoTecto, Cena, Fonte, LUGARES_DA_CENA, RoomCompositor } from './compositor'
import { Destino, Directo, directoSuportado, EstadoDoDirecto } from '../studio/directo'
import { paraOAr } from '../studio/destinosGuardados'
import type { RoomCore } from './useRoomCore'

export const MAX_MULTICAM_DESTINOS = 4

/** O id da fonte «partilha de ecrã» no compositor. Um só sítio, para o herói e a lista baterem certo. */
export const ECRA = 'ecra'

/**
 * Multicâmara (anfitrião): compõe os participantes num canvas e envia-o ao
 * directo para até quatro destinos RTMP. Os objectos imperativos vivem em refs:
 * pô-los em estado re-renderizava a sala a cada frame.
 */
export function useMulticam(core: RoomCore) {
  const { t } = useTranslation()
  const { code } = core
  const [open, setOpen] = useState(false)
  const [cena, setCena] = useState<Cena>('grelha')
  const [focoIds, setFocoIds] = useState<string[]>([])
  const [destinos, setDestinos] = useState<Destino[]>([{ url: '', chave: '', rotulo: '' }])
  const [estado, setEstado] = useState<EstadoDoDirecto>({ fase: 'parado' })
  /** O quadro como fonte (canvas → stream). Com ele no palco, a cena é SÓ o quadro. */
  const [boardStream, setBoardStreamState] = useState<MediaStream | null>(null)
  const cenaAntesDoQuadro = useRef<{ cena: Cena; focoIds: string[] } | null>(null)
  const compositorRef = useRef<RoomCompositor | null>(null)
  const directoRef = useRef<Directo | null>(null)
  // O canvas do compositor não é do React — entra no DOM à mão sempre que o
  // painel (re)monta, e sai com ele.
  const previewRef = useCallback((node: HTMLDivElement | null) => {
    const c = compositorRef.current
    if (!node || !c) return
    c.canvas.className = 'rm-multicam__canvas'
    node.appendChild(c.canvas)
  }, [])

  // Segue quem entra e sai sem recriar o AudioContext (emudeceria o directo).
  useEffect(() => {
    if (!open || !compositorRef.current) return
    const fontes: Fonte[] = [
      ...(boardStream ? [{ id: 'quadro', nome: t('room.quadro.titulo'), stream: boardStream }] : []),
      // A PARTILHA DE ECRÃ como fonte, que é o que faz da `moldura` uma cena
      // de aula e não de painel. Entra pela mesma porta do quadro — o
      // compositor nunca soube distinguir uma câmara de outra coisa, e é essa
      // indiferença que torna isto barato.
      ...(core.presentation ? [{ id: ECRA, nome: t('room.multicam.ecra'), stream: core.presentation.stream }] : []),
      { id: 'eu', nome: participanteLocal().username, stream: core.localStreamRef.current },
      ...core.peers.filter((p) => p.stream).map((p): Fonte => ({ id: p.peerId, nome: p.username, stream: p.stream })),
    ]
    compositorRef.current.definirParticipantes(fontes)
    // O herói da moldura é o ecrã quando há ecrã. Quando não há, fica `null` e
    // o compositor cai para a primeira fonte — não para um rectângulo preto.
    compositorRef.current.heroiId = core.presentation ? ECRA : null
    compositorRef.current.marca = {
      logo: null,
      faixa: getAppName(),
      assinatura: getAppName(),
      cor: COR_DA_MARCA,
    }
    // `t` fora de propósito: este efeito não pode recriar o AudioContext (ver
    // o comentário acima — emudeceria o directo), e com `t` nas dependências
    // uma troca de língua fazia-o correr. CUSTO ASSUMIDO: o nome da fonte
    // «quadro» fica na língua antiga até alguém entrar ou sair.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, core.peers, core.localStreamRef, boardStream, core.presentation])

  useEffect(() => {
    if (compositorRef.current) compositorRef.current.cena = cena
    // TROCAR DE CENA APARA O FOCO ao tecto da cena nova, e tem de ser aqui.
    // A `moldura` deixa escolher três; as outras não. Sem este aparo, três
    // escolhas feitas na moldura sobreviviam a uma ida para `lado-a-lado`, e
    // então: três fichas acesas sob uma dica que diz «até duas», só duas no ar,
    // e a ficha da terceira a REMOVER em vez de trazer ao ar (o `toggleFoco` vê
    // o id na lista). O clique seguinte, com o `slice(-2)`, despejava de uma vez
    // as duas pessoas que estavam na emissão.
    //
    // O aparo é uma função PURA (`aparadoAoTecto`), e não um `slice` escrito
    // aqui: dentro de um efeito não se prova sem jsdom, que este repo não tem.
    // Devolve a MESMA lista quando já cabe, senão isto re-renderizava a sala a
    // cada troca de cena.
    setFocoIds((ids) => aparadoAoTecto(ids, cena))
  }, [cena])
  useEffect(() => {
    if (compositorRef.current) compositorRef.current.focoIds = focoIds
  }, [focoIds])

  // Sair da sala a emitir pára o directo e liberta o compositor.
  useEffect(
    () => () => {
      void directoRef.current?.parar()
      directoRef.current = null
      compositorRef.current?.destruir()
      compositorRef.current = null
    },
    [],
  )

  function openPanel() {
    if (!compositorRef.current) {
      const c = new RoomCompositor()
      c.cena = cena
      c.focoIds = focoIds
      // A pré-visualização desenha JÁ: antes só arrancava com o directo, e o
      // painel mostrava um quadro vazio precisamente quando se escolhe a cena.
      c.iniciarPreVisualizacao()
      compositorRef.current = c
    }
    setOpen(true)
  }

  async function close() {
    const d = directoRef.current
    directoRef.current = null
    await d?.parar()
    compositorRef.current?.destruir()
    compositorRef.current = null
    setOpen(false)
    setEstado({ fase: 'parado' })
  }

  async function goLive() {
    const c = compositorRef.current
    if (!c) return
    try {
      const alvos = paraOAr(destinos)
      const token = core.roomTokenRef.current
      if (!token) throw new Error(t('room.multicam.semToken'))
      const fluxo = c.montarFluxo()
      const d = new Directo()
      d.aoMudar = setEstado
      directoRef.current = d
      await d.comecar(fluxo, code, token, alvos)
    } catch (e) {
      setEstado({ fase: 'erro', motivo: (e as Error).message || t('room.multicam.erroDirecto') })
      directoRef.current = null
    }
  }

  async function stopLive() {
    const d = directoRef.current
    directoRef.current = null
    await d?.parar()
    setEstado({ fase: 'parado' })
  }

  /** Põe (ou tira) o quadro no palco da emissão. Tirar devolve a cena que estava. */
  function setBoardStream(stream: MediaStream | null) {
    setBoardStreamState((cur) => {
      if (cur && cur !== stream) cur.getTracks().forEach((tr) => tr.stop())
      return stream
    })
    if (stream) {
      if (!cenaAntesDoQuadro.current) cenaAntesDoQuadro.current = { cena, focoIds }
      setCena('solo')
      setFocoIds(['quadro'])
    } else if (cenaAntesDoQuadro.current) {
      setCena(cenaAntesDoQuadro.current.cena)
      setFocoIds(cenaAntesDoQuadro.current.focoIds)
      cenaAntesDoQuadro.current = null
    }
  }

  function toggleFoco(id: string) {
    setFocoIds((ids) => {
      // O tecto é o da CENA, lido da tabela `LUGARES_DA_CENA` e não escrito
      // outra vez aqui: um lugar em `solo`, dois em `lado-a-lado`, e na
      // `moldura` os que a coluna tem. Baixar o tecto da coluna não pode deixar
      // o anfitrião a escolher um quarto que nunca aparece.
      if (ids.includes(id)) return ids.filter((x) => x !== id)
      return [...ids, id].slice(-LUGARES_DA_CENA[cena])
    })
  }

  return {
    supported: directoSuportado(),
    open,
    openPanel,
    close,
    previewRef,
    cena,
    setCena,
    focoIds,
    toggleFoco,
    destinos,
    addDestino: () =>
      setDestinos((ds) => (ds.length >= MAX_MULTICAM_DESTINOS ? ds : [...ds, { url: '', chave: '', rotulo: '' }])),
    removeDestino: (i: number) => setDestinos((ds) => ds.filter((_, j) => j !== i)),
    updateDestino: (i: number, patch: Partial<Destino>) =>
      setDestinos((ds) => ds.map((d, j) => (j === i ? { ...d, ...patch } : d))),
    estado,
    boardOnStage: !!boardStream,
    setBoardStream,
    goLive,
    stopLive,
  }
}

export type Multicam = ReturnType<typeof useMulticam>
