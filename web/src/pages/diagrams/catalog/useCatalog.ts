import { useSyncExternalStore } from 'react'
import { catalogVersion, subscribeCatalog } from './index'

/** Versão do catálogo carregado: muda quando chega o desenho de um grupo. */
export function useCatalogVersion(): number {
  return useSyncExternalStore(subscribeCatalog, catalogVersion, catalogVersion)
}
