/**
 * Azure: forma genérica com a cor de identificação da Microsoft Azure e o nome do serviço.
 * NÃO é o ícone oficial (termos: uso só em diagramas, documentação e formação).
 * Cores de PAPEL (saem no SVG/PNG), como em `paint.ts`.
 */
import type { GroupModule } from './index'
import { GLYPHS } from './glyphs'

const mod: GroupModule = {
  bg: '#0078d4',
  fg: '#ffffff',
  items: {
    vm: { glyph: GLYPHS.compute },
    functions: { glyph: GLYPHS.serverless },
    aks: { glyph: GLYPHS.cluster },
    containerApps: { glyph: GLYPHS.container },
    appService: { glyph: GLYPHS.web },
    blob: { glyph: GLYPHS.objectStorage },
    sqlDatabase: { glyph: GLYPHS.database },
    cosmosDb: { glyph: GLYPHS.document },
    redis: { glyph: GLYPHS.cache },
    serviceBus: { glyph: GLYPHS.queue },
    eventHubs: { glyph: GLYPHS.stream },
    frontDoor: { glyph: GLYPHS.globe },
    appGateway: { glyph: GLYPHS.loadBalancer },
    loadBalancer: { glyph: GLYPHS.loadBalancer },
    apim: { glyph: GLYPHS.apiGateway },
    dns: { glyph: GLYPHS.dns },
    entraId: { glyph: GLYPHS.identity },
    monitor: { glyph: GLYPHS.monitoring },
    keyVault: { glyph: GLYPHS.secret },
    firewall: { glyph: GLYPHS.firewall },
    region: { glyph: GLYPHS.region },
    vnet: { glyph: GLYPHS.cloud },
    subnet: { glyph: GLYPHS.subnet },
  },
}

export default mod
