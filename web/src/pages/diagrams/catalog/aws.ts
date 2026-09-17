/**
 * AWS: forma genérica com a cor de identificação da AWS e o nome do serviço no cartão.
 * NÃO é o ícone oficial (termos: só para criar diagramas, sem redistribuição numa aplicação).
 * Cores de PAPEL (saem no SVG/PNG), como em `paint.ts`.
 */
import type { GroupModule } from './index'
import { GLYPHS } from './glyphs'

const mod: GroupModule = {
  bg: '#ff9900',
  fg: '#232f3e',
  items: {
    ec2: { glyph: GLYPHS.compute },
    lambda: { glyph: GLYPHS.serverless },
    ecs: { glyph: GLYPHS.container },
    eks: { glyph: GLYPHS.cluster },
    fargate: { glyph: GLYPHS.container },
    s3: { glyph: GLYPHS.objectStorage },
    ebs: { glyph: GLYPHS.blockStorage },
    rds: { glyph: GLYPHS.database },
    aurora: { glyph: GLYPHS.database },
    dynamodb: { glyph: GLYPHS.document },
    elasticache: { glyph: GLYPHS.cache },
    sqs: { glyph: GLYPHS.queue },
    sns: { glyph: GLYPHS.notification },
    kinesis: { glyph: GLYPHS.stream },
    cloudfront: { glyph: GLYPHS.globe },
    elb: { glyph: GLYPHS.loadBalancer },
    apiGateway: { glyph: GLYPHS.apiGateway },
    route53: { glyph: GLYPHS.dns },
    iam: { glyph: GLYPHS.identity },
    cognito: { glyph: GLYPHS.identity },
    cloudwatch: { glyph: GLYPHS.monitoring },
    secretsManager: { glyph: GLYPHS.secret },
    waf: { glyph: GLYPHS.shield },
    region: { glyph: GLYPHS.region },
    vpc: { glyph: GLYPHS.cloud },
    subnet: { glyph: GLYPHS.subnet },
  },
}

export default mod
