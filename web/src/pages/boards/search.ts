/**
 * Pesquisa dos quadros. Sem o recurso `whiteboards` no servidor: a lista de
 * sempre (`GET /api/whiteboards`, inteira), que não traz o dono — por isso o
 * filtro «Os meus» e o agrupar por dono só existem com o servidor.
 */
import { listWhiteboards, WhiteboardMeta } from '../../api'
import { localSchema } from '../../ui/search/localSchema'
import type { LocalFallback } from '../../ui/search/useResourceSearch'

export const whiteboardsFallback: LocalFallback<WhiteboardMeta> = {
  load: (signal) => listWhiteboards(signal),
  source: {
    schema: localSchema(
      'whiteboards',
      [
        { name: 'title', type: 'text' },
        { name: 'room_code', type: 'text', groupable: true },
        { name: 'is_public', type: 'bool' },
        { name: 'created_at', type: 'datetime' },
      ],
      [
        { name: 'public', group: 'sharing', filter: [['is_public', 'eq', true]] },
        { name: 'private', group: 'sharing', filter: [['is_public', 'eq', false]] },
        { name: 'this_week', group: 'period', filter: [['created_at', 'in_period', 'this_week']] },
        { name: 'this_month', group: 'period', filter: [['created_at', 'in_period', 'this_month']] },
      ],
      { textFields: ['title', 'room_code'], defaultOrder: ['-created_at'] },
    ),
    get: (b, f) => (b as unknown as Record<string, unknown>)[f],
    text: (b) => `${b.title} ${b.room_code}`,
  },
}
