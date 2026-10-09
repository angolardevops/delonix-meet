/**
 * `GET /api/operator/v1/nodes` serve dois papéis: lista de nós E detecção de
 * "sou administrador de plataforma" — 403 se não for, 404 se a instalação
 * nem tiver superfície de operador. Os dois casos têm de mostrar ecrãs
 * DIFERENTES (não um "erro genérico" só porque os dois são respostas más).
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import '../i18n'
import { ApiError, MediaNodeList } from '../api'
import Overview from './Overview'

vi.mock('../api', async () => {
  const actual = await vi.importActual<typeof import('../api')>('../api')
  return { ...actual, listNodes: vi.fn() }
})

const { listNodes } = await import('../api')

afterEach(() => {
  cleanup()
  vi.mocked(listNodes).mockReset()
})

describe('Overview', () => {
  it('mostra o ecrã de "sem acesso" num 403', async () => {
    vi.mocked(listNodes).mockRejectedValue(new ApiError(403, null, 'forbidden'))
    render(<Overview />)
    await waitFor(() => expect(screen.getByText('Sem acesso de operador')).toBeInTheDocument())
    expect(screen.queryByText('node-a')).not.toBeInTheDocument()
  })

  it('mostra o ecrã de "sem superfície" num 404, distinto do 403', async () => {
    vi.mocked(listNodes).mockRejectedValue(new ApiError(404, null, 'not found'))
    render(<Overview />)
    await waitFor(() => expect(screen.getByText('Sem superfície de operador')).toBeInTheDocument())
    expect(screen.queryByText('Sem acesso de operador')).not.toBeInTheDocument()
  })

  it('mostra a lista de nós num 200', async () => {
    const data: MediaNodeList = {
      items: [
        {
          node_id: 'n1',
          hostname: 'node-a',
          version: '4.5.0',
          edition: 'oss',
          started_at: '2026-01-01T00:00:00Z',
          last_seen_at: '2026-01-01T00:00:10Z',
          status: 'serving',
          rooms: 2,
          peers: 5,
          ws_connections: 5,
          live_broadcasts: 0,
          load: 0.3,
          peer_capacity: 100,
          accepting_new_rooms: true,
        },
      ],
      serving: 1,
      draining: 0,
      unreachable: 0,
      peers: 5,
    }
    vi.mocked(listNodes).mockResolvedValue(data)
    render(<Overview />)
    await waitFor(() => expect(screen.getByText('node-a')).toBeInTheDocument())
    expect(screen.getByText('30%')).toBeInTheDocument()
  })
})
