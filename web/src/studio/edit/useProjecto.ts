/**
 * Estado do projecto aberto: histórico, fontes em memória, gravação automática.
 *
 * A gravação automática corre 800 ms depois da última edição. Guarda o JSON do
 * projecto e um histórico curto; os blobs foram guardados UMA vez, quando a
 * fonte entrou, e nunca mais se escrevem.
 */
import { useCallback, useEffect, useRef, useState } from 'react'
import * as bd from './bd'
import * as H from './historico'
import { criarFonte } from './midia'
import type { Edicao, Fonte, OrigemDaFonte, Projecto, TipoDeFonte } from './projecto'
import { editar, montarInicial, novoProjecto } from './projecto'

const ULTIMO = 'dx_editor_ultimo'

export interface FonteEmBruto {
  blob: Blob
  nome: string
  origem: OrigemDaFonte
  tipo?: TipoDeFonte
  /** Id da gravação da biblioteca, quando a fonte veio de lá. */
  gravacao?: string
}

export interface EstadoDeGravacao {
  guardadoEm: number | null
  aGuardar: boolean
  erro: boolean
}

function lerUltimo(): string | null {
  try {
    return localStorage.getItem(ULTIMO)
  } catch {
    return null
  }
}
function escreverUltimo(id: string | null) {
  try {
    if (id) localStorage.setItem(ULTIMO, id)
    else localStorage.removeItem(ULTIMO)
  } catch {
    /* sem localStorage: abre-se vazio da próxima vez */
  }
}

export function useProjecto() {
  const [historico, setHistorico] = useState<H.Historico | null>(null)
  const [aCarregar, setACarregar] = useState(true)
  const [gravacao, setGravacao] = useState<EstadoDeGravacao>({ guardadoEm: null, aGuardar: false, erro: false })
  const [lista, setLista] = useState<bd.RegistoDeProjecto[]>([])
  const blobs = useRef(new Map<string, Blob>())
  const [urls, setUrls] = useState<Map<string, string>>(new Map())
  const sujo = useRef(false)
  const historicoRef = useRef<H.Historico | null>(null)
  historicoRef.current = historico

  const registarUrl = useCallback((id: string, blob: Blob) => {
    blobs.current.set(id, blob)
    setUrls((m) => {
      if (m.has(id)) return m
      const n = new Map(m)
      n.set(id, URL.createObjectURL(blob))
      return n
    })
  }, [])

  const largarUrls = useCallback(() => {
    setUrls((m) => {
      for (const u of m.values()) URL.revokeObjectURL(u)
      return new Map()
    })
    blobs.current.clear()
  }, [])

  const recarregarLista = useCallback(() => {
    bd.listarProjectos()
      .then(setLista)
      .catch(() => setLista([]))
  }, [])

  const carregarBlobs = useCallback(
    async (p: Projecto) => {
      for (const f of p.fontes) {
        const b = await bd.lerFonte(f.id).catch(() => null)
        if (b) registarUrl(f.id, b)
      }
    },
    [registarUrl],
  )

  const abrir = useCallback(
    async (id: string) => {
      setACarregar(true)
      try {
        const r = await bd.lerProjecto(id)
        if (!r) return false
        largarUrls()
        setHistorico({ passado: r.passado ?? [], presente: r.projecto, futuro: r.futuro ?? [], ultimaChave: null })
        setGravacao({ guardadoEm: r.alteradoEm, aGuardar: false, erro: false })
        escreverUltimo(id)
        await carregarBlobs(r.projecto)
        return true
      } finally {
        setACarregar(false)
      }
    },
    [carregarBlobs, largarUrls],
  )

  // Ao montar: reabre o último projecto deste browser.
  useEffect(() => {
    recarregarLista()
    const id = lerUltimo()
    if (!id) {
      setACarregar(false)
      return
    }
    abrir(id)
      .then((ok) => {
        if (!ok) escreverUltimo(null)
      })
      .catch(() => setACarregar(false))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => () => largarUrls(), [largarUrls])

  /** Guarda já (usado antes de exportar e ao mudar de projecto). */
  const guardarJa = useCallback(async () => {
    const h = historicoRef.current
    if (!h) return
    sujo.current = false
    setGravacao((g) => ({ ...g, aGuardar: true }))
    try {
      await bd.guardarProjecto({ id: h.presente.id, projecto: h.presente, passado: h.passado, futuro: h.futuro, alteradoEm: Date.now() })
      setGravacao({ guardadoEm: Date.now(), aGuardar: false, erro: false })
    } catch {
      setGravacao((g) => ({ ...g, aGuardar: false, erro: true }))
    }
  }, [])

  useEffect(() => {
    if (!historico || !sujo.current) return
    const id = setTimeout(() => void guardarJa().then(recarregarLista), 800)
    return () => clearTimeout(id)
  }, [historico, guardarJa, recarregarLista])

  const mudar = useCallback((fn: (h: H.Historico) => H.Historico) => {
    setHistorico((h) => {
      if (!h) return h
      const n = fn(h)
      if (n !== h) sujo.current = true
      return n
    })
  }, [])

  const aplicar = useCallback((e: Edicao, chave: string | null = null) => mudar((h) => H.aplicar(h, e, chave)), [mudar])
  const aplicarVarias = useCallback((es: Edicao[]) => mudar((h) => H.aplicarVarias(h, es)), [mudar])
  const desfazer = useCallback(() => mudar(H.desfazer), [mudar])
  const refazer = useCallback(() => mudar(H.refazer), [mudar])

  const guardarFontes = useCallback(
    async (projectoId: string, brutas: FonteEmBruto[]): Promise<Fonte[]> => {
      const out: Fonte[] = []
      for (const b of brutas) {
        const criada = await criarFonte(b.blob, b.nome, b.origem, b.tipo)
        const f = b.gravacao ? { ...criada, gravacao: b.gravacao } : criada
        await bd.guardarFonte({ id: f.id, projectoId, blob: b.blob })
        registarUrl(f.id, b.blob)
        out.push(f)
      }
      return out
    },
    [registarUrl],
  )

  /** Projecto novo a partir de uma gravação (ou vazio, sem fontes). */
  const criar = useCallback(
    async (titulo: string, brutas: FonteEmBruto[]) => {
      if (historicoRef.current && sujo.current) await guardarJa()
      setACarregar(true)
      try {
        largarUrls()
        let p = novoProjecto(titulo)
        const fontes = await guardarFontes(p.id, brutas)
        for (const f of fontes) p = editar(p, { tipo: 'fonte', fonte: f })
        const principal = fontes.find((f) => f.tipo !== 'audio')
        if (principal?.largura && principal.altura) p = editar(p, { tipo: 'formato', largura: principal.largura, altura: principal.altura, fps: 30 })
        p = montarInicial(p)
        sujo.current = true
        setHistorico(H.iniciar(p))
        escreverUltimo(p.id)
        await bd.guardarProjecto({ id: p.id, projecto: p, passado: [], futuro: [], alteradoEm: Date.now() })
        setGravacao({ guardadoEm: Date.now(), aGuardar: false, erro: false })
        sujo.current = false
        recarregarLista()
      } finally {
        setACarregar(false)
      }
    },
    [guardarFontes, guardarJa, largarUrls, recarregarLista],
  )

  /** Acrescenta fontes ao projecto aberto (importar). Devolve as criadas. */
  const acrescentar = useCallback(
    async (brutas: FonteEmBruto[]): Promise<Fonte[]> => {
      const h = historicoRef.current
      if (!h) return []
      const fontes = await guardarFontes(h.presente.id, brutas)
      mudar((x) => fontes.reduce((acc, f) => H.aplicar(acc, { tipo: 'fonte', fonte: f }), x))
      return fontes
    },
    [guardarFontes, mudar],
  )

  /**
   * Abre no editor uma gravação da biblioteca. Se um projecto deste
   * dispositivo já tem essa gravação como fonte, reabre-o; senão descarrega o
   * ficheiro (`descarregar`) e cria um projecto novo com ele.
   */
  const abrirGravacao = useCallback(
    async (gravacao: { id: string; titulo: string }, descarregar: () => Promise<Blob>, nomeDaFonte: string) => {
      const existentes = await bd.listarProjectos().catch(() => [] as bd.RegistoDeProjecto[])
      const ja = existentes.find((r) => r.projecto.fontes.some((f) => f.gravacao === gravacao.id))
      if (ja) {
        if (historicoRef.current && sujo.current) await guardarJa()
        return abrir(ja.id)
      }
      setACarregar(true)
      let blob: Blob
      try {
        blob = await descarregar()
      } finally {
        setACarregar(false)
      }
      await criar(gravacao.titulo, [{ blob, nome: nomeDaFonte, origem: 'biblioteca', gravacao: gravacao.id }])
      return true
    },
    [abrir, criar, guardarJa],
  )

  const apagar = useCallback(
    async (id: string) => {
      await bd.apagarProjecto(id)
      if (historicoRef.current?.presente.id === id) {
        largarUrls()
        setHistorico(null)
        escreverUltimo(null)
      }
      recarregarLista()
    },
    [largarUrls, recarregarLista],
  )

  const lerBlob = useCallback(async (fonteId: string): Promise<Blob> => {
    const m = blobs.current.get(fonteId)
    if (m) return m
    const b = await bd.lerFonte(fonteId)
    if (!b) throw new Error('fonte em falta no dispositivo')
    blobs.current.set(fonteId, b)
    return b
  }, [])

  return {
    historico,
    projecto: historico?.presente ?? null,
    aCarregar,
    gravacao,
    lista,
    urls,
    aplicar,
    aplicarVarias,
    desfazer,
    refazer,
    podeDesfazer: !!historico && H.podeDesfazer(historico),
    podeRefazer: !!historico && H.podeRefazer(historico),
    abrir,
    criar,
    acrescentar,
    abrirGravacao,
    apagar,
    lerBlob,
    guardarJa,
    recarregarLista,
  }
}

export type ProjectoAberto = ReturnType<typeof useProjecto>
