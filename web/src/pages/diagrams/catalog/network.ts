/**
 * Rede e segurança: equipamento genérico.
 * Cores de PAPEL (saem no SVG/PNG), como em `paint.ts`.
 */
import type { GroupModule } from './index'
import { GLYPHS } from './glyphs'

const mod: GroupModule = {
  bg: '#1e7a4a',
  fg: '#ffffff',
  items: {
    internet: { glyph: GLYPHS.globe },
    router: { glyph: GLYPHS.router },
    switch: { glyph: GLYPHS.switch },
    firewall: { glyph: GLYPHS.firewall },
    loadBalancer: { glyph: GLYPHS.loadBalancer },
    vpn: { glyph: GLYPHS.vpn },
    waf: { glyph: GLYPHS.shield },
    proxy: { glyph: GLYPHS.proxy },
    dnsServer: { glyph: GLYPHS.dns },
    mail: { glyph: GLYPHS.mail },
  },
}

export default mod
