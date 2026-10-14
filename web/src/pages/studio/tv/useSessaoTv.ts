/**
 * A sessão do estúdio de TV: fontes, mesa de corte, mesa de som, macros e
 * corte por voz, e a ponte para o compositor do Estúdio.
 *
 * Vive enquanto o Estúdio estiver aberto (o `EstudioTv` monta à primeira
 * visita a um ecrã de TV e fica montado, escondido, no resto), porque os
 * cinco ecrãs são vistas da MESMA sessão: cortar na mesa e ver o corte na
 * cena completa, mexer num fader e ver o nível no StudioLive.
 *
 * O estado que muda a cada frame (transições, T-bar, níveis) vive em refs e
 * nos objectos imperativos; o React só é avisado quando há algo para
 * redesenhar — como no resto do Estúdio.
 */
import { MutableRefObject, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listDevices } from '../../../media'
import type { Fonte } from '../../../room/compositor'
import type { CompositorDeAula } from '../../../studio/compositor'
import type { Palco } from '../../../studio/usePalco'
import { accaoDaTecla } from '../../../studio/tv/atalhos'
import { microfoneDoGrupo, RegistoDeFontes } from '../../../studio/tv/fontes'
import { correrMacro, dbParaLinear, esperar, type Macro, MACROS_INICIAIS, macroDaTecla, type Passo, type ProgressoDaMacro, type ResultadoDoPasso, type Sobreposicao } from '../../../studio/tv/macros'
import {
  auto,
  avancar,
  cortar,
  directoAoAr,
  type EstadoDaMesa,
  escolherTransicao,
  fontePrincipal,
  limparFontes,
  MESA_INICIAL,
  moverTbar,
  mudarDuracao,
  type Plano,
  planoDe,
  porEmPrevia,
  quadroDaMesa,
  tallyDe,
  type TipoDeTransicao,
} from '../../../studio/tv/mesa'
import { MesaDeSom } from '../../../studio/tv/mesaDeSom'
import {
  apagarEfeito,
  DUCK_DB,
  type Efeito,
  efeitoDaTecla,
  guardarEfeito,
  haVoz,
  idDoEfeito,
  indiceDoEfeito,
  lerEfeitos,
  MAX_EFEITOS,
} from '../../../studio/tv/efeitos'
import type { AccaoFisica } from '../../../studio/tv/mesaFisica'
import { CORTE_DE_VOZ_ZERO, decidirCorteDeVoz } from '../../../studio/tv/vozCorte'
import { useMesaFisica } from './useMesaFisica'
import { isTypingTarget } from '../../../ui/hotkeys'

export type { EcraTv } from '../../../studio/tv/ecras'
export { ECRAS_TV } from '../../../studio/tv/ecras'

export interface DispositivoDeVideo {
  deviceId: string
  nome: string
  /** O grupo de hardware: é ele que casa uma câmara com o microfone dela. */
  groupId?: string
}

export interface PropsDaSessao {
  compRef: MutableRefObject<CompositorDeAula | null>
  palco: Palco
  temEcra: boolean
  participantes: Fonte[]
  haSondagem: boolean
  gravando: boolean
  noAr: boolean
  onPararGravacao: () => Promise<void> | void
  onTerminarEmissao: () => Promise<void> | void
  /** Os atalhos só valem com a mesa à vista. */
  atalhosActivos: boolean
}

const SOBREPOSICAO_DO_NUMERO: Record<number, Sobreposicao> = { 1: 'legenda', 2: 'logotipo', 3: 'relogio', 4: 'sondagem' }

export function useSessaoTv(p: PropsDaSessao) {
  const { t } = useTranslation()
  const { compRef, palco } = p

  // ------------------------------------------------------------ fontes
  const registo = useMemo(() => new RegistoDeFontes(), [])
  const [versaoFontes, setVersaoFontes] = useState(0)
  const [camaras, setCamaras] = useState<DispositivoDeVideo[]>([])
  const [microfones, setMicrofones] = useState<DispositivoDeVideo[]>([])
  const [saidas, setSaidas] = useState<DispositivoDeVideo[]>([])
  const [erroFonte, setErroFonte] = useState('')

  useEffect(() => {
    registo.aoMudar = () => setVersaoFontes((v) => v + 1)
    return () => {
      registo.aoMudar = null
      registo.destruir()
    }
  }, [registo])

  const procurarDispositivos = useCallback(async () => {
    const d = await listDevices().catch(() => null)
    if (!d) return
    setCamaras(d.cams.map((c, i) => ({ deviceId: c.deviceId, groupId: c.groupId, nome: c.label || t('tv.fontes.camaraN', { n: i + 1 }) })))
    setMicrofones(d.mics.map((c, i) => ({ deviceId: c.deviceId, groupId: c.groupId, nome: c.label || t('tv.som.microfoneN', { n: i + 1 }) })))
    setSaidas(d.speakers.map((x, i) => ({ deviceId: x.deviceId, groupId: x.groupId, nome: x.label || t('tv.som.saidaN', { n: i + 1 }) })))
  }, [t])

  useEffect(() => {
    void procurarDispositivos()
    const md = navigator.mediaDevices
    const f = () => void procurarDispositivos()
    md?.addEventListener?.('devicechange', f)
    return () => md?.removeEventListener?.('devicechange', f)
  }, [procurarDispositivos])

  const ligarCamara = useCallback(
    async (d: DispositivoDeVideo) => {
      setErroFonte('')
      try {
        await registo.ligarCamara(d.deviceId, d.nome, d.groupId)
        // Com permissão dada, os nomes dos dispositivos passam a vir preenchidos.
        void procurarDispositivos()
      } catch (e) {
        setErroFonte((e as Error)?.name === 'NotAllowedError' ? t('tv.fontes.erros.permissao') : t('tv.fontes.erros.camara'))
      }
    },
    [registo, procurarDispositivos, t],
  )

  // O ecrã e o quadro do Estúdio, e os participantes da sala ligada.
  useEffect(() => {
    const c = compRef.current
    registo.definirEcra(p.temEcra ? (c?.videoDoEcra ?? null) : null, c?.fluxoDoEcra ?? null, t('tv.fontes.ecra'))
  }, [registo, compRef, p.temEcra, t])
  useEffect(() => {
    registo.definirQuadro(compRef.current?.quadro ?? null, t('tv.fontes.quadro'))
  }, [registo, compRef, t])
  useEffect(() => registo.definirParticipantes(p.participantes), [registo, p.participantes])

  /**
   * Um ficheiro de vídeo como fonte — o «rolar o VT». Nasce em pausa e no
   * princípio; quem o corta ao ar decide quando começa.
   */
  const adicionarFicheiro = useCallback((f: File) => registo.adicionarFicheiro(f), [registo])

  /** O transporte de um ficheiro. `null` para o que não é ficheiro. */
  const comandarFicheiro = useCallback(
    (id: string, accao: 'tocar' | 'pausar' | 'inicio' | 'ciclo') => {
      const v = registo.video(id)
      if (!v) return
      if (accao === 'tocar') void v.play().catch(() => undefined)
      else if (accao === 'pausar') v.pause()
      else if (accao === 'ciclo') v.loop = !v.loop
      else {
        v.currentTime = 0
        v.pause()
      }
      setVersaoFontes((x) => x + 1)
    },
    [registo],
  )

  // ------------------------------------------------------------ mesa de corte
  const mesaRef = useRef<EstadoDaMesa>(MESA_INICIAL)
  const [mesa, setMesaEstado] = useState<EstadoDaMesa>(MESA_INICIAL)
  const setMesa = useCallback((f: (e: EstadoDaMesa) => EstadoDaMesa) => {
    const novo = f(mesaRef.current)
    if (novo === mesaRef.current) return
    mesaRef.current = novo
    setMesaEstado(novo)
  }, [])

  // Uma fonte que desaparece sai do programa e da pré.
  useEffect(() => {
    setMesa((e) => limparFontes(e, registo.ids()))
  }, [versaoFontes, registo, setMesa])

  // Uma mesa acabada de abrir põe a primeira fonte em PRÉ — não no ar: o que
  // está no ar continua a ser o palco do Estúdio até alguém cortar. Abrir a
  // mesa para espreitar não pode mudar a emissão.
  useEffect(() => {
    const bus = registo.barramento()
    if (!bus.length) return
    setMesa((e) => {
      if (e.previa || e.emCurso) return e
      const livre = bus.find((f) => !e.programa?.fontes.includes(f.id))
      return livre ? { ...e, previa: planoDe(livre.id) } : e
    })
  }, [versaoFontes, registo, setMesa])

  // O compositor desenha o programa da mesa quando ela tem alguma coisa no ar.
  const temPrograma = !!(mesa.programa || mesa.emCurso)
  useEffect(() => {
    const c = compRef.current
    if (!c) return
    c.mesa = temPrograma ? { fontes: registo, quadro: (agora) => quadroDaMesa(mesaRef.current, agora) } : null
  }, [compRef, registo, temPrograma])
  useEffect(
    () => () => {
      if (compRef.current) compRef.current.mesa = null
    },
    [compRef],
  )

  // Um AUTO a decorrer avança a cada frame; o React só sabe quando acaba.
  const emCurso = mesa.emCurso
  useEffect(() => {
    if (!emCurso || emCurso.inicio < 0) return
    let raf = 0
    const passo = () => {
      const antes = mesaRef.current
      const depois = avancar(antes, Date.now())
      if (depois !== antes) {
        mesaRef.current = depois
        setMesaEstado(depois)
        return
      }
      raf = requestAnimationFrame(passo)
    }
    raf = requestAnimationFrame(passo)
    return () => cancelAnimationFrame(raf)
  }, [emCurso])

  /**
   * O microfone QUE PERTENCE a esta fonte, pelo grupo de hardware — ou `null`.
   *
   * Uma câmara entra sem som de propósito (`getUserMedia({audio:true})` com um
   * `deviceId` de vídeo devolve o microfone POR OMISSÃO do sistema, não o da
   * câmara: seriam dois dispositivos diferentes a fingir ser um). O que casa os
   * dois é o `groupId`. Até 2026-10-14 não havia forma de saber qual dos
   * microfones da lista era o da capturadora HDMI que acabou de ser ligada.
   */
  const microfoneDaFonte = useCallback(
    (id: string): DispositivoDeVideo | null => {
      return microfoneDoGrupo(microfones, registo.obter(id)?.groupId)
    },
    [registo, microfones],
  )

  const planoDoNumero = useCallback((n: number): Plano | null => {
    const id = registo.idDoNumero(n)
    return id ? planoDe(id) : null
  }, [registo])

  const accoes = useMemo(
    () => ({
      previa: (plano: Plano) => setMesa((e) => porEmPrevia(e, plano)),
      ar: (plano: Plano) => setMesa((e) => directoAoAr(e, plano, Date.now())),
      cortar: () => setMesa((e) => cortar(e, Date.now())),
      auto: (tipo?: TipoDeTransicao) => setMesa((e) => auto(e, Date.now(), tipo)),
      escolher: (tipo: TipoDeTransicao) =>
        setMesa((e) => {
          const escolhida = escolherTransicao(e, tipo)
          // Carregar num tipo faz a transição já (CORTAR corta; os outros correm em AUTO).
          return auto(escolhida, Date.now(), tipo)
        }),
      duracao: (ms: number) => setMesa((e) => mudarDuracao(e, ms)),
      tbar: (pos: number) => setMesa((e) => moverTbar(e, pos, Date.now())),
    }),
    [setMesa],
  )

  // ------------------------------------------------------------ sobreposições
  const sobreposicaoLigada = useCallback(
    (q: Sobreposicao) => {
      const s = palco.sobreposicoes
      return q === 'legenda' ? s.rodape : q === 'logotipo' ? s.logotipo : q === 'relogio' ? s.cronometro : s.sondagem
    },
    [palco.sobreposicoes],
  )
  const definirSobreposicao = useCallback(
    (q: Sobreposicao, ligada: boolean) => {
      if (q === 'legenda') palco.mudarSobreposicoes({ rodape: ligada })
      else if (q === 'logotipo') palco.mudarSobreposicoes({ logotipo: ligada })
      else if (q === 'relogio') palco.mudarSobreposicoes({ cronometro: ligada })
      else palco.mudarSobreposicoes({ sondagem: ligada })
    },
    [palco],
  )

  // ------------------------------------------------------------ mesa de som
  const [som, setSom] = useState<MesaDeSom | null>(null)
  const [versaoSom, setVersaoSom] = useState(0)
  const [erroSom, setErroSom] = useState('')
  const somRef = useRef<MesaDeSom | null>(null)

  const ligarSom = useCallback(async () => {
    if (somRef.current) return somRef.current
    const m = new MesaDeSom()
    somRef.current = m
    m.aoMudar = () => setVersaoSom((v) => v + 1)
    try {
      await m.preparar()
      await m.retomar()
      setSom(m)
      // O microfone escolhido no Estúdio entra como primeiro canal.
      try {
        await m.adicionarMicrofone(palco.microfone, palco.microfones.find((x) => x.id === palco.microfone)?.nome ?? t('tv.som.microfonePrincipal'))
      } catch {
        setErroSom(t('tv.som.erros.microfone'))
      }
      return m
    } catch {
      setErroSom(t('tv.som.erros.contexto'))
      somRef.current = null
      m.destruir()
      return null
    }
    // Só `t`, para as mensagens de erro. Os `palco.microfone(s)` mudam quando
    // alguém sobe ao palco, e recriar este callback aí desmontava a mesa de som
    // da cena a decorrer.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t])

  /** Leva o som de uma fonte à mesa — abrindo-a, se ainda não estiver aberta. */
  const levarSomDaFonteAMesa = useCallback(
    async (id: string): Promise<void> => {
      const mic = microfoneDaFonte(id)
      if (!mic) return
      const mesa = somRef.current ?? (await ligarSom())
      await mesa?.adicionarMicrofone(mic.deviceId, mic.nome).catch(() => undefined)
    },
    [microfoneDaFonte, ligarSom],
  )

  useEffect(
    () => () => {
      compRef.current?.definirSomExterno(null)
      somRef.current?.destruir()
      somRef.current = null
    },
    [compRef],
  )

  // Com canais na mesa, o som do ar É a mesa (e não o microfone do compositor).
  const canaisDeSom = som?.lista().length ?? 0
  useEffect(() => {
    compRef.current?.definirSomExterno(som && canaisDeSom > 0 ? som.saida : null)
  }, [compRef, som, canaisDeSom])

  // O som do separador partilhado é um CANAL da mesa — com fader, medidor, EQ e
  // porta — e não um barramento escondido do compositor. Até 2026-10-14 o tipo
  // de canal `'ecra'` existia e NADA o criava: partilhar um vídeo punha-o no ar
  // sem fader nem medidor, e a única forma de o baixar era o deslizador de
  // mistura do palco, noutro ecrã. Enquanto a mesa o tem, o compositor larga-o
  // (`ligarSomDoEcra`), senão o mesmo som entrava duas vezes.
  useEffect(() => {
    if (!som) return
    const audio = (p.temEcra ? (compRef.current?.fluxoDoEcra ?? null) : null)?.getAudioTracks() ?? []
    if (audio.length) som.adicionarFluxo('ecra', t('tv.fontes.ecra'), 'ecra', new MediaStream(audio), 'ecra')
    else if (som.lista().some((c) => c.id === 'ecra')) som.remover('ecra')
  }, [som, compRef, p.temEcra, versaoFontes, t])

  // A música também é um canal: até 2026-10-14 vivia num barramento do
  // compositor que a mesa não via — não tinha fader, nem medidor, nem forma de
  // a baixar sob a voz sem ir a outro ecrã. Só existe com um fluxo montado
  // (gravação ou directo): fora disso o grafo de áudio do compositor não
  // existe e a música toca pelo volume do elemento.
  useEffect(() => {
    if (!som) return
    const audio = (palco.musica ? (compRef.current?.fluxoDeMusica ?? null) : null)?.getAudioTracks() ?? []
    if (audio.length) som.adicionarFluxo('musica', palco.musica?.nome || t('tv.som.tipos.musica'), 'musica', new MediaStream(audio), null)
    else if (som.lista().some((c) => c.id === 'musica')) som.remover('musica')
  }, [som, compRef, palco.musica, p.gravando, p.noAr, t])

  // O som de um FICHEIRO é um canal, e o canal tem o id da fonte: assim o
  // tally da tira e o «seguir a imagem» funcionam sem mais nada — o canal já
  // sabe de que imagem é.
  useEffect(() => {
    if (!som) return
    const ids = new Set<string>()
    for (const f of registo.lista()) {
      if (f.tipo !== 'ficheiro') continue
      const el = registo.video(f.id)
      if (!el) continue
      ids.add(f.id)
      som.adicionarElemento(f.id, f.nome, el, f.id)
    }
    for (const c of som.lista()) if (c.tipo === 'media' && !ids.has(c.id)) som.remover(c.id)
  }, [som, registo, versaoFontes])

  // Participantes com áudio entram como canais; quem sai, sai.
  useEffect(() => {
    if (!som) return
    const ids = new Set<string>()
    for (const pessoa of p.participantes) {
      if (!pessoa.stream?.getAudioTracks().length) continue
      const id = `p:${pessoa.id}`
      ids.add(id)
      som.adicionarFluxo(id, pessoa.nome, 'participante', pessoa.stream, id)
    }
    for (const c of som.lista()) if (c.tipo === 'participante' && !ids.has(c.id)) som.remover(c.id)
  }, [som, p.participantes])

  // A MESA NASCE COM A SESSÃO, não com o ecrã dela. Até 2026-10-14 o
  // `ligarSom()` estava no `useEffect` de montagem da Mesa de som e da Cena
  // completa: quem fosse ao ar sem passar por um desses dois ecrãs emitia pelo
  // microfone cru do compositor — com `echoCancellation` e `noiseSuppression`
  // ligados, que é o certo numa reunião e errado numa emissão — e sem EQ,
  // porta, compressor nem medidor. O que vai ao ar passa pela mesa.
  //
  // Espera-se por um gesto porque um `AudioContext` criado sem ele nasce
  // suspenso, e porque pedir o microfone é um pedido ao browser: faz-se quando
  // a pessoa já está a mexer no estúdio, não no primeiro `render`.
  useEffect(() => {
    if (somRef.current) return
    const abrir = () => void ligarSom()
    // Quem chegou aqui a clicar já deu o gesto: abre já, para quem entra pelo
    // endereço directo (`?vista=tv&ecra=mesa-de-som`) não ver «a ligar…» à
    // espera de um clique que não tem razão de ser.
    if (navigator.userActivation?.hasBeenActive) {
      abrir()
      return
    }
    window.addEventListener('pointerdown', abrir, { capture: true, once: true })
    window.addEventListener('keydown', abrir, { capture: true, once: true })
    return () => {
      window.removeEventListener('pointerdown', abrir, true)
      window.removeEventListener('keydown', abrir, true)
    }
  }, [ligarSom])

  // O AudioContext nasce suspenso sem gesto: qualquer toque na mesa retoma-o.
  useEffect(() => {
    if (!som) return
    const r = () => void som.retomar()
    window.addEventListener('pointerdown', r, true)
    window.addEventListener('keydown', r, true)
    return () => {
      window.removeEventListener('pointerdown', r, true)
      window.removeEventListener('keydown', r, true)
    }
  }, [som])

  // SEGUIR A IMAGEM: o canal que o pede abre quando a fonte dele entra no ar e
  // cala quando sai. É o outro sentido do corte por voz — e a razão de o
  // selector «fonte do canal» existir: até 2026-10-14 ele só servia o corte por
  // voz, e ligar um canal a uma fonte não fazia mais nada.
  useEffect(() => {
    if (!som) return
    for (const c of som.lista()) {
      if (!c.seguirImagem || !c.fonte) continue
      const mudo = tallyDe(mesa, c.fonte) !== 'programa'
      if (c.mudo !== mudo) som.mudar(c.id, { mudo })
    }
  }, [som, versaoSom, mesa])

  // ------------------------------------------------------------ efeitos
  //
  // As quatro teclas (⌥5–⌥8). Cada uma é um CANAL da mesa — com fader,
  // medidor, EQ e limitador —, não um `<audio>` solto: um genérico que saísse
  // por fora da mesa era exactamente o som que ninguém controla. Os ficheiros
  // vivem em IndexedDB e voltam ao abrir o estúdio; o som descodificado vive
  // só nesta sessão, porque é do `AudioContext` dela.
  const [efeitos, setEfeitos] = useState<(Efeito | null)[]>(() => Array(MAX_EFEITOS).fill(null))
  const buffers = useRef(new Map<string, AudioBuffer>())

  const carregarEfeito = useCallback(
    async (indice: number, nome: string, ficheiro: Blob, emCiclo: boolean, mesa?: MesaDeSom | null) => {
      const id = idDoEfeito(indice)
      const m = mesa ?? somRef.current
      if (m) {
        try {
          buffers.current.set(id, await m.ctx.decodeAudioData(await ficheiro.arrayBuffer()))
        } catch {
          // Um ficheiro que o browser não sabe ler não fica meio carregado: a
          // tecla continua livre e a pessoa percebe porquê ao tentar outra vez.
          return false
        }
        m.adicionarEfeito(id, nome)
      }
      setEfeitos((a) => a.map((e, i) => (i === indice ? { id, nome, emCiclo } : e)))
      return true
    },
    [],
  )

  /** Põe um ficheiro numa tecla: guarda-o, descodifica-o e cria o canal. */
  const porEfeito = useCallback(
    async (indice: number, ficheiro: File) => {
      const nome = ficheiro.name.replace(/\.[^.]+$/, '').slice(0, 40) || t('tv.efeitos.semNome')
      const ok = await carregarEfeito(indice, nome, ficheiro, false)
      if (ok) await guardarEfeito({ id: idDoEfeito(indice), nome, emCiclo: false, ficheiro }).catch(() => undefined)
    },
    [carregarEfeito, t],
  )

  const tirarEfeito = useCallback((indice: number) => {
    const id = idDoEfeito(indice)
    somRef.current?.remover(id)
    buffers.current.delete(id)
    setEfeitos((a) => a.map((e, i) => (i === indice ? null : e)))
    void apagarEfeito(id).catch(() => undefined)
  }, [])

  const dispararEfeito = useCallback((indice: number) => {
    const e = efeitos[indice]
    const m = somRef.current
    const b = e && buffers.current.get(e.id)
    if (!e || !m || !b) return
    if (m.aToca(e.id)) m.pararEfeito(e.id)
    else m.tocarEfeito(e.id, b, e.emCiclo)
  }, [efeitos])

  const mudarCicloDoEfeito = useCallback(
    (indice: number, emCiclo: boolean) => {
      const e = efeitos[indice]
      if (!e) return
      setEfeitos((a) => a.map((x, i) => (x && i === indice ? { ...x, emCiclo } : x)))
      void lerEfeitos().then((todos) => {
        const g = todos.find((x) => x.id === e.id)
        if (g) void guardarEfeito({ ...g, emCiclo }).catch(() => undefined)
      })
    },
    [efeitos],
  )

  // Os ficheiros guardados voltam assim que a mesa existe (é ela que
  // descodifica, no contexto dela).
  const efeitosCarregados = useRef(false)
  useEffect(() => {
    if (!som || efeitosCarregados.current) return
    efeitosCarregados.current = true
    void lerEfeitos().then(async (todos) => {
      for (const g of todos) {
        const i = indiceDoEfeito(g.id)
        if (i !== null) await carregarEfeito(i, g.nome, g.ficheiro, g.emCiclo, som)
      }
    })
  }, [som, carregarEfeito])

  // DUCKING: a música e os efeitos baixam enquanto alguém fala. A decisão é do
  // `haVoz` (puro, com teste); aqui só se mede e se aplica.
  const [duckLigado, setDuckLigado] = useState(true)
  useEffect(() => {
    if (!som) return
    if (!duckLigado) {
      som.aplicarDuck(0)
      return
    }
    const t0 = window.setInterval(() => som.aplicarDuck(haVoz(som.niveisDeVoz()) ? DUCK_DB : 0), 100)
    return () => {
      window.clearInterval(t0)
      som.aplicarDuck(0)
    }
  }, [som, duckLigado])

  // ------------------------------------------------------------ corte por voz
  const [vozLigada, setVozLigada] = useState(false)
  const vozRef = useRef(CORTE_DE_VOZ_ZERO)
  useEffect(() => {
    if (!vozLigada || !som) return
    const t0 = window.setInterval(() => {
      const niveis = new Map<string, number>()
      for (const c of som.lista()) {
        if (!c.fonte || c.mudo || !registo.obter(c.fonte)) continue
        const db = som.nivelPre(c.id).rmsDb
        niveis.set(c.fonte, Math.max(niveis.get(c.fonte) ?? -Infinity, db))
      }
      const e = mesaRef.current
      const d = decidirCorteDeVoz(vozRef.current, niveis, fontePrincipal(e.programa), Date.now())
      vozRef.current = d.estado
      if (d.cortarPara && !e.emCurso) setMesa((x) => directoAoAr(x, planoDe(d.cortarPara!), Date.now()))
    }, 100)
    return () => window.clearInterval(t0)
  }, [vozLigada, som, registo, setMesa])
  const canaisComFonte = som?.lista().filter((c) => c.fonte && registo.obter(c.fonte)).length ?? 0

  // ------------------------------------------------------------ macros
  const macros: readonly Macro[] = MACROS_INICIAIS
  const [progressoMacro, setProgressoMacro] = useState<ProgressoDaMacro | null>(null)
  const macroEmCurso = useRef<AbortController | null>(null)
  const refs = useRef({ p, som, sobreposicaoLigada })
  refs.current = { p, som, sobreposicaoLigada }

  const executar = useCallback(
    async (passo: Passo, sinal: AbortSignal): Promise<ResultadoDoPasso> => {
      const { p: props, som: mesaSom } = refs.current
      const indisponivel = (razao: string): ResultadoDoPasso => ({ estado: 'indisponivel', razao })
      switch (passo.tipo) {
        case 'previa': {
          const ids = passo.numeros.map((n) => registo.idDoNumero(n))
          if (ids.some((x) => !x)) return indisponivel('fonteEmFalta')
          setMesa((e) => porEmPrevia(e, { fontes: ids as string[], layout: ids.length > 1 ? passo.layout : 'solo' }))
          return { estado: 'feito' }
        }
        case 'transicao': {
          if (!mesaRef.current.previa) return indisponivel('semPrevia')
          const duracao = passo.duracaoMs ?? mesaRef.current.duracaoMs
          setMesa((e) => auto(e, Date.now(), passo.transicao, duracao))
          if (passo.transicao !== 'cortar') await esperar(duracao + 50, sinal)
          return { estado: 'feito' }
        }
        case 'sobreposicao':
          if (passo.qual === 'sondagem' && !props.haSondagem) return indisponivel('semSondagem')
          definirSobreposicao(passo.qual, passo.ligada)
          return { estado: 'feito' }
        case 'conteudo':
          props.palco.escolherConteudo(passo.conteudo)
          return { estado: 'feito' }
        case 'musica':
          if (!props.palco.musica) return indisponivel('semMusica')
          props.palco.mudarMistura({ musica: dbParaLinear(passo.db) })
          return { estado: 'feito' }
        case 'microfones': {
          const mics = mesaSom?.lista().filter((c) => c.tipo === 'microfone') ?? []
          if (!mesaSom || !mics.length) return indisponivel('semCanais')
          for (const c of mics) mesaSom.mudar(c.id, { mudo: passo.mudos })
          return { estado: 'feito' }
        }
        case 'luz':
          return indisponivel('semAgente')
        case 'terminarEmissao':
          if (!props.noAr) return indisponivel('naoNoAr')
          await props.onTerminarEmissao()
          return { estado: 'feito' }
        case 'pararGravacao':
          if (!props.gravando) return indisponivel('naoAGravar')
          await props.onPararGravacao()
          return { estado: 'feito' }
        default:
          return { estado: 'feito' }
      }
    },
    [registo, setMesa, definirSobreposicao],
  )

  const correrMacroDaTecla = useCallback(
    (n: number) => {
      const m = macroDaTecla(macros, n)
      if (!m) return
      macroEmCurso.current?.abort()
      const c = new AbortController()
      macroEmCurso.current = c
      correrMacro(m, executar, c.signal, setProgressoMacro)
        .catch(() => undefined)
        .finally(() => {
          if (macroEmCurso.current === c) macroEmCurso.current = null
        })
    },
    [macros, executar],
  )
  useEffect(() => () => macroEmCurso.current?.abort(), [])

  // ------------------------------------------------------------ mesa física
  //
  // Os faders e os botões de uma superfície MIDI mexem nos MESMOS objectos que
  // o rato: não há um caminho de controlo à parte, e por isso não há dois
  // estados a divergir. O que a superfície não souber fazer, faz-se no ecrã.
  const refExecutar = useRef<(a: AccaoFisica) => void>(() => undefined)
  refExecutar.current = (a: AccaoFisica) => {
    const m = somRef.current
    const canais = m?.lista() ?? []
    switch (a.tipo) {
      case 'fader': {
        const c = canais[a.canal]
        if (c && m) m.mudar(c.id, { fader: a.posicao })
        break
      }
      case 'mestre':
        m?.mudarMestre(a.posicao)
        break
      case 'mudo': {
        const c = canais[a.canal]
        if (c && m) m.mudar(c.id, { mudo: !c.mudo })
        break
      }
      case 'solo': {
        const c = canais[a.canal]
        if (c && m) m.mudar(c.id, { solo: !c.solo })
        break
      }
      case 'efeito':
        dispararEfeito(a.indice)
        break
      case 'cortar':
        setMesa((e) => cortar(e, Date.now()))
        break
      case 'misturar':
        setMesa((e) => auto(e, Date.now(), 'misturar'))
        break
    }
  }
  const mesaFisica = useMesaFisica({
    executar: (a) => refExecutar.current(a),
    luzes: () => (somRef.current?.lista() ?? []).map((c) => ({ mudo: c.mudo, solo: c.solo })),
  })

  // ------------------------------------------------------------ atalhos
  useEffect(() => {
    if (!p.atalhosActivos) return
    const aoTeclar = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.isComposing || isTypingTarget(e.target) || isTypingTarget(document.activeElement)) return
      const a = accaoDaTecla(e)
      if (!a) return
      // Um diálogo aberto (a paleta, as definições) é dono do teclado.
      if (document.querySelector('[role="dialog"][aria-modal="true"]')) return
      e.preventDefault()
      switch (a.tipo) {
        case 'previa': {
          const pl = planoDoNumero(a.n)
          if (pl) accoes.previa(pl)
          break
        }
        case 'ar': {
          const pl = planoDoNumero(a.n)
          if (pl) accoes.ar(pl)
          break
        }
        case 'cortar':
          accoes.cortar()
          break
        case 'misturar':
          accoes.auto('misturar')
          break
        case 'limpar':
          accoes.escolher('limpar')
          break
        case 'stinger':
          accoes.escolher('stinger')
          break
        case 'sobreposicao': {
          const q = SOBREPOSICAO_DO_NUMERO[a.n]
          if (q && (q !== 'sondagem' || p.haSondagem)) definirSobreposicao(q, !sobreposicaoLigada(q))
          break
        }
        case 'efeito': {
          const i = efeitoDaTecla(a.n)
          if (i !== null) dispararEfeito(i)
          break
        }
        case 'macro':
          correrMacroDaTecla(a.n)
          break
      }
    }
    window.addEventListener('keydown', aoTeclar, true)
    return () => window.removeEventListener('keydown', aoTeclar, true)
  }, [p.atalhosActivos, p.haSondagem, accoes, planoDoNumero, definirSobreposicao, sobreposicaoLigada, correrMacroDaTecla, dispararEfeito])

  return {
    registo,
    versaoFontes,
    camaras,
    microfones,
    saidas,
    procurarDispositivos,
    ligarCamara,
    adicionarFicheiro,
    comandarFicheiro,
    microfoneDaFonte,
    levarSomDaFonteAMesa,
    erroFonte,
    mesa,
    mesaRef,
    accoes,
    sobreposicaoLigada,
    definirSobreposicao,
    som,
    versaoSom,
    ligarSom,
    erroSom,
    vozLigada,
    setVozLigada,
    canaisComFonte,
    efeitos,
    porEfeito,
    tirarEfeito,
    dispararEfeito,
    mudarCicloDoEfeito,
    duckLigado,
    setDuckLigado,
    mesaFisica,
    macros,
    progressoMacro,
    correrMacroDaTecla,
  }
}

export type SessaoTv = ReturnType<typeof useSessaoTv>
