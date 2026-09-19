/**
 * NGolaCloud / Delonix: formas genéricas com os rótulos do produto.
 * Cores de PAPEL (saem no SVG/PNG), como em `paint.ts`.
 */
import type { GroupModule } from './index'
import { GLYPHS } from './glyphs'

const mod: GroupModule = {
  bg: '#ad1017',
  fg: '#ffffff',
  items: {
    meet: { glyph: GLYPHS.video },
    paasApp: { glyph: GLYPHS.app },
    pgManaged: { glyph: GLYPHS.database },
    mysqlManaged: { glyph: GLYPHS.database },
    redisManaged: { glyph: GLYPHS.cache },
    mongoManaged: { glyph: GLYPHS.document },
    qdrantManaged: { glyph: GLYPHS.vector },
    kafkaManaged: { glyph: GLYPHS.stream },
    minio: { glyph: GLYPHS.objectStorage },
    observability: { glyph: GLYPHS.monitoring },
    registry: { glyph: GLYPHS.registry },
    ai: { glyph: GLYPHS.ai },
    dks: { glyph: GLYPHS.cluster },
    delonixNetVpc: { glyph: GLYPHS.cloud },
  },
}

export default mod
