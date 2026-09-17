/**
 * Pesquisa das gravações. Com o servidor: o recurso `recordings` da pesquisa
 * (por fundir). Sem ele: a biblioteca inteira (`GET /api/recordings`), filtrada
 * no browser pelo nome, sala, autor, descrição e etiquetas.
 */
import { recordingsLibrary, type RecordingLibraryItem } from '../../api'
import { displayName } from './recordingView'
import { localSchema } from '../../ui/search/localSchema'
import type { LocalFallback } from '../../ui/search/useResourceSearch'

/** `scope: 'published'` lista as publicadas que a pessoa vê, também as de salas onde não esteve. */
export const recordingsFallbackFor = (scope: 'mine' | 'published'): LocalFallback<RecordingLibraryItem> => ({
  load: (signal) => recordingsLibrary(signal, { scope }),
  source: {
    schema: localSchema(
      'recordings',
      [
        { name: 'title', type: 'text' },
        { name: 'room_code', type: 'text', groupable: true },
        { name: 'uploader', type: 'user' },
        { name: 'status', type: 'enum', options: ['transcribing', 'ready', 'failed'] },
        { name: 'kind', type: 'enum', options: ['meeting', 'training', 'broadcast', 'hybrid'] },
        { name: 'shared_with_me', type: 'bool', groupable: false },
        { name: 'size_bytes', type: 'number', aggregates: ['sum'] },
        { name: 'created_at', type: 'datetime' },
      ],
      [
        { name: 'mine', group: 'owner', filter: [['shared_with_me', 'eq', false]] },
        { name: 'shared_with_me', group: 'owner', filter: [['shared_with_me', 'eq', true]] },
        { name: 'failed', group: 'status', filter: [['status', 'eq', 'failed']] },
        { name: 'today', group: 'period', filter: [['created_at', 'in_period', 'today']] },
        { name: 'this_week', group: 'period', filter: [['created_at', 'in_period', 'this_week']] },
        { name: 'this_month', group: 'period', filter: [['created_at', 'in_period', 'this_month']] },
      ],
      { textFields: ['title', 'room_code', 'uploader'], defaultOrder: ['-created_at'] },
    ),
    get: (r, f) => {
      switch (f) {
        case 'title':
          return displayName(r.filename)
        case 'uploader':
          return r.uploader_name
        case 'status':
          return r.status
        case 'shared_with_me':
          return !r.owned
        case 'size_bytes':
          return r.status === 'failed' ? null : r.size_bytes
        default:
          return (r as unknown as Record<string, unknown>)[f]
      }
    },
    text: (r) => `${r.filename} ${r.room_code} ${r.uploader_name} ${r.description} ${r.tags.join(' ')}`,
  },
})

export const recordingsFallback = recordingsFallbackFor('mine')
