import { createContext, useContext } from 'react'
import type { OrgSummary, User } from '../api'
import type { Async } from './AsyncSection'

export type NavKey =
  | 'home'
  | 'calendar'
  | 'studio'
  | 'recordings'
  | 'whiteboards'
  | 'directory'
  | 'integrations'
  | 'analytics'
  | 'admin'

export interface ShellApi {
  user: User
  orgs: Async<OrgSummary[]>
  /** Organização activa: a primeira onde a pessoa é admin, senão a primeira. */
  org: OrgSummary | null
  isAdmin: boolean
  navOpen: boolean
  setNavOpen: (open: boolean) => void
  navigate: (k: NavKey) => void
  enterRoom: (code: string, voice?: boolean) => void
  openPalette: () => void
  openSettings: (tab?: 'account' | 'appearance' | 'security' | 'brand') => void
}

export const ShellCtx = createContext<ShellApi | null>(null)

export function useShell(): ShellApi {
  const c = useContext(ShellCtx)
  if (!c) throw new Error('useShell fora do Shell')
  return c
}
