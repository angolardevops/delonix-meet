import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, currentUser, inviteToRoom, isAbort, searchUsers, User } from '../api'

/** Chamar membros da organização para a sala em curso (tocam como chamada). */
export function useInvite(code: string) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<User[]>([])
  const [selected, setSelected] = useState<User[]>([])
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState<{ tone: 'success' | 'danger'; text: string } | null>(null)

  useEffect(() => {
    if (query.trim().length < 2) {
      setResults([])
      return
    }
    let alive = true
    const id = setTimeout(() => {
      void searchUsers(query)
        .then((r) => {
          if (alive) setResults(r)
        })
        .catch((e) => {
          if (!isAbort(e) && alive) setStatus({ tone: 'danger', text: apiErrorMessage(e, t('room.convite.erroPesquisa')) })
        })
    }, 250)
    return () => {
      alive = false
      clearTimeout(id)
    }
  }, [query, t])

  function show() {
    setOpen(true)
    setQuery('')
    setSelected([])
    setStatus(null)
  }

  async function send() {
    if (selected.length === 0 || busy) return
    setBusy(true)
    try {
      const { ringing, offline } = await inviteToRoom(code, selected.map((u) => u.id))
      const partes = [t('room.convite.aChamar', { count: ringing.length })]
      if (offline.length > 0) partes.push(t('room.convite.offline', { count: offline.length }))
      setStatus({ tone: 'success', text: partes.join(' · ') })
      setSelected([])
      setQuery('')
    } catch (e) {
      setStatus({ tone: 'danger', text: apiErrorMessage(e, t('room.convite.erroConvidar')) })
    } finally {
      setBusy(false)
    }
  }

  const me = currentUser()?.id
  return {
    open,
    show,
    close: () => setOpen(false),
    query,
    setQuery,
    results: results.filter((u) => u.id !== me && !selected.some((s) => s.id === u.id)),
    selected,
    select: (u: User) => {
      setSelected((prev) => [...prev, u])
      setQuery('')
    },
    unselect: (id: string) => setSelected((prev) => prev.filter((s) => s.id !== id)),
    busy,
    status,
    send,
  }
}

export type Invite = ReturnType<typeof useInvite>
