/**
 * Cloud genérico: formas por tipo de recurso, sem fornecedor.
 * Cores de PAPEL (saem no SVG/PNG), como em `paint.ts`.
 */
import type { GroupModule } from './index'
import { GLYPHS } from './glyphs'

const mod: GroupModule = {
  bg: '#2f5d8a',
  fg: '#ffffff',
  items: {
    compute: { glyph: GLYPHS.compute },
    container: { glyph: GLYPHS.container },
    serverless: { glyph: GLYPHS.serverless },
    objectStorage: { glyph: GLYPHS.objectStorage },
    blockStorage: { glyph: GLYPHS.blockStorage },
    database: { glyph: GLYPHS.database },
    cache: { glyph: GLYPHS.cache },
    queue: { glyph: GLYPHS.queue },
    stream: { glyph: GLYPHS.stream },
    cdn: { glyph: GLYPHS.globe },
    loadBalancer: { glyph: GLYPHS.loadBalancer },
    apiGateway: { glyph: GLYPHS.apiGateway },
    dns: { glyph: GLYPHS.dns },
    firewall: { glyph: GLYPHS.firewall },
    waf: { glyph: GLYPHS.shield },
    identity: { glyph: GLYPHS.identity },
    monitoring: { glyph: GLYPHS.monitoring },
    secret: { glyph: GLYPHS.secret },
    region: { glyph: GLYPHS.region },
    zone: { glyph: GLYPHS.subnet },
    vpc: { glyph: GLYPHS.cloud },
    subnet: { glyph: GLYPHS.subnet },
  },
}

export default mod
