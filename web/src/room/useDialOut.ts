import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  createDialOut, currentUser, DialOut, Extension, hangupDialOut, isAbort, listDialOuts,
  listExtensions, myOrgs,
} from '../api'
import { chaveDoErro, estaVivo, fundir, maisRecentes } from './dialOut'

/** Um ramal que se pode chamar, com o nome da organização (só aparece se houver mais de uma). */
export interface Chamavel {
  ext: Extension
  org: string
}

const POLL_MS = 2000

/**
 * «Ligar a…» um ramal a partir da sala: escolhe-se o ramal, toca, e quem atende
 * entra na sala. O estado de cada pedido lê-se por `GET` (não há ainda um evento
 * do servidor), por isso, com o diálogo aberto, consulta-se de 2 em 2 s.
 */
export function useDialOut(code: string) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [chamaveis, setChamaveis] = useState<Chamavel[]>([])
  const [carga, setCarga] = useState<'idle' | 'a-carregar' | 'pronta' | 'sem-acesso' | 'erro'>('idle')
  const [items, setItems] = useState<DialOut[]>([])
  const [busyId, setBusyId] = useState<string | null>(null)
  const [status, setStatus] = useState<{ tone: 'success' | 'danger'; text: string } | null>(null)
  const [erroLista, setErroLista] = useState<string | null>(null)
  const vivo = useRef(false)
  vivo.current = items.some((i) => estaVivo(i.status))
  // Um GET de cada vez, e o mais recente ganha: com a rede lenta os pedidos não se sobrepõem.
  const emVoo = useRef(false)

  const erro = useCallback(
    (e: unknown, fallbackKey: string) => {
      // Nunca o texto do servidor (está numa só língua): a chave conhecida ou a genérica.
      const chave = chaveDoErro(e)
      return t(chave ?? fallbackKey)
    },
    [t],
  )

  // Os ramais: só quem é administrador de uma organização os pode listar.
  useEffect(() => {
    if (!open) return
    const ac = new AbortController()
    setCarga('a-carregar')
    void (async () => {
      try {
        const orgs = (await myOrgs(ac.signal)).filter((o) => o.role === 'admin')
        if (orgs.length === 0) {
          setCarga('sem-acesso')
          return
        }
        const me = currentUser()?.id
        const listas = await Promise.all(orgs.map((o) => listExtensions(o.id, ac.signal).then((l) => l.map((ext) => ({ ext, org: o.name })))))
        setChamaveis(listas.flat().filter((c) => c.ext.active && c.ext.member_id !== me))
        setCarga('pronta')
      } catch (e) {
        if (isAbort(e)) return
        setCarga('erro')
      }
    })()
    return () => ac.abort()
  }, [open])

  const recarregar = useCallback(
    async (signal?: AbortSignal) => {
      if (emVoo.current) return
      emVoo.current = true
      try {
        const r = await listDialOuts(code, signal)
        setItems((prev) => maisRecentes(fundir(prev, r.items), 10))
        setErroLista(null)
      } catch (e) {
        if (!isAbort(e)) setErroLista(erro(e, 'room.ligar.erro.lista'))
      } finally {
        emVoo.current = false
      }
    },
    [code, erro],
  )

  // Com o diálogo aberto acompanha-se o estado: de 2 em 2 s enquanto algo está vivo.
  useEffect(() => {
    if (!open) return
    const ac = new AbortController()
    void recarregar(ac.signal)
    const id = setInterval(() => {
      if (vivo.current) void recarregar(ac.signal)
    }, POLL_MS)
    return () => {
      ac.abort()
      clearInterval(id)
    }
  }, [open, recarregar])

  async function ligar(c: Chamavel) {
    if (busyId) return
    setBusyId(c.ext.id)
    setStatus(null)
    try {
      const d = await createDialOut(code, c.ext.id)
      setItems((prev) => maisRecentes([d, ...prev.filter((p) => p.id !== d.id)], 10))
    } catch (e) {
      setStatus({ tone: 'danger', text: erro(e, 'room.ligar.erro.ligar') })
    } finally {
      setBusyId(null)
    }
  }

  async function desligar(d: DialOut) {
    if (busyId) return
    setBusyId(d.id)
    setStatus(null)
    try {
      const r = await hangupDialOut(code, d.id)
      setItems((prev) => prev.map((p) => (p.id === r.id ? r : p)))
    } catch (e) {
      setStatus({ tone: 'danger', text: erro(e, 'room.ligar.erro.desligar') })
    } finally {
      setBusyId(null)
    }
  }

  const q = query.trim().toLowerCase()
  const nomeDe = (c: Chamavel) => c.ext.label || c.ext.member_username || c.ext.extension
  return {
    open,
    show: () => {
      setOpen(true)
      setQuery('')
      setStatus(null)
    },
    close: () => setOpen(false),
    query,
    setQuery,
    carga,
    chamaveis: chamaveis
      .filter((c) => !q || nomeDe(c).toLowerCase().includes(q) || c.ext.extension.includes(q))
      .sort((a, b) => nomeDe(a).localeCompare(nomeDe(b))),
    nomeDe,
    varias: new Set(chamaveis.map((c) => c.org)).size > 1,
    items,
    /** Ramais com um pedido vivo: não se volta a ligar-lhes até acabar. */
    ocupados: new Set(items.filter((i) => estaVivo(i.status) && i.extension_id).map((i) => i.extension_id as string)),
    busyId,
    status,
    erroLista,
    ligar,
    desligar,
  }
}

export type DialOutCtl = ReturnType<typeof useDialOut>
