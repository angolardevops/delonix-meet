/**
 * Google Cloud: forma genérica com a cor de identificação e o nome do serviço.
 * NÃO é o ícone oficial (sem concessão escrita de redistribuição).
 * Cores de PAPEL (saem no SVG/PNG), como em `paint.ts`.
 */
import type { GroupModule } from './index'
import { GLYPHS } from './glyphs'

const mod: GroupModule = {
  bg: '#4285f4',
  fg: '#ffffff',
  items: {
    computeEngine: { glyph: GLYPHS.compute },
    cloudRun: { glyph: GLYPHS.container },
    cloudFunctions: { glyph: GLYPHS.serverless },
    gke: { glyph: GLYPHS.cluster },
    appEngine: { glyph: GLYPHS.web },
    cloudStorage: { glyph: GLYPHS.objectStorage },
    cloudSql: { glyph: GLYPHS.database },
    spanner: { glyph: GLYPHS.database },
    firestore: { glyph: GLYPHS.document },
    bigquery: { glyph: GLYPHS.analytics },
    memorystore: { glyph: GLYPHS.cache },
    pubsub: { glyph: GLYPHS.topic },
    cloudCdn: { glyph: GLYPHS.globe },
    loadBalancing: { glyph: GLYPHS.loadBalancer },
    apigee: { glyph: GLYPHS.apiGateway },
    cloudDns: { glyph: GLYPHS.dns },
    iam: { glyph: GLYPHS.identity },
    monitoring: { glyph: GLYPHS.monitoring },
    secretManager: { glyph: GLYPHS.secret },
    cloudArmor: { glyph: GLYPHS.shield },
    region: { glyph: GLYPHS.region },
    vpc: { glyph: GLYPHS.cloud },
    subnet: { glyph: GLYPHS.subnet },
  },
}

export default mod
