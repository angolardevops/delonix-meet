/**
 * Lista paginada de organizações, o painel de detalhe que abre ao clicar
 * numa linha, e a gravação do formulário de quotas (a rota mais antiga das
 * três — seats/concurrency seguem o mesmo padrão e não se repetem aqui).
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import '../i18n'
import { OperatorOrgDetail, OperatorOrgPage, OperatorOrgSummary } from '../api'
import Tenants from './Tenants'

vi.mock('../api', async () => {
  const actual = await vi.importActual<typeof import('../api')>('../api')
  return { ...actual, listTenants: vi.fn(), getTenant: vi.fn(), saveTenantQuotas: vi.fn() }
})

const { listTenants, getTenant, saveTenantQuotas } = await import('../api')

const org: OperatorOrgSummary = {
  id: 'org-1',
  name: 'Acme',
  slug: 'acme',
  domain: 'acme.ao',
  created_at: '2026-01-01T00:00:00Z',
  member_count: 7,
  max_groups: 10,
  max_rooms: null,
  max_meetings: 50,
  max_storage_bytes: null,
  max_seats: 20,
  max_concurrent_participants: null,
}

const page: OperatorOrgPage = { items: [org], next_page_token: null }

const detail: OperatorOrgDetail = {
  org,
  seats: { used: 3, active_this_month: 2, inactive: 1, inactive_days: 30, owner_missing: false, limit: 20, available: 17 },
  storage: { org_id: 'org-1', recordings: { bytes: 0, count: 0 }, whiteboards: { bytes: 0, count: 0 }, used_bytes: 0, max_storage_bytes: null, remaining_bytes: null },
}

afterEach(() => {
  cleanup()
  vi.mocked(listTenants).mockReset()
  vi.mocked(getTenant).mockReset()
  vi.mocked(saveTenantQuotas).mockReset()
})

describe('Tenants', () => {
  it('a lista renderiza, clicar abre o detalhe, e guardar as quotas mostra "Guardado"', async () => {
    vi.mocked(listTenants).mockResolvedValue(page)
    vi.mocked(getTenant).mockResolvedValue(detail)
    vi.mocked(saveTenantQuotas).mockResolvedValue({ ...org, max_groups: 15 })

    render(<Tenants />)

    // A lista.
    await waitFor(() => expect(screen.getByText('Acme')).toBeInTheDocument())
    expect(screen.getByText('acme.ao')).toBeInTheDocument()

    // Clicar na linha abre o detalhe.
    fireEvent.click(screen.getByTestId('tenant-row-acme'))
    await waitFor(() => expect(getTenant).toHaveBeenCalledWith('org-1', expect.anything()))
    await waitFor(() => expect(screen.getByText('Quotas do plano')).toBeInTheDocument())

    // Guardar as quotas.
    const groupsInput = screen.getByLabelText('Máx. grupos') as HTMLInputElement
    fireEvent.change(groupsInput, { target: { value: '15' } })
    fireEvent.click(screen.getAllByText('Guardar')[0])

    await waitFor(() => expect(saveTenantQuotas).toHaveBeenCalledTimes(1))
    expect(saveTenantQuotas).toHaveBeenCalledWith(
      'org-1',
      expect.objectContaining({ max_groups: 15, max_rooms: null, max_meetings: 50 }),
    )
    await waitFor(() => expect(screen.getByText('Guardado.')).toBeInTheDocument())
  })
})
