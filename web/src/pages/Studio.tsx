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
import * as arquivo from '../studio/arquivo'
import AudioPanel from '../studio/AudioPanel'
import CenasPanel from '../studio/CenasPanel'
import { AVATAR_INICIAL, CompositorDeAula, EstadoDoAvatar, Recorte, RECORTE_INTEIRO } from '../studio/compositor'
import Cronometro from '../studio/Cronometro'
import { useDebito } from '../studio/debito'
import type { SondagemNoPalco } from '../studio/desenho'
import { Destino, Directo, directoSuportado, EstadoDoDirecto } from '../studio/directo'
import EditPanel, { Gravado, VistaDoEditor } from '../studio/EditPanel'
import { cortesSuportados } from '../studio/editor'
import LayoutsPanel from '../studio/LayoutsPanel'
import LivePanel from '../studio/LivePanel'
import LocalPanel from '../studio/LocalPanel'
import { eh4k, plataformaDoUrl, type Qualidade, QUALIDADES, rotuloDaQualidade } from '../studio/palco'
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

type Vista = 'emissao' | VistaDoEditor

/** A vista vem do endereço (`#/studio?vista=legendas`), para se poder voltar a ela. */
function vistaDoEndereco(): Vista {
  const v = new URLSearchParams(location.hash.split('?')[1] ?? '').get('vista')
  return v === 'edicao' || v === 'legendas' || v === 'exportacoes' ? v : 'emissao'
}

export default function Studio() {
  const { t, i18n } = useTranslation()
  const { navOpen, setNavOpen, org } = useShell()
  const compRef = useRef<CompositorDeAula | null>(null)
  const canvasHostRef = useRef<HTMLDivElement>(null)
  const efeitoRef = useRef<BackgroundEffect | null>(null)
  // O directo é imperativo e vive num ref: pô-lo em estado faria a página
  // re-renderizar a cada pedaço enviado.
  const directoRef = useRef<Directo | null>(null)

  const [vista, setVistaEstado] = useState<Vista>(vistaDoEndereco)
  const setVista = useCallback((v: Vista) => {
    setVistaEstado(v)
    const alvo = v === 'emissao' ? '#/studio' : `#/studio?vista=${v}`
    if (location.hash !== alvo) history.replaceState(null, '', alvo)
  }, [])
  useEffect(() => {
    const seguir = () => setVistaEstado(vistaDoEndereco())
    window.addEventListener('hashchange', seguir)
    return () => window.removeEventListener('hashchange', seguir)
  }, [])
  const [pronto, setPronto] = useState(false)
  const [temEcra, setTemEcra] = useState(false)
  const [temCamara, setTemCamara] = useState(false)
  const [avatar, setAvatar] = useState<EstadoDoAvatar>({ ...AVATAR_INICIAL })
  const [recorte, setRecorte] = useState<Recorte>({ ...RECORTE_INTEIRO })
  const [aRecortar, setARecortar] = useState(false)
  const [aPrepararRecorte, setAPrepararRecorte] = useState(false)
  const [estado, setEstado] = useState<'parado' | 'a-gravar' | 'pausa'>('parado')
  const [erro, setErro] = useState('')

  const [resultado, setResultado] = useState<Gravado | null>(null)
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
    // A gravação passa ao editor, que a transforma num projecto não destrutivo
    // (as faixas são fontes; cortar é uma edição, não um ficheiro novo).
    setResultado({ faixas, url: URL.createObjectURL(faixas.completo), duracao: 0 })
    setVista('edicao')
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
  async function guardarNaBiblioteca(exportado: Blob, duracao: number, tituloDoProjecto: string) {
    setAGuardar(true)
    setErro('')
    setGuardado('')
    const nome = tituloDoProjecto.trim() || titulo.trim() || t('studio.semTitulo')
    try {
      const id = await arquivo.guardar({
        titulo: nome,
        criadaEm: Date.now(),
        duracao,
        completo: exportado,
        audio: null,
        enviada: false,
      })
      if (!navigator.onLine) {
        setGuardado(t('studio.guardadoLocal'))
        recarregarFila()
        return
      }
      try {
        const sala = await createRoom(nome, 'sfu', false, false, 'normal')
        await uploadRecording(sala.code, exportado, `${nome}.webm`)
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

  const quadroOuAr = noAr || aGravar || emPausa
  const podeIrParaOAr = directo.fase !== 'a-ligar' && !noAr && destinosComChave.length > 0 && temFonte && directoSuportado()

  return (
    <>
    {vista !== 'emissao' && (
      <EditPanel
        vista={vista}
        onVista={setVista}
        resultado={resultado}
        podeCortar={cortesSuportados()}
        titulo={titulo}
        onTitulo={setTitulo}
        aGuardar={aGuardar}
        guardado={guardado}
        onGuardar={guardarNaBiblioteca}
      />
    )}
    {/* A emissão fica montada (escondida) enquanto se edita: o compositor e o
        canvas vivem nela, e desmontá-la perdia o palco a meio de um directo. */}
    <div className="dx-stage st" hidden={vista !== 'emissao'}>
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
        <span className="st-top__mark">
          <BrandMark size={22} tone="tile" />
        </span>
        <h1 className="st-top__title">
          {t('studio.titulo')}
          {titulo.trim() && <span> · {titulo.trim()}</span>}
        </h1>
        {quadroOuAr && (
          <span className="st-top__clock dx-num">
            {aGravar || emPausa ? (
              <Cronometro activo={aGravar} ler={lerSegundos} label={t('studio.topo.cronometro')} data-studio="tempo" />
            ) : (
              <Cronometro activo ler={lerNoAr} label={t('studio.topo.tempoNoAr')} />
            )}
          </span>
        )}
        {(aGravar || emPausa) && (
          <StatusBadge tone={aGravar ? 'record' : 'warning'}>
            {aGravar ? t('studio.topo.rec') : t('studio.topo.pausa')}
            {eh4k(palco.qualidade) && ' 4K'}
          </StatusBadge>
        )}
        {noAr && (
          <StatusBadge tone="live">
            <span className="dx-badge__tri" aria-hidden="true" />
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
            onClick={() => setVista('edicao')}
          >
            {t('studio.vistas.edicao')}
          </button>
        </div>

        <span className="dx-spacer" />

        <span className="st-top__meta dx-num" data-studio="topo-meta">
          {noAr && <span>{t('studio.topo.enc', { kbps: kbps.toLocaleString(i18n.language) })} · </span>}
          <Relogio />
        </span>

        <div className="st-top__actions" data-studio="acoes">
          {!aGravar && !emPausa ? (
            <Button
              variant="outline"
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
          {/* «Emitir para todos»: vai para o ar em TODOS os destinos com chave
              de uma vez (é uma só ligação — ver LivePanel). */}
          <Button
            variant="live"
            className="st-top__emit"
            busy={directo.fase === 'a-ligar'}
            disabled={!podeIrParaOAr}
            title={noAr ? t('studio.directo.estados.noAr') : destinosComChave.length ? undefined : t('studio.topo.semChave')}
            onClick={() => void irParaOAr()}
          >
            {t('studio.topo.emitirParaTodos')}
          </Button>
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
          <LayoutsPanel layout={palco.layout} onLayout={palco.escolherLayout} />
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
              {/* Estado da INTERFACE sobre o programa, como no template. As
                  sobreposições queimadas evitam este canto (o cronómetro vai
                  ao centro), para nada do que vai para o ar ficar tapado. */}
              <span className="st-overlay st-overlay--tr" aria-hidden="true">
                {noAr && (
                  <span className="st-overlay__chip dx-num">
                    {t('studio.palco.noAr')} · <Cronometro activo ler={lerNoAr} data-studio="no-ar-tempo" />
                  </span>
                )}
                <span className="st-overlay__chip dx-num" data-studio="palco-qualidade">
                  {rotuloQualidade}
                </span>
              </span>
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
              fase — a audiência por plataforma não existe no servidor, por isso
              a ficha diz a fase e não um número inventado). */}
          <ul className="st-chips" aria-label={t('studio.directo.titulo')}>
            {destinos.map((d, i) => {
              const temChave = !!d.chave.trim()
              return (
                <li key={i} className={cx('st-chip', noAr && temChave && 'is-live', !temChave && 'is-off')}>
                  <span className="dx-num">{plataformaDoUrl(d.url, location.host)}</span>
                  <span className="st-chip__state dx-num">
                    {!temChave
                      ? t('studio.directo.estados.semChave')
                      : noAr
                        ? t('studio.directo.estados.noAr')
                        : t('studio.directo.estados.pronto')}
                  </span>
                </li>
              )
            })}
          </ul>

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
                <header className="st-group__head">
                  <h2 id="st-sala-fechada-h" className="st-group__title">
                    {t('studio.sala.fila')}
                  </h2>
                  <span className="dx-spacer" />
                  <span className="dx-num dx-muted st-small">{t('studio.sala.semSala')}</span>
                </header>
                <div className="st-guests st-guests--empty" aria-hidden="true">
                  <span />
                  <span />
                  <span />
                </div>
                <div className="st-guests__actions">
                  <Button size="sm" variant="primary" icon="userPlus" data-studio="sala-abrir" disabled={!online} onClick={() => setSalaAberta(true)}>
                    {t('studio.sala.abrir')}
                  </Button>
                  <span className="st-note">{online ? t('studio.sala.explicacaoCurta') : t('studio.sala.semRede')}</span>
                </div>
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
          >
            <LocalPanel
              estado={estado}
              lerSegundos={lerSegundos}
              porEnviar={porEnviar}
              ocupacaoBytes={ocupacao}
              online={online}
              resolucao={rotuloQualidade}
              qualidade={palco.qualidade}
              qualidades={(Object.keys(QUALIDADES) as Qualidade[]).map((q) => ({ valor: q, rotulo: rotuloDaQualidade(q) }))}
              qualidadeBloqueada={aGravar || emPausa || noAr || directo.fase === 'a-ligar'}
              onQualidade={(q) => palco.escolherQualidade(q as Qualidade)}
              lerBytes={lerBytes}
              onEnviar={() => void enviarFila()}
            />
          </LivePanel>
        </aside>

        {/* Alcance do polegar: no telemóvel, as acções da emissão ficam em baixo. */}
        <div className="st-thumb" data-studio="barra-polegar">
          {noAr ? (
            <Button variant="live" size="lg" block onClick={() => void sairDoAr()}>
              {t('studio.directo.parar')}
            </Button>
          ) : (
            <Button variant="live" size="lg" block busy={directo.fase === 'a-ligar'} disabled={!podeIrParaOAr} onClick={() => void irParaOAr()}>
              {t('studio.directo.irParaOAr')}
            </Button>
          )}
        </div>
      </div>
    </div>
    </>
  )
}
