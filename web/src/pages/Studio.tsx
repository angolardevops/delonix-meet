/**
 * Estúdio — gravar uma vídeo-aula e/ou emiti-la em directo: o ecrã (inteiro
 * ou uma REGIÃO) com a câmara por cima, numa bolha que se move, composto num
 * canvas 1920×1080.
 *
 * Vive fora da sala de propósito: não há SFU, não há pares, não há rede no
 * caminho da media. Uma aula grava-se sozinho, e prender isso ao caminho de
 * uma chamada seria pôr a falhar coisas que não têm de existir aqui.
 *
 * A página é dona do estado e fala com os objectos imperativos (compositor,
 * directo, arquivo). Os painéis em `studio/*.tsx` só desenham.
 */
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, createRoom, joinRoom, uploadRecording } from '../api'
import type { Fonte } from '../room/compositor'
import { BrandMark } from '../components/BrandMark'
import { useShell } from '../components/shellContext'
import { BackgroundEffect } from '../media'
import { Alert, Button, cx, IconButton, Spinner, StatusBadge } from '../ui/kit'
import { analisarPausas, AnaliseDeAudio, trocosSemPausas } from '../studio/analise'
import * as arquivo from '../studio/arquivo'
import AudioPanel from '../studio/AudioPanel'
import CenasPanel from '../studio/CenasPanel'
import { AVATAR_INICIAL, CompositorDeAula, EstadoDoAvatar, Recorte, RECORTE_INTEIRO } from '../studio/compositor'
import Cronometro from '../studio/Cronometro'
import { useDebito } from '../studio/debito'
import type { SondagemNoPalco } from '../studio/desenho'
import { Destino, Directo, directoSuportado, EstadoDoDirecto } from '../studio/directo'
import EditPanel, { Gravado } from '../studio/EditPanel'
import { cortar, cortarVarios, cortesSuportados } from '../studio/editor'
import LayoutsPanel from '../studio/LayoutsPanel'
import LivePanel from '../studio/LivePanel'
import LocalPanel from '../studio/LocalPanel'
import { eh4k, plataformaDoUrl, rotuloDaQualidade } from '../studio/palco'
import QuadroLocal from '../studio/QuadroLocal'
import RegionPicker from '../studio/RegionPicker'
import Relogio from '../studio/Relogio'
import SobreposicoesPanel from '../studio/SobreposicoesPanel'
import SourcesPanel from '../studio/SourcesPanel'
import { useLegendas } from '../studio/useLegendas'
import { usePalco } from '../studio/usePalco'
import '../ui/studio.css'
import '../ui/studio-palco.css'

/**
 * A sala (convidados, chat, perguntas, sondagens) entra por `lazy`, e só
 * quando se abre: é o único sítio do Estúdio que fala `signaling`/`webrtc`, e
 * uma aula gravada offline não pode arrastar esse caminho — nem para o
 * precache do PWA (ver `studio.invariantes.test.ts`).
 */
const SalaDoEstudio = lazy(() => import('./studio/SalaDoEstudio'))

/** O ecrã é de telemóvel: a emissão arruma-se para o polegar e a sala abre no chat. */
const ecraDeTelemovel = () => typeof window !== 'undefined' && window.matchMedia?.('(max-width: 720px)').matches

/**
 * O tecto por omissão do servidor (`MAX_DESTINOS_POR_DIRECTO` em
 * `broadcast.rs`). O servidor continua a ser quem decide; isto só evita mandar
 * um pedido que se sabe à partida que vai ser recusado.
 */
const MAX_DESTINOS = 4

type Vista = 'emissao' | 'edicao'

export default function Studio() {
  const { t } = useTranslation()
  const { navOpen, setNavOpen, org } = useShell()
  const compRef = useRef<CompositorDeAula | null>(null)
  const canvasHostRef = useRef<HTMLDivElement>(null)
  const efeitoRef = useRef<BackgroundEffect | null>(null)
  const previewRef = useRef<HTMLVideoElement>(null)
  // O directo é imperativo e vive num ref: pô-lo em estado faria a página
  // re-renderizar a cada pedaço enviado.
  const directoRef = useRef<Directo | null>(null)

  const [vista, setVista] = useState<Vista>('emissao')
  const [pronto, setPronto] = useState(false)
  const [resolucao, setResolucao] = useState('')
  const [temEcra, setTemEcra] = useState(false)
  const [temCamara, setTemCamara] = useState(false)
  const [avatar, setAvatar] = useState<EstadoDoAvatar>({ ...AVATAR_INICIAL })
  const [recorte, setRecorte] = useState<Recorte>({ ...RECORTE_INTEIRO })
  const [aRecortar, setARecortar] = useState(false)
  const [aPrepararRecorte, setAPrepararRecorte] = useState(false)
  const [estado, setEstado] = useState<'parado' | 'a-gravar' | 'pausa'>('parado')
  const [erro, setErro] = useState('')

  const [resultado, setResultado] = useState<Gravado | null>(null)
  const [de, setDe] = useState(0)
  const [ate, setAte] = useState(0)
  const [aCortar, setACortar] = useState(0)
  const [analise, setAnalise] = useState<AnaliseDeAudio | null>(null)
  const [aAnalisar, setAAnalisar] = useState(false)
  const [titulo, setTitulo] = useState('')
  const [aGuardar, setAGuardar] = useState(false)
  const [guardado, setGuardado] = useState('')

  const [online, setOnline] = useState(() => navigator.onLine)
  const [porEnviar, setPorEnviar] = useState<arquivo.AulaGuardada[]>([])
  const [ocupacao, setOcupacao] = useState(0)

  const [directo, setDirecto] = useState<EstadoDoDirecto>({ fase: 'parado' })
  const kbps = useDebito(directo)
  // A sala ligada ao Estúdio: pedida pela pessoa, carregada por `lazy`.
  const [salaAberta, setSalaAberta] = useState(false)
  const [salaLigada, setSalaLigada] = useState<{ codigo: string; token: string } | null>(null)
  const [haSondagem, setHaSondagem] = useState(false)
  const [convidadosNoPalco, setConvidadosNoPalco] = useState(0)
  const [destinos, setDestinos] = useState<Destino[]>([
    { url: 'rtmp://a.rtmp.youtube.com/live2', chave: '', rotulo: 'YouTube' },
  ])

  // O compositor é um objecto imperativo com um canvas: monta uma vez e o
  // React só lhe dá ordens. Pô-lo em estado faria a árvore re-renderizar a
  // cada frame. O canvas vive num nó onde o React não põe filhos.
  useEffect(() => {
    const c = new CompositorDeAula()
    compRef.current = c
    c.aoPerderEcra = () => {
      setTemEcra(false)
      setErro(t('studio.erros.ecraTerminado'))
    }
    c.canvas.className = 'st-canvas'
    c.canvas.setAttribute('data-studio', 'canvas')
    canvasHostRef.current?.appendChild(c.canvas)
    c.iniciarPreVisualizacao()
    setResolucao(`${c.canvas.width}×${c.canvas.height}`)
    setPronto(true)
    return () => {
      efeitoRef.current?.stop()
      efeitoRef.current = null
      const d = directoRef.current
      directoRef.current = null
      void d?.parar()
      c.destruir()
      c.canvas.remove()
      compRef.current = null
    }
  }, [t])

  const aGravarOuPausa = estado !== 'parado'
  const palco = usePalco({
    compRef,
    pronto,
    bloqueado: aGravarOuPausa || directo.fase === 'no-ar' || directo.fase === 'a-ligar',
    titulo,
    organizacao: org?.name ?? '',
    avatar,
    aplicarAvatar: setAvatar,
  })
  const estadoLegendas = useLegendas(
    compRef,
    palco.sobreposicoes.legendas,
    aGravarOuPausa || directo.fase === 'no-ar',
  )
  // A resolução mostrada segue o canvas REAL (a qualidade pode ter sido recusada).
  useEffect(() => {
    const c = compRef.current
    if (c) setResolucao(`${c.canvas.width}×${c.canvas.height}`)
  }, [pronto, palco.qualidade])

  const aoPalco = useCallback((fontes: Fonte[]) => {
    compRef.current?.definirConvidados(fontes)
    setConvidadosNoPalco(fontes.length)
  }, [])
  const aSondagem = useCallback((s: SondagemNoPalco | null) => {
    if (compRef.current) compRef.current.sondagem = s
    setHaSondagem(!!s)
  }, [])
  const obterCamara = useCallback(() => compRef.current?.trackDaCamara ?? null, [])

  // Estado da rede: o aviso «sem rede» e o esvaziar da fila dependem disto.
  useEffect(() => {
    const sobe = () => setOnline(true)
    const desce = () => setOnline(false)
    window.addEventListener('online', sobe)
    window.addEventListener('offline', desce)
    return () => {
      window.removeEventListener('online', sobe)
      window.removeEventListener('offline', desce)
    }
  }, [])

  const recarregarFila = useCallback(() => {
    arquivo
      .porEnviar()
      .then(setPorEnviar)
      .catch(() => setPorEnviar([]))
    arquivo
      .ocupacao()
      .then(setOcupacao)
      .catch(() => setOcupacao(0))
  }, [])
  useEffect(() => recarregarFila(), [recarregarFila])

  // Volta a rede → tenta esvaziar a fila. Sem isto o utilizador teria de se
  // lembrar de vir cá carregar num botão, e a promessa de «grava offline»
  // ficava a meio.
  useEffect(() => {
    if (!online || !porEnviar.length) return
    void enviarFila()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [online, porEnviar.length])

  // O compositor lê estes campos a cada frame — basta escrevê-los.
  useEffect(() => {
    if (compRef.current) compRef.current.avatar = avatar
  }, [avatar])
  useEffect(() => {
    if (compRef.current) compRef.current.recorte = recorte
  }, [recorte])

  const lerSegundos = useCallback(() => compRef.current?.segundos ?? 0, [])
  const lerBytes = useCallback(() => compRef.current?.bytesGravados ?? 0, [])
  const mudarAvatar = useCallback((patch: Partial<EstadoDoAvatar>) => setAvatar((a) => ({ ...a, ...patch })), [])
  const fecharRecorte = useCallback(() => setARecortar(false), [])

  const escolherEcra = useCallback(async () => {
    setErro('')
    try {
      await compRef.current?.escolherEcra()
      setTemEcra(true)
      setRecorte({ ...RECORTE_INTEIRO })
    } catch (e) {
      // Cancelar o seletor do browser não é um erro — é uma decisão.
      if ((e as Error)?.name !== 'NotAllowedError') setErro(t('studio.erros.ecra'))
    }
  }, [t])

  async function alternarCamara() {
    setErro('')
    const c = compRef.current
    if (!c) return
    if (c.temCamara) {
      pararRecorteDeFundo()
      c.desligarCamara()
      setTemCamara(false)
      return
    }
    try {
      await c.ligarCamara()
      setTemCamara(true)
    } catch {
      setErro(t('studio.erros.camara'))
    }
  }

  /** Liga a segmentação e passa o canvas com alfa ao compositor. */
  async function ligarRecorteDeFundo() {
    const c = compRef.current
    if (!c || !c.temCamara || efeitoRef.current) return
    setAPrepararRecorte(true)
    setErro('')
    try {
      const track = c.trackDaCamara
      if (!track) throw new Error('sem câmara')
      const ef = new BackgroundEffect()
      await ef.start(track)
      efeitoRef.current = ef
      // Enquanto a segmentação não produzir o primeiro resultado,
      // `pessoaComAlfa` é null e o compositor cai na bolha — sem buraco no
      // ecrã à espera do modelo.
      const alimentar = () => {
        if (!efeitoRef.current || !compRef.current) return
        compRef.current.pessoaComAlfa = efeitoRef.current.pessoaComAlfa
        requestAnimationFrame(alimentar)
      }
      alimentar()
      setAvatar((a) => ({ ...a, modo: 'recorte' }))
    } catch {
      setErro(t('studio.erros.recorte'))
    } finally {
      setAPrepararRecorte(false)
    }
  }

  function pararRecorteDeFundo() {
    efeitoRef.current?.stop()
    efeitoRef.current = null
    if (compRef.current) compRef.current.pessoaComAlfa = null
    setAvatar((a) => ({ ...a, modo: 'bolha' }))
  }

  /**
   * Arrastar a bolha para qualquer sítio do palco (os quatro cantos não
   * chegam quando o conteúdo do slide está justamente no canto). Guarda
   * FRACÇÕES do canvas e passa o canto a `livre`.
   */
  function arrastarBolha(e: ReactPointerEvent<HTMLDivElement>) {
    if (!temCamara || aRecortar) return
    // No quadro, arrastar é escrever — a bolha move-se pelos cantos.
    if (palco.conteudo === 'quadro') return
    if ((e.target as HTMLElement).closest('button, input, a')) return
    const alvo = e.currentTarget
    const cv = alvo.querySelector('canvas')
    if (!cv) return
    alvo.setPointerCapture(e.pointerId)
    const mover = (ev: PointerEvent) => {
      const r = cv.getBoundingClientRect()
      setAvatar((a) => ({
        ...a,
        canto: 'livre',
        x: Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width)),
        y: Math.min(1, Math.max(0, (ev.clientY - r.top) / r.height)),
      }))
    }
    mover(e.nativeEvent)
    const largar = () => {
      alvo.removeEventListener('pointermove', mover)
      alvo.removeEventListener('pointerup', largar)
      alvo.removeEventListener('pointercancel', largar)
    }
    alvo.addEventListener('pointermove', mover)
    alvo.addEventListener('pointerup', largar)
    alvo.addEventListener('pointercancel', largar)
  }

  async function gravar() {
    setErro('')
    setGuardado('')
    try {
      await compRef.current?.iniciarGravacao()
      setEstado('a-gravar')
      // Grava-se a olhar para o palco, não para a edição da gravação anterior.
      setVista('emissao')
    } catch {
      setErro(t('studio.erros.gravar'))
    }
  }

  function pausarOuRetomar() {
    if (estado === 'a-gravar') {
      compRef.current?.pausar()
      setEstado('pausa')
    } else {
      compRef.current?.retomar()
      setEstado('a-gravar')
    }
  }

  async function parar() {
    const faixas = await compRef.current?.terminarGravacao()
    setEstado('parado')
    if (!faixas) {
      setErro(t('studio.vazia'))
      return
    }
    if (resultado) URL.revokeObjectURL(resultado.url)
    setResultado({ faixas, url: URL.createObjectURL(faixas.completo), duracao: 0 })
    setAnalise(null)
    setDe(0)
    setAte(0)
    setVista('edicao')
  }

  const mudarDuracao = useCallback((d: number) => {
    setResultado((r) => (r && r.duracao !== d ? { ...r, duracao: d } : r))
    setAte((a) => (a > 0 ? a : d))
  }, [])

  function mudarDe(v: number) {
    setDe(v)
    if (v >= ate) setAte(Math.min(resultado?.duracao ?? v + 1, v + 1))
  }

  function mudarAte(v: number) {
    setAte(v)
    if (v <= de) setDe(Math.max(0, v - 1))
  }

  function nomeBase() {
    return (titulo.trim() || t('studio.semTitulo')).replace(/[^\w.-]+/g, '-')
  }

  function descarregar(qual: 'completo' | 'video' | 'audio') {
    if (!resultado) return
    const b = resultado.faixas[qual]
    if (!b) return
    const url = qual === 'completo' ? resultado.url : URL.createObjectURL(b)
    const a = document.createElement('a')
    a.href = url
    const sufixo = qual === 'completo' ? '' : `-${qual}`
    a.download = `${nomeBase()}${sufixo}.${qual === 'audio' ? 'weba' : 'webm'}`
    a.click()
    if (qual !== 'completo') setTimeout(() => URL.revokeObjectURL(url), 10_000)
  }

  /**
   * Procura as pausas mortas. É análise de SINAL, não um modelo: corre em
   * milissegundos e funciona no primeiro arranque, offline.
   */
  async function procurarPausas() {
    const faixa = resultado?.faixas.audio
    if (!faixa) {
      setErro(t('studio.erros.semAudio'))
      return
    }
    setErro('')
    setAAnalisar(true)
    try {
      setAnalise(await analisarPausas(faixa))
    } catch {
      setErro(t('studio.erros.analise'))
    } finally {
      setAAnalisar(false)
    }
  }

  /** Substitui o resultado pelo cortado — o que se guarda é o que se vê. */
  function substituir(novo: Blob, duracao: number) {
    if (!resultado) return
    URL.revokeObjectURL(resultado.url)
    setResultado({
      faixas: { completo: novo, video: null, audio: resultado.faixas.audio },
      url: URL.createObjectURL(novo),
      duracao,
    })
    setAnalise(null)
    setDe(0)
    setAte(0)
  }

  async function removerPausas() {
    if (!resultado || !analise) return
    const trocos = trocosSemPausas(analise)
    if (!trocos.length) {
      setErro(t('studio.erros.tudoPausa'))
      return
    }
    setErro('')
    setACortar(0.001)
    try {
      const novo = await cortarVarios(
        resultado.faixas.completo,
        trocos,
        (pr) => setACortar(Math.max(0.001, pr.fraccao ?? 0.001)),
        4,
        resultado.faixas.audio,
      )
      substituir(novo, trocos.reduce((a, tr) => a + (tr.fim - tr.inicio), 0))
    } catch (e) {
      setErro(apiErrorMessage(e, t('studio.erros.corte')))
    } finally {
      setACortar(0)
    }
  }

  async function aplicarCorte() {
    if (!resultado) return
    setErro('')
    setACortar(0.001)
    try {
      const novo = await cortar(
        resultado.faixas.completo,
        { inicio: de, fim: ate },
        (p) => setACortar(Math.max(0.001, p.fraccao ?? 0.001)),
        4,
        resultado.faixas.audio,
      )
      substituir(novo, ate - de)
    } catch (e) {
      setErro(apiErrorMessage(e, t('studio.erros.corte')))
    } finally {
      setACortar(0)
    }
  }

  /**
   * Vai para o ar. A emissão precisa de uma SALA, porque é a sala que o
   * servidor autentica e é nela que o registo por sala vive. Cria-se uma para
   * a sessão de directo — o mesmo padrão que guardar na biblioteca já usa.
   */
  async function irParaOAr() {
    const c = compRef.current
    if (!c) return
    setDirecto({ fase: 'a-ligar' })
    try {
      const fluxo = await c.montarFluxo()
      // Com uma sala ligada (convidados), emite-se por ELA; sem sala, cria-se
      // uma para a sessão de directo, como sempre.
      let codigo = salaLigada?.codigo ?? ''
      let token = salaLigada?.token ?? ''
      if (!codigo) {
        const sala = await createRoom(titulo.trim() || t('studio.semTitulo'), 'sfu', false, false, 'normal')
        codigo = sala.code
        token = (await joinRoom(sala.code)).room_token
      }
      const d = new Directo()
      d.aoMudar = setDirecto
      directoRef.current = d
      await d.comecar(fluxo, codigo, token, destinos.filter((dest) => dest.chave.trim()))
    } catch (e) {
      // A razão vem do servidor («esta sala tem cifra ponta-a-ponta…») e
      // mostra-se tal como foi escrita, no PAINEL, ao pé do botão que a causou.
      setDirecto({ fase: 'erro', motivo: apiErrorMessage(e, t('studio.directo.erro')) })
      directoRef.current = null
      await c.largarFluxo()
    }
  }

  async function sairDoAr() {
    const d = directoRef.current
    directoRef.current = null
    await d?.parar()
    await compRef.current?.largarFluxo()
    setDirecto({ fase: 'parado' })
  }

  /**
   * Envia uma aula do arquivo local para a biblioteca do servidor. As
   * gravações pertencem a uma sala no modelo de dados, por isso a aula ganha
   * uma — reutilizar o que existe em vez de abrir um segundo caminho de upload.
   */
  async function enviarUma(aula: arquivo.AulaGuardada): Promise<void> {
    if (!aula.completo) return
    const sala = await createRoom(aula.titulo, 'sfu', false, false, 'normal')
    await uploadRecording(sala.code, aula.completo, `${aula.titulo}.webm`)
    await arquivo.marcarEnviada(aula.id)
  }

  async function enviarFila(): Promise<void> {
    const fila = await arquivo.porEnviar().catch(() => [] as arquivo.AulaGuardada[])
    for (const aula of fila) {
      try {
        await enviarUma(aula)
      } catch (e) {
        // Uma falha não pode parar a fila: a aula seguinte pode passar, e esta
        // fica com a razão escrita para a interface a mostrar.
        await arquivo.marcarErro(aula.id, apiErrorMessage(e, t('studio.erros.guardar'))).catch(() => undefined)
      }
    }
    recarregarFila()
  }

  /**
   * Guardar. GRAVA SEMPRE NO DISPOSITIVO PRIMEIRO e só depois tenta o servidor.
   *
   * A ordem não é um detalhe: fazer o upload primeiro e guardar localmente só
   * em caso de falha perde a aula quando o upload rebenta a meio — que é
   * precisamente quando ela é mais precisa.
   */
  async function guardarNaBiblioteca() {
    if (!resultado) return
    setAGuardar(true)
    setErro('')
    setGuardado('')
    const nome = titulo.trim() || t('studio.semTitulo')
    try {
      const id = await arquivo.guardar({
        titulo: nome,
        criadaEm: Date.now(),
        duracao: resultado.duracao,
        completo: resultado.faixas.completo,
        audio: resultado.faixas.audio,
        enviada: false,
      })
      if (!navigator.onLine) {
        setGuardado(t('studio.guardadoLocal'))
        recarregarFila()
        return
      }
      try {
        const sala = await createRoom(nome, 'sfu', false, false, 'normal')
        await uploadRecording(sala.code, resultado.faixas.completo, `${nome}.webm`)
        await arquivo.marcarEnviada(id)
        setGuardado(t('studio.guardado'))
      } catch (e) {
        // O servidor falhou mas a aula ESTÁ salva. Isto não é um erro para o
        // utilizador — é um adiamento, e a mensagem tem de o dizer assim.
        await arquivo.marcarErro(id, apiErrorMessage(e, t('studio.erros.guardar')))
        setGuardado(t('studio.guardadoLocal'))
      }
      recarregarFila()
    } catch (e) {
      setErro(apiErrorMessage(e, t('studio.erros.guardar')))
    } finally {
      setAGuardar(false)
    }
  }

  const aGravar = estado === 'a-gravar'
  const emPausa = estado === 'pausa'
  const noAr = directo.fase === 'no-ar'
  // Há imagem para gravar: uma fonte, o quadro, o cartão de intervalo ou um convidado.
  const temFonte = temEcra || temCamara || palco.conteudo !== 'fontes' || convidadosNoPalco > 0
  const arrastavel = temCamara && !aRecortar && palco.conteudo !== 'quadro'
  const noArDesde = directo.fase === 'no-ar' ? directo.desde : 0
  const lerNoAr = useCallback(() => (noArDesde ? (Date.now() - noArDesde) / 1000 : 0), [noArDesde])
  const destinosComChave = destinos.filter((d) => d.chave.trim())
  const rotuloQualidade = rotuloDaQualidade(palco.qualidade)

  return (
    <div className="dx-stage st">
      <header className="st-top">
        <IconButton
          icon="menu"
          bare
          className="st-top__burger"
          label={t('shell.abrirNavegacao')}
          aria-expanded={navOpen}
          aria-controls="shell-nav"
          onClick={() => setNavOpen(!navOpen)}
        />
        <BrandMark size={22} />
        <h1 className="st-top__title">
          {t('studio.titulo')}
          {titulo.trim() && <span className="st-top__sub"> · {titulo.trim()}</span>}
        </h1>
        {(aGravar || emPausa) && (
          <StatusBadge tone={aGravar ? 'record' : 'warning'}>
            {aGravar ? t('studio.topo.rec') : t('studio.topo.pausa')}
            {eh4k(palco.qualidade) && ' 4K'}{' '}
            <Cronometro
              activo={aGravar}
              ler={lerSegundos}
              className="dx-num"
              label={t('studio.topo.cronometro')}
              data-studio="tempo"
            />
          </StatusBadge>
        )}
        {noAr && (
          <StatusBadge tone="live">
            {t('studio.topo.aoVivoDestinos', { count: destinosComChave.length })}
          </StatusBadge>
        )}

        <div className="dx-seg st-views" role="group" aria-label={t('studio.vistas.rotulo')}>
          <button type="button" aria-pressed={vista === 'emissao'} data-studio-vista="emissao" onClick={() => setVista('emissao')}>
            {t('studio.vistas.emissao')}
          </button>
          <button
            type="button"
            aria-pressed={vista === 'edicao'}
            data-studio-vista="edicao"
            disabled={!resultado}
            onClick={() => setVista('edicao')}
          >
            {t('studio.vistas.edicao')}
          </button>
        </div>

        <span className="dx-spacer" />

        <span className="st-top__meta dx-num" data-studio="topo-meta">
          {noAr && <span>{t('studio.topo.enc', { kbps: kbps.toLocaleString() })} · </span>}
          <Relogio />
        </span>

        <div className="st-top__actions" data-studio="acoes">
          {!aGravar && !emPausa ? (
            <Button
              variant="primary"
              icon="record"
              data-studio="gravar"
              disabled={!temFonte}
              title={temFonte ? undefined : t('studio.acoes.semFonte')}
              onClick={() => void gravar()}
            >
              {t('studio.acoes.gravar')}
            </Button>
          ) : (
            <>
              <Button variant="secondary" icon={aGravar ? 'pause' : 'play'} data-studio="pausa" onClick={pausarOuRetomar}>
                {aGravar ? t('studio.acoes.pausar') : t('studio.acoes.retomar')}
              </Button>
              <Button variant="danger" icon="square" data-studio="parar" onClick={() => void parar()}>
                {t('studio.acoes.parar')}
              </Button>
            </>
          )}
        </div>
      </header>

      <div className="st-notices">
        <p className="st-narrow-note">{t('studio.ecraEstreito')}</p>
        {!online && (
          <div className="dx-alert dx-alert--warning" role="status" data-studio="offline">
            {t('studio.offline')}
          </div>
        )}
        {erro && <Alert tone="danger">{erro}</Alert>}
      </div>

      <div className="st-body" hidden={vista !== 'emissao'}>
        <aside className="st-col st-col--left">
          <LayoutsPanel
            layout={palco.layout}
            conteudo={palco.conteudo}
            qualidade={palco.qualidade}
            qualidadeBloqueada={aGravar || emPausa || noAr || directo.fase === 'a-ligar'}
            onLayout={palco.escolherLayout}
            onConteudo={palco.escolherConteudo}
            onQualidade={palco.escolherQualidade}
          />
          <CenasPanel
            cenas={palco.cenas}
            activa={palco.cenaActiva}
            indisponivel={palco.bancoIndisponivel}
            onAplicar={palco.aplicarCena}
            onNova={palco.novaCena}
            onApagar={(id) => void palco.apagarCena(id)}
          />
          <SourcesPanel
            temEcra={temEcra}
            temCamara={temCamara}
            recorte={recorte}
            avatar={avatar}
            aPrepararRecorte={aPrepararRecorte}
            onEscolherEcra={() => void escolherEcra()}
            onEcraInteiro={() => {
              setRecorte({ ...RECORTE_INTEIRO })
              setARecortar(false)
            }}
            onAbrirRegiao={() => setARecortar(true)}
            onAlternarCamara={() => void alternarCamara()}
            onAvatar={mudarAvatar}
            onFundo={(semFundo) => (semFundo ? void ligarRecorteDeFundo() : pararRecorteDeFundo())}
          />
          <AudioPanel
            mistura={palco.mistura}
            microfones={palco.microfones}
            microfone={palco.microfone}
            musica={palco.musica}
            musicaATocar={palco.musicaATocar}
            onMistura={palco.mudarMistura}
            onMicrofone={palco.escolherMicrofone}
            onMusica={palco.carregarMusica}
            onAlternarMusica={palco.alternarMusica}
          />
        </aside>

        <section className="st-centre" aria-label={t('studio.palco.rotulo')}>
          {/* O estado do palco fica POR CIMA da imagem e não sobre ela: o
              logótipo e o cronómetro queimados vivem nos cantos do programa, e
              uma ficha da interface por cima deles escondia o que vai para o ar. */}
          <div className="st-stage-meta">
            <span className="dx-num st-small dx-muted">
              {t('studio.palco.previsualizacao')} · {resolucao}
            </span>
            <span className="dx-spacer" />
            {noAr && (
              <StatusBadge tone="live">
                {t('studio.palco.noAr')} · <Cronometro activo ler={lerNoAr} className="dx-num" data-studio="no-ar-tempo" />
              </StatusBadge>
            )}
            {aGravar && <StatusBadge tone="record">{t('studio.topo.rec')}</StatusBadge>}
            <span className="st-overlay__chip dx-num" data-studio="palco-qualidade">
              {rotuloQualidade}
            </span>
          </div>
          <div className="st-stage-fit">
            <div
              className={cx(
                'st-stage',
                arrastavel && 'st-stage--drag',
                noAr && 'st-stage--live',
                (aGravar || emPausa) && !noAr && 'st-stage--rec',
              )}
              title={arrastavel ? t('studio.palco.arrastavel') : undefined}
              onPointerDown={arrastarBolha}
            >
              <div ref={canvasHostRef} className="st-stage__canvas" />
              {palco.conteudo === 'quadro' && (
                <QuadroLocal
                  onRiscar={(de, ate, cor, esp) => compRef.current?.riscarNoQuadro(de, ate, cor, esp)}
                  onLimpar={() => compRef.current?.limparQuadro()}
                />
              )}
              {!temFonte && pronto && (
                <div className="st-stage__empty">
                  <p>{t('studio.palco.vazio')}</p>
                  <div className="st-actions st-actions--center">
                    <Button variant="primary" icon="screen" onClick={() => void escolherEcra()}>
                      {t('studio.fonte.escolherEcra')}
                    </Button>
                    <Button variant="secondary" icon="video" onClick={() => void alternarCamara()}>
                      {t('studio.imagem.ligar')}
                    </Button>
                  </div>
                </div>
              )}
              {temEcra && aRecortar && (
                <RegionPicker
                  onFechar={fecharRecorte}
                  onAplicar={(r) => {
                    setRecorte(r)
                    setARecortar(false)
                  }}
                />
              )}
            </div>
          </div>

          {/* Telemóvel: os destinos em fichas por baixo do palco (etiqueta e
              fase — a audiência por plataforma não existe no servidor). */}
          {destinosComChave.length > 0 && (
            <ul className="st-chips" aria-label={t('studio.directo.titulo')}>
              {destinosComChave.map((d, i) => (
                <li key={i} className={cx('st-chip', noAr && 'is-live')}>
                  <span className="dx-num">{plataformaDoUrl(d.url, location.host)}</span>
                  <span className="st-chip__state">
                    {noAr ? t('studio.directo.estados.noAr') : t('studio.directo.estados.pronto')}
                  </span>
                </li>
              ))}
            </ul>
          )}

          <div className="st-under">
            {salaAberta ? (
              <Suspense fallback={<Spinner label={t('studio.sala.aCarregar')} />}>
                <SalaDoEstudio
                  titulo={titulo}
                  micId={palco.microfone}
                  obterCamara={obterCamara}
                  onLigacao={setSalaLigada}
                  onPalco={aoPalco}
                  onSondagem={aSondagem}
                  separadorInicial={ecraDeTelemovel() ? 'chat' : 'convidados'}
                />
              </Suspense>
            ) : (
              <section className="st-group st-panel" data-studio="sala-fechada" aria-labelledby="st-sala-fechada-h">
                <h2 id="st-sala-fechada-h" className="st-group__title">
                  {t('studio.sala.fila')}
                </h2>
                <p className="st-note">{t('studio.sala.explicacao')}</p>
                <div className="st-actions">
                  <Button size="sm" variant="secondary" icon="userPlus" data-studio="sala-abrir" disabled={!online} onClick={() => setSalaAberta(true)}>
                    {t('studio.sala.abrir')}
                  </Button>
                </div>
                {!online && <p className="st-note">{t('studio.sala.semRede')}</p>}
              </section>
            )}
            <SobreposicoesPanel
              valor={palco.sobreposicoes}
              temLogotipo={palco.temLogotipo}
              temSondagem={haSondagem}
              legendas={
                estadoLegendas === 'a-preparar'
                  ? t('studio.legendas.aPreparar')
                  : estadoLegendas === 'sem-modelo'
                    ? t('studio.legendas.semModelo')
                    : estadoLegendas === 'activas'
                      ? t('studio.legendas.activas')
                      : ''
              }
              onMudar={palco.mudarSobreposicoes}
              onLogotipo={(f) => void palco.carregarLogotipo(f)}
            />
          </div>
        </section>

        <aside className="st-col st-col--right">
          <LivePanel
            suportado={directoSuportado()}
            destinos={destinos}
            maximo={MAX_DESTINOS}
            estado={directo}
            podeEmitir={temFonte}
            onMudar={(i, patch) => setDestinos((ds) => ds.map((d, j) => (j === i ? { ...d, ...patch } : d)))}
            onAdicionar={() =>
              setDestinos((ds) => (ds.length >= MAX_DESTINOS ? ds : [...ds, { url: '', chave: '', rotulo: '' }]))
            }
            onRemover={(i) => setDestinos((ds) => ds.filter((_, j) => j !== i))}
            onIrParaOAr={() => void irParaOAr()}
            onParar={() => void sairDoAr()}
          />
          <LocalPanel
            estado={estado}
            lerSegundos={lerSegundos}
            porEnviar={porEnviar}
            ocupacaoBytes={ocupacao}
            online={online}
            resolucao={resolucao}
            qualidade={rotuloQualidade}
            lerBytes={lerBytes}
            onEnviar={() => void enviarFila()}
          />
        </aside>

        {/* Alcance do polegar: no telemóvel, as acções da emissão ficam em baixo. */}
        <div className="st-thumb" data-studio="barra-polegar">
          {noAr ? (
            <Button variant="live" size="lg" block icon="x" onClick={() => void sairDoAr()}>
              {t('studio.directo.parar')}
            </Button>
          ) : (
            <Button
              variant="live"
              size="lg"
              block
              icon="live"
              busy={directo.fase === 'a-ligar'}
              disabled={directo.fase === 'a-ligar' || destinosComChave.length === 0 || !temFonte || !directoSuportado()}
              onClick={() => void irParaOAr()}
            >
              {t('studio.directo.irParaOAr')}
            </Button>
          )}
        </div>
      </div>

      <div className="st-body st-body--edit" hidden={vista !== 'edicao'}>
        <EditPanel
          resultado={resultado}
          previewRef={previewRef}
          onDuracao={mudarDuracao}
          podeCortar={cortesSuportados()}
          de={de}
          ate={ate}
          onDe={mudarDe}
          onAte={mudarAte}
          aCortar={aCortar}
          onCortar={() => void aplicarCorte()}
          analise={analise}
          aAnalisar={aAnalisar}
          onProcurarPausas={() => void procurarPausas()}
          onRemoverPausas={() => void removerPausas()}
          onCancelarPausas={() => setAnalise(null)}
          onDescarregar={descarregar}
          titulo={titulo}
          onTitulo={setTitulo}
          aGuardar={aGuardar}
          onGuardar={() => void guardarNaBiblioteca()}
          guardado={guardado}
        />
      </div>
    </div>
  )
}
