/**
 * Pesquisa das gravações. Com o servidor: o recurso `recordings` do contrato
 * (título, transcrição, categoria, duração…). Sem ele: a biblioteca de sempre
 * (`GET /api/recordings`, inteira) com os campos que ela traz — sem
 * transcrição, sem categoria, sem duração.
 */
import { recordingsLibrary, RecordingItem } from '../../api'
import { displayName } from './recordingView'
import { localSchema } from '../../ui/search/localSchema'
import type { LocalFallback } from '../../ui/search/useResourceSearch'

export const recordingsFallback: LocalFallback<RecordingItem> = {
  load: (signal) => recordingsLibrary(signal),
  source: {
    schema: localSchema(
      'recordings',
      [
        { name: 'title', type: 'text' },
        { name: 'room_code', type: 'text', groupable: true },
        { name: 'uploader', type: 'user' },
        { name: 'status', type: 'enum', options: ['ready', 'failed'] },
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
          return r.status === 'failed' ? 'failed' : 'ready'
        case 'shared_with_me':
          return !r.owned
        case 'size_bytes':
          return r.status === 'failed' ? null : r.size_bytes
        default:
          return (r as unknown as Record<string, unknown>)[f]
      }
    },
    text: (r) => `${r.filename} ${r.room_code} ${r.uploader_name}`,
  },
}
