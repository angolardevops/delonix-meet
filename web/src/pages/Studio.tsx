import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createRoom, uploadRecording } from '../api'
import { BackgroundEffect } from '../media'
import PageHeader from '../components/PageHeader'
import { Btn } from '../components/ui'
import { CamIcon, CamOffIcon, FilmIcon, RecordIcon, StopIcon } from '../icons'
import { cortar, cortarVarios, cortesSuportados } from '../studio/editor'
import { analisarPausas, AnaliseDeAudio, resumo, trocosSemPausas } from '../studio/analise'
import * as arquivo from '../studio/arquivo'
import {
  AVATAR_INICIAL,
  CantoDoAvatar,
  CompositorDeAula,
  EstadoDoAvatar,
  FormaDoAvatar,
  ModoDoAvatar,
  Recorte,
  RECORTE_INTEIRO,
  ResultadoDaGravacao,
} from '../studio/compositor'

/**
 * Estúdio — gravar uma vídeo-aula: ecrã (inteiro, janela, separador ou uma
 * REGIÃO dele) com a câmara por cima, numa bolha que se move.
 *
 * Vive fora da sala de propósito: não há SFU, não há pares, não há rede no
 * caminho da media. Uma aula grava-se sozinho, e prender isso ao caminho de
 * uma chamada seria pôr a falhar coisas que não têm de existir aqui.
 */

const CANTOS: { key: CantoDoAvatar; rotulo: string }[] = [
  { key: 'superior-esquerdo', rotulo: '↖' },
  { key: 'superior-direito', rotulo: '↗' },
  { key: 'inferior-esquerdo', rotulo: '↙' },
  { key: 'inferior-direito', rotulo: '↘' },
]

function mmss(s: number): string {
  const m = Math.floor(s / 60)
  const r = s % 60
  return `${String(m).padStart(2, '0')}:${String(r).padStart(2, '0')}`
}

export default function Studio() {
  const { t } = useTranslation()
  const compRef = useRef<CompositorDeAula | null>(null)
  const palcoRef = useRef<HTMLDivElement>(null)
  const [pronto, setPronto] = useState(false)
  const [temEcra, setTemEcra] = useState(false)
  const [temCamara, setTemCamara] = useState(false)
  const [avatar, setAvatar] = useState<EstadoDoAvatar>({ ...AVATAR_INICIAL })
  const [recorte, setRecorte] = useState<Recorte>({ ...RECORTE_INTEIRO })
  const [aRecortar, setARecortar] = useState(false)
  const [estado, setEstado] = useState<'parado' | 'a-gravar' | 'pausa'>('parado')
  const [segundos, setSegundos] = useState(0)
  const [erro, setErro] = useState('')
  const efeitoRef = useRef<BackgroundEffect | null>(null)
  const [aPrepararRecorte, setAPrepararRecorte] = useState(false)
  const [resultado, setResultado] = useState<{ faixas: ResultadoDaGravacao; url: string; duracao: number } | null>(null)
  // Corte: pontos de entrada e saída, em segundos do ficheiro gravado.
  const [de, setDe] = useState(0)
  const [ate, setAte] = useState(0)
  const [analise, setAnalise] = useState<AnaliseDeAudio | null>(null)
  const [aAnalisar, setAAnalisar] = useState(false)
  const [online, setOnline] = useState(() => navigator.onLine)
  const [porEnviar, setPorEnviar] = useState<arquivo.AulaGuardada[]>([])
  const [aCortar, setACortar] = useState(0)   // 0 = parado; senão fracção
  const previewRef = useRef<HTMLVideoElement>(null)
  const [titulo, setTitulo] = useState('')
  const [aGuardar, setAGuardar] = useState(false)
  const [guardado, setGuardado] = useState('')

  // O compositor é um objecto imperativo com um canvas: monta uma vez e o
  // React só lhe dá ordens. Pô-lo em estado faria a árvore re-renderizar a
  // cada frame, que é exactamente o que o lote 3 tirou da sala.
  useEffect(() => {
    const c = new CompositorDeAula()
    compRef.current = c
    c.aoPerderEcra = () => {
      setTemEcra(false)
      setErro(t('studio.ecraTerminado', 'A partilha de ecrã terminou.'))
    }
    palcoRef.current?.appendChild(c.canvas)
    c.canvas.className = 'studio-canvas'
    c.iniciarPreVisualizacao()
    setPronto(true)
    return () => {
      efeitoRef.current?.stop()
      efeitoRef.current = null
      c.destruir()
      compRef.current = null
    }
  }, [t])

  // Estado da rede. A app inteira muda de comportamento com isto, por isso
  // vive aqui e não numa árvore de contexto para uma coisa só.
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
    arquivo.porEnviar().then(setPorEnviar).catch(() => setPorEnviar([]))
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

  // Relógio da gravação — vive AQUI, no nó que o mostra, e não na página.
  useEffect(() => {
    if (estado !== 'a-gravar') return
    const id = setInterval(() => setSegundos(compRef.current?.segundos ?? 0), 1000)
    return () => clearInterval(id)
  }, [estado])

  // O compositor lê estes campos a cada frame — basta escrevê-los.
  useEffect(() => {
    if (compRef.current) compRef.current.avatar = avatar
  }, [avatar])
  useEffect(() => {
    if (compRef.current) compRef.current.recorte = recorte
  }, [recorte])

  const escolherEcra = useCallback(async () => {
    setErro('')
    try {
      await compRef.current?.escolherEcra()
      setTemEcra(true)
      setRecorte({ ...RECORTE_INTEIRO })
    } catch (e) {
      // Cancelar o seletor do browser não é um erro — é uma decisão.
      if ((e as Error)?.name !== 'NotAllowedError') {
        setErro(t('studio.erroEcra', 'Não foi possível capturar o ecrã.'))
      }
    }
  }, [t])

  const alternarCamara = useCallback(async () => {
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
      setErro(t('studio.erroCamara', 'Não foi possível aceder à câmara.'))
    }
  }, [t])

  /** Liga a segmentação e passa o canvas com alfa ao compositor. */
  async function ligarRecorteDeFundo() {
    const c = compRef.current
    if (!c || !c.temCamara) return
    setAPrepararRecorte(true)
    setErro('')
    try {
      const track = c.trackDaCamara
      if (!track) throw new Error('sem câmara')
      const ef = new BackgroundEffect()
      await ef.start(track)
      efeitoRef.current = ef
      // O compositor lê este campo a cada frame; enquanto a segmentação não
      // produzir o primeiro resultado, `pessoaComAlfa` é null e o desenho
      // cai na bolha — sem buraco no ecrã à espera do modelo.
      const alimentar = () => {
        if (!efeitoRef.current || !compRef.current) return
        compRef.current.pessoaComAlfa = efeitoRef.current.pessoaComAlfa
        requestAnimationFrame(alimentar)
      }
      alimentar()
      setAvatar((a) => ({ ...a, modo: 'recorte' }))
    } catch {
      setErro(t('studio.erroRecorte', 'Não foi possível preparar o recorte de fundo.'))
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

  async function gravar() {
    setErro('')
    setResultado(null)
    setGuardado('')
    try {
      await compRef.current?.iniciarGravacao()
      setEstado('a-gravar')
      setSegundos(0)
    } catch {
      setErro(t('studio.erroGravar', 'Não foi possível iniciar a gravação.'))
    }
  }

  async function parar() {
    const blob = await compRef.current?.terminarGravacao()
    setEstado('parado')
    setSegundos(0)
    if (!blob) {
      setErro(t('studio.vazia', 'A gravação saiu vazia — nada foi guardado.'))
      return
    }
    setResultado({ faixas: blob, url: URL.createObjectURL(blob.completo), duracao: 0 })
    setAnalise(null)
    setDe(0)
    setAte(0)
  }

  function nomeBase() {
    return (titulo.trim() || t('studio.semTitulo', 'aula')).replace(/[^\w.-]+/g, '-')
  }

  function descarregar(qual: 'completo' | 'video' | 'audio' = 'completo') {
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
   * Procura as pausas mortas da aula. É análise de SINAL, não um modelo: corre
   * em milissegundos e funciona no primeiro arranque, offline.
   */
  async function procurarPausas() {
    const faixa = resultado?.faixas.audio
    if (!faixa) {
      setErro(t('studio.semAudio', 'Esta gravação não tem faixa de áudio isolada.'))
      return
    }
    setErro('')
    setAAnalisar(true)
    try {
      setAnalise(await analisarPausas(faixa))
    } catch {
      setErro(t('studio.erroAnalise', 'Não foi possível analisar o áudio.'))
    } finally {
      setAAnalisar(false)
    }
  }

  /** Remove as pausas encontradas, juntando o que fica num só ficheiro. */
  async function apertarPausas() {
    if (!resultado || !analise) return
    const trocos = trocosSemPausas(analise)
    if (!trocos.length) {
      setErro(t('studio.tudoPausa', 'Não sobra nada depois de remover as pausas.'))
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
      URL.revokeObjectURL(resultado.url)
      setResultado({
        faixas: { completo: novo, video: null, audio: resultado.faixas.audio },
        url: URL.createObjectURL(novo),
        duracao: trocos.reduce((a, tr) => a + (tr.fim - tr.inicio), 0),
      })
      setAnalise(null)
      setDe(0)
      setAte(0)
    } catch (e) {
      setErro((e as Error).message || t('studio.erroCorte', 'Não foi possível cortar.'))
    } finally {
      setACortar(0)
    }
  }

  /** Aplica o corte e SUBSTITUI o resultado — o que se guarda é o cortado. */
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
      URL.revokeObjectURL(resultado.url)
      setResultado({
        faixas: { completo: novo, video: null, audio: resultado.faixas.audio },
        url: URL.createObjectURL(novo),
        duracao: ate - de,
      })
      setDe(0)
      setAte(0)
    } catch (e) {
      setErro((e as Error).message || t('studio.erroCorte', 'Não foi possível cortar.'))
    } finally {
      setACortar(0)
    }
  }

  /**
   * Guardar na biblioteca. As gravações pertencem a uma sala no modelo de
   * dados, por isso a aula ganha uma — com o título que o utilizador deu. É
   * reutilizar o que existe em vez de abrir um segundo caminho de upload.
   */
  /**
   * Envia uma aula do arquivo local para a biblioteca do servidor.
   * As gravações pertencem a uma sala no modelo de dados, por isso a aula ganha
   * uma — reutilizar o que existe em vez de abrir um segundo caminho de upload.
   */
  async function enviarUma(aula: arquivo.AulaGuardada): Promise<void> {
    if (!aula.completo) return
    const sala = await createRoom(aula.titulo, 'sfu', false, false, 'normal')
    await uploadRecording(sala.code, aula.completo, `${aula.titulo}.webm`)
    await arquivo.marcarEnviada(aula.id)
  }

  async function enviarFila(): Promise<void> {
    const fila = await arquivo.porEnviar()
    for (const aula of fila) {
      try {
        await enviarUma(aula)
      } catch (e) {
        // Uma falha não pode parar a fila: a aula seguinte pode passar, e esta
        // fica com a razão escrita para a interface a mostrar.
        await arquivo.marcarErro(aula.id, (e as Error).message ?? 'falhou')
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
    const nome = titulo.trim() || t('studio.semTitulo', 'Aula')
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
        setGuardado(t('studio.guardadoLocal', 'Guardada no dispositivo — envia sozinha quando houver rede.'))
        recarregarFila()
        return
      }
      try {
        const sala = await createRoom(nome, 'sfu', false, false, 'normal')
        await uploadRecording(sala.code, resultado.faixas.completo, `${nome}.webm`)
        await arquivo.marcarEnviada(id)
        setGuardado(t('studio.guardado', 'Guardada na biblioteca de Gravações.'))
      } catch (e) {
        // O servidor falhou mas a aula ESTÁ salva. Isto não é um erro para o
        // utilizador — é um adiamento, e a mensagem tem de o dizer assim.
        await arquivo.marcarErro(id, (e as Error).message ?? 'falhou')
        setGuardado(t('studio.guardadoLocal', 'Guardada no dispositivo — envia sozinha quando houver rede.'))
      }
      recarregarFila()
    } catch (e) {
      setErro((e as Error).message || t('studio.erroGuardar', 'Não foi possível guardar.'))
    } finally {
      setAGuardar(false)
    }
  }

  const aGravar = estado === 'a-gravar'
  const emPausa = estado === 'pausa'

  return (
    <div className="page studio">
      <PageHeader
        icon={<FilmIcon />}
        title={t('studio.titulo', 'Estúdio')}
        subtitle={t('studio.sub', 'Grava uma vídeo-aula: o teu ecrã, com a tua câmara por cima.')}
        actions={
          <div className="studio-acoes">
            {!aGravar && !emPausa ? (
              <Btn onClick={() => void gravar()} disabled={!temEcra && !temCamara}>
                <RecordIcon /> {t('studio.gravar', 'Gravar')}
              </Btn>
            ) : (
              <>
                <span className="studio-tempo mono" role="timer" aria-live="off">
                  ● {mmss(segundos)}
                </span>
                <Btn
                  variant="ghost"
                  onClick={() => {
                    if (aGravar) {
                      compRef.current?.pausar()
                      setEstado('pausa')
                    } else {
                      compRef.current?.retomar()
                      setEstado('a-gravar')
                    }
                  }}
                >
                  {aGravar ? t('studio.pausa', 'Pausa') : t('studio.retomar', 'Retomar')}
                </Btn>
                <Btn variant="danger" onClick={() => void parar()}>
                  <StopIcon /> {t('studio.parar', 'Parar')}
                </Btn>
              </>
            )}
          </div>
        }
      />

      {!online && (
        <p className="studio-offline" role="status">
          {t('studio.offline', 'Sem rede. O Estúdio grava, corta e guarda na mesma — as aulas ficam no dispositivo e sobem sozinhas quando a ligação voltar.')}
        </p>
      )}

      {porEnviar.length > 0 && (
        <div className="studio-fila" role="status">
          <span>
            {t('studio.fila', '{{n}} aula(s) por enviar', { n: porEnviar.length })}
            {porEnviar[0]?.erro ? ` · ${porEnviar[0].erro}` : ''}
          </span>
          <Btn variant="ghost" disabled={!online} onClick={() => void enviarFila()}>
            {t('studio.enviarAgora', 'Enviar agora')}
          </Btn>
        </div>
      )}

      {erro && (
        <p className="error" role="alert">
          {erro}
        </p>
      )}

      <div className="studio-grid">
        <div className="studio-palco">
          <div
            className={temCamara && !aRecortar ? 'studio-canvas-wrap arrastavel' : 'studio-canvas-wrap'}
            ref={palcoRef}
            onPointerDown={(e) => {
              // Arrastar a bolha para qualquer sítio (achado: os quatro cantos
              // não chegam quando o conteúdo do slide está justamente no canto).
              if (!temCamara || aRecortar) return
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
            }}
          >
            {!temEcra && !temCamara && pronto && (
              <div className="studio-vazio">
                <p>{t('studio.comecaPor', 'Começa por escolher o que gravar.')}</p>
                <Btn onClick={() => void escolherEcra()}>{t('studio.escolherEcra', 'Escolher ecrã')}</Btn>
              </div>
            )}
          </div>
          {temEcra && (
            <SeletorDeRegiao
              aberto={aRecortar}
              recorte={recorte}
              onFechar={() => setARecortar(false)}
              onAplicar={(r) => {
                setRecorte(r)
                setARecortar(false)
              }}
            />
          )}
        </div>

        <aside className="studio-painel">
          <section className="studio-grupo">
            <h3>{t('studio.oQueGravar', 'O que gravar')}</h3>
            <Btn variant={temEcra ? 'ghost' : 'primary'} onClick={() => void escolherEcra()}>
              {temEcra ? t('studio.trocarEcra', 'Trocar fonte') : t('studio.escolherEcra', 'Escolher ecrã')}
            </Btn>
            {temEcra && (
              <>
                <div className="studio-seg">
                  <button
                    className={recorte.w === 1 && recorte.h === 1 ? 'seg-btn active' : 'seg-btn'}
                    onClick={() => setRecorte({ ...RECORTE_INTEIRO })}
                  >
                    {t('studio.tudo', 'Tudo')}
                  </button>
                  <button
                    className={recorte.w < 1 || recorte.h < 1 ? 'seg-btn active' : 'seg-btn'}
                    onClick={() => setARecortar(true)}
                  >
                    {t('studio.regiao', 'Só uma região')}
                  </button>
                </div>
                {(recorte.w < 1 || recorte.h < 1) && (
                  <small className="muted mono">
                    {Math.round(recorte.w * 100)}% × {Math.round(recorte.h * 100)}%
                  </small>
                )}
              </>
            )}
          </section>

          <section className="studio-grupo">
            <h3>{t('studio.avatar', 'A tua imagem')}</h3>
            <Btn variant={temCamara ? 'ghost' : 'primary'} onClick={() => void alternarCamara()}>
              {temCamara ? <CamOffIcon /> : <CamIcon />}
              {temCamara ? t('studio.semCamara', 'Desligar câmara') : t('studio.comCamara', 'Ligar câmara')}
            </Btn>

            {temCamara && (
              <>
                <label className="set-label">
                  {t('studio.posicao', 'Posição')}
                  <small className="muted">{t('studio.arrastaBolha', 'Ou arrasta a bolha na pré-visualização.')}</small>
                  <div className="studio-cantos" role="group" aria-label={t('studio.posicao', 'Posição')}>
                    {CANTOS.map((c) => (
                      <button
                        key={c.key}
                        className={avatar.canto === c.key ? 'studio-canto active' : 'studio-canto'}
                        aria-pressed={avatar.canto === c.key}
                        title={c.key}
                        onClick={() => setAvatar((a) => ({ ...a, canto: c.key }))}
                      >
                        {c.rotulo}
                      </button>
                    ))}
                  </div>
                </label>

                <label className="set-label">
                  {t('studio.tamanho', 'Tamanho')}
                  <input
                    type="range"
                    min={10}
                    max={45}
                    value={Math.round(avatar.tamanho * 100)}
                    onChange={(e) => setAvatar((a) => ({ ...a, tamanho: Number(e.target.value) / 100 }))}
                  />
                </label>

                <div className="studio-seg">
                  {([
                    { m: 'bolha' as ModoDoAvatar, r: t('studio.modoBolha', 'Bolha') },
                    { m: 'recorte' as ModoDoAvatar, r: t('studio.modoRecorte', 'Sem fundo') },
                  ]).map(({ m, r }) => (
                    <button
                      key={m}
                      className={avatar.modo === m ? 'seg-btn active' : 'seg-btn'}
                      disabled={aPrepararRecorte}
                      onClick={() => {
                        if (m === 'recorte') void ligarRecorteDeFundo()
                        else pararRecorteDeFundo()
                      }}
                    >
                      {aPrepararRecorte && m === 'recorte' ? t('studio.aPreparar', 'A preparar…') : r}
                    </button>
                  ))}
                </div>
                {avatar.modo === 'recorte' && (
                  <small className="muted">
                    {t('studio.recorteNota', 'A segmentação corre no teu computador — nada sai da máquina.')}
                  </small>
                )}

                <div className="studio-seg">
                  {(['circulo', 'rectangulo'] as FormaDoAvatar[]).map((f) => (
                    <button
                      key={f}
                      className={avatar.forma === f ? 'seg-btn active' : 'seg-btn'}
                      disabled={avatar.modo === 'recorte'}
                      title={avatar.modo === 'recorte' ? t('studio.formaNA', 'Sem fundo, a forma é a da pessoa.') : undefined}
                      onClick={() => setAvatar((a) => ({ ...a, forma: f }))}
                    >
                      {f === 'circulo' ? t('studio.circulo', 'Círculo') : t('studio.rect', 'Rectângulo')}
                    </button>
                  ))}
                </div>
              </>
            )}
          </section>

          {resultado && (
            <section className="studio-grupo studio-resultado">
              <h3>{t('studio.pronta', 'Aula gravada')}</h3>
              <video
                className="studio-preview"
                src={resultado.url}
                controls
                ref={previewRef}
                onLoadedMetadata={(e) => {
                  const d = e.currentTarget.duration
                  // Um WebM de MediaRecorder chega muitas vezes com duração
                  // `Infinity` até se procurar até ao fim: sem isto o cursor
                  // de corte nascia sem escala.
                  if (Number.isFinite(d) && d > 0) {
                    setResultado((r) => (r ? { ...r, duracao: d } : r))
                    setAte((a) => (a > 0 ? a : d))
                  } else {
                    e.currentTarget.currentTime = 1e6
                  }
                }}
                onDurationChange={(e) => {
                  const d = e.currentTarget.duration
                  if (Number.isFinite(d) && d > 0) {
                    setResultado((r) => (r ? { ...r, duracao: d } : r))
                    setAte((a) => (a > 0 ? a : d))
                  }
                }}
              />

              {resultado.duracao > 0 && cortesSuportados() && (
                <div className="studio-corte">
                  <label className="set-label">
                    {t('studio.corteDe', 'Começar em')} <span className="mono">{mmss(Math.round(de))}</span>
                    <input
                      type="range"
                      min={0}
                      max={Math.floor(resultado.duracao)}
                      value={Math.min(de, ate)}
                      onChange={(e) => {
                        const v = Number(e.target.value)
                        setDe(v)
                        if (v >= ate) setAte(Math.min(resultado.duracao, v + 1))
                        if (previewRef.current) previewRef.current.currentTime = v
                      }}
                    />
                  </label>
                  <label className="set-label">
                    {t('studio.corteAte', 'Terminar em')} <span className="mono">{mmss(Math.round(ate))}</span>
                    <input
                      type="range"
                      min={0}
                      max={Math.floor(resultado.duracao)}
                      value={ate}
                      onChange={(e) => {
                        const v = Number(e.target.value)
                        setAte(v)
                        if (v <= de) setDe(Math.max(0, v - 1))
                        if (previewRef.current) previewRef.current.currentTime = v
                      }}
                    />
                  </label>
                  <Btn
                    variant="ghost"
                    disabled={aCortar > 0 || ate - de < 1 || (de === 0 && Math.round(ate) >= Math.floor(resultado.duracao))}
                    onClick={() => void aplicarCorte()}
                  >
                    {aCortar > 0
                      ? `${t('studio.aCortar', 'A cortar')} ${Math.round(aCortar * 100)}%`
                      : `${t('studio.cortar', 'Cortar')} · ${mmss(Math.round(ate - de))}`}
                  </Btn>
                </div>
              )}

              {resultado.faixas.audio && cortesSuportados() && (
                <div className="studio-pausas">
                  <small className="muted">{t('studio.pausasTit', 'Pausas mortas')}</small>
                  {!analise ? (
                    <Btn variant="ghost" disabled={aAnalisar || aCortar > 0} onClick={() => void procurarPausas()}>
                      {aAnalisar ? t('studio.aAnalisar', 'A analisar…') : t('studio.procurarPausas', 'Procurar pausas')}
                    </Btn>
                  ) : resumo(analise).pausas === 0 ? (
                    <small className="studio-ok">{t('studio.semPausas', 'Sem pausas para apertar — a aula está corrida.')}</small>
                  ) : (
                    <>
                      {/* Mapa da aula: o que fica e o que sai, de relance. */}
                      <div
                        className="studio-mapa"
                        role="img"
                        aria-label={t('studio.pausasEncontradas', '{{n}} pausas · {{s}} s a poupar', {
                          n: resumo(analise).pausas,
                          s: resumo(analise).poupanca,
                        })}
                      >
                        {analise.pausas.map((pa, i) => (
                          <span
                            key={i}
                            className="studio-mapa-pausa"
                            style={{
                              left: `${(pa.inicio / analise.duracao) * 100}%`,
                              width: `${((pa.fim - pa.inicio) / analise.duracao) * 100}%`,
                            }}
                          />
                        ))}
                      </div>
                      <small className="muted">
                        {t('studio.pausasEncontradas', '{{n}} pausas · {{s}} s a poupar', {
                          n: resumo(analise).pausas,
                          s: resumo(analise).poupanca,
                        })}{' '}
                        ({resumo(analise).pct}%)
                      </small>
                      <div className="studio-seg">
                        <Btn variant="ghost" disabled={aCortar > 0} onClick={() => void apertarPausas()}>
                          {aCortar > 0
                            ? `${t('studio.aCortar', 'A cortar')} ${Math.round(aCortar * 100)}%`
                            : t('studio.apertar', 'Apertar pausas')}
                        </Btn>
                        <Btn variant="ghost" disabled={aCortar > 0} onClick={() => setAnalise(null)}>
                          {t('common.cancel', 'Cancelar')}
                        </Btn>
                      </div>
                    </>
                  )}
                </div>
              )}

              {resultado.faixas.audio && (
                <div className="studio-faixas">
                  <small className="muted">{t('studio.faixas', 'Faixas separadas')}</small>
                  <div className="studio-seg">
                    <button className="seg-btn" onClick={() => descarregar('video')} disabled={!resultado.faixas.video}>
                      {t('studio.soVideo', 'Só vídeo')}
                    </button>
                    <button className="seg-btn" onClick={() => descarregar('audio')}>
                      {t('studio.soAudio', 'Só áudio')}
                    </button>
                  </div>
                </div>
              )}

              <label className="set-label">
                {t('studio.titulo2', 'Título')}
                <input
                  value={titulo}
                  onChange={(e) => setTitulo(e.target.value)}
                  placeholder={t('studio.tituloPh', 'Ex.: Aula 3 — funções')}
                  maxLength={80}
                />
              </label>
              <div className="studio-resultado-acoes">
                <Btn onClick={() => void guardarNaBiblioteca()} disabled={aGuardar}>
                  {aGuardar ? t('studio.aGuardar', 'A guardar…') : t('studio.guardar', 'Guardar na biblioteca')}
                </Btn>
                <Btn variant="ghost" onClick={() => descarregar('completo')}>
                  {t('studio.descarregar', 'Descarregar')}
                </Btn>
              </div>
              {guardado && <p className="studio-ok">{guardado}</p>}
            </section>
          )}
        </aside>
      </div>
    </div>
  )
}

/**
 * Seletor de região: arrasta-se um rectângulo por cima da pré-visualização.
 * As coordenadas saem em FRACÇÕES, não em pixéis — o canvas de gravação e a
 * pré-visualização têm tamanhos diferentes, e guardar pixéis de um deles
 * partia o recorte assim que a janela mudasse de tamanho.
 */
function SeletorDeRegiao({
  aberto,
  recorte,
  onAplicar,
  onFechar,
}: {
  aberto: boolean
  recorte: Recorte
  onAplicar: (r: Recorte) => void
  onFechar: () => void
}) {
  const { t } = useTranslation()
  const ref = useRef<HTMLDivElement>(null)
  const [arrasto, setArrasto] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null)

  if (!aberto) return null

  function fraccao(e: React.PointerEvent): { x: number; y: number } {
    const r = ref.current!.getBoundingClientRect()
    return {
      x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
      y: Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
    }
  }

  const sel = arrasto && {
    left: `${Math.min(arrasto.x0, arrasto.x1) * 100}%`,
    top: `${Math.min(arrasto.y0, arrasto.y1) * 100}%`,
    width: `${Math.abs(arrasto.x1 - arrasto.x0) * 100}%`,
    height: `${Math.abs(arrasto.y1 - arrasto.y0) * 100}%`,
  }

  return (
    <div className="studio-recorte" role="dialog" aria-label={t('studio.regiao', 'Só uma região')}>
      <div
        className="studio-recorte-area"
        ref={ref}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId)
          const p = fraccao(e)
          setArrasto({ x0: p.x, y0: p.y, x1: p.x, y1: p.y })
        }}
        onPointerMove={(e) => {
          if (!arrasto) return
          const p = fraccao(e)
          setArrasto((a) => (a ? { ...a, x1: p.x, y1: p.y } : a))
        }}
        onPointerUp={() => {
          if (!arrasto) return
          const w = Math.abs(arrasto.x1 - arrasto.x0)
          const h = Math.abs(arrasto.y1 - arrasto.y0)
          // Um arrasto minúsculo é um clique falhado, não uma região de 2px.
          if (w < 0.03 || h < 0.03) {
            setArrasto(null)
            return
          }
          onAplicar({ x: Math.min(arrasto.x0, arrasto.x1), y: Math.min(arrasto.y0, arrasto.y1), w, h })
          setArrasto(null)
        }}
      >
        {sel && <div className="studio-recorte-sel" style={sel} />}
      </div>
      <div className="studio-recorte-barra">
        <span className="muted">{t('studio.arrasta', 'Arrasta para escolher a região a gravar.')}</span>
        <Btn variant="ghost" onClick={() => onAplicar({ ...RECORTE_INTEIRO })}>
          {t('studio.tudo', 'Tudo')}
        </Btn>
        <Btn variant="ghost" onClick={onFechar}>
          {t('common.cancel', 'Cancelar')}
        </Btn>
      </div>
      <span className="sr-only">
        {Math.round(recorte.w * 100)}% × {Math.round(recorte.h * 100)}%
      </span>
    </div>
  )
}
