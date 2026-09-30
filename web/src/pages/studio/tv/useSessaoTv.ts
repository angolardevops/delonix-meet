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
import { RegistoDeFontes } from '../../../studio/tv/fontes'
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
  type TipoDeTransicao,
} from '../../../studio/tv/mesa'
import { MesaDeSom } from '../../../studio/tv/mesaDeSom'
import { CORTE_DE_VOZ_ZERO, decidirCorteDeVoz } from '../../../studio/tv/vozCorte'
import { isTypingTarget } from '../../../ui/hotkeys'

export type { EcraTv } from '../../../studio/tv/ecras'
export { ECRAS_TV } from '../../../studio/tv/ecras'

export interface DispositivoDeVideo {
  deviceId: string
  nome: string
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
    setCamaras(d.cams.map((c, i) => ({ deviceId: c.deviceId, nome: c.label || t('tv.fontes.camaraN', { n: i + 1 }) })))
    setMicrofones(d.mics.map((c, i) => ({ deviceId: c.deviceId, nome: c.label || t('tv.som.microfoneN', { n: i + 1 }) })))
    setSaidas(d.speakers.map((x, i) => ({ deviceId: x.deviceId, nome: x.label || t('tv.som.saidaN', { n: i + 1 }) })))
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
        await registo.ligarCamara(d.deviceId, d.nome)
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t])

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
        case 'macro':
          correrMacroDaTecla(a.n)
          break
      }
    }
    window.addEventListener('keydown', aoTeclar, true)
    return () => window.removeEventListener('keydown', aoTeclar, true)
  }, [p.atalhosActivos, p.haSondagem, accoes, planoDoNumero, definirSobreposicao, sobreposicaoLigada, correrMacroDaTecla])

  return {
    registo,
    versaoFontes,
    camaras,
    microfones,
    saidas,
    procurarDispositivos,
    ligarCamara,
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
    macros,
    progressoMacro,
    correrMacroDaTecla,
  }
}

export type SessaoTv = ReturnType<typeof useSessaoTv>
