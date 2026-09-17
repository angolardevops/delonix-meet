/**
 * Infra on-prem: servidores, virtualização, armazenamento e rack.
 * Cores de PAPEL (saem no SVG/PNG), como em `paint.ts`.
 */
import type { GroupModule } from './index'
import { GLYPHS } from './glyphs'

const mod: GroupModule = {
  bg: '#5c5c63',
  fg: '#ffffff',
  items: {
    server: { glyph: GLYPHS.server },
    vm: { glyph: GLYPHS.vm },
    hypervisor: { glyph: GLYPHS.hypervisor },
    nas: { glyph: GLYPHS.nas },
    backup: { glyph: GLYPHS.backup },
    workstation: { glyph: GLYPHS.workstation },
    rack: { glyph: GLYPHS.rack },
    datacenter: { glyph: GLYPHS.region },
  },
}

export default mod
