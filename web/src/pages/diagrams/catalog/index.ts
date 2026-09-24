/**
 * Catálogo de formas de arquitectura — índice SÍNCRONO e leve.
 *
 * Aqui só vivem as chaves (texto): que grupos há, que itens tem cada um, e
 * quais são contentores. O desenho (glifo e cores) de cada grupo vive num
 * módulo à parte, carregado por `import()` só quando o grupo abre na paleta ou
 * quando um quadro aberto o usa — assim o catálogo não entra no bundle inicial
 * nem no chunk do editor. Os rótulos vivem na área de locale
 * `diagramCatalog`, carregada com o primeiro grupo.
 *
 * Nenhum ícone oficial de fornecedor: ver docs/quadros/quadros-formas-licencas.md.
 */

export type CatalogGroup = 'cloud' | 'aws' | 'azure' | 'gcp' | 'k8s' | 'network' | 'onprem' | 'platform'

export const CATALOG_GROUPS: CatalogGroup[] = ['cloud', 'aws', 'azure', 'gcp', 'k8s', 'network', 'onprem', 'platform']

export const CATALOG_ITEMS: Record<CatalogGroup, string[]> = {
  cloud: ['compute', 'container', 'serverless', 'objectStorage', 'blockStorage', 'database', 'cache', 'queue', 'stream', 'cdn', 'loadBalancer', 'apiGateway', 'dns', 'firewall', 'waf', 'identity', 'monitoring', 'secret', 'region', 'zone', 'vpc', 'subnet'],
  aws: ['ec2', 'lambda', 'ecs', 'eks', 'fargate', 's3', 'ebs', 'rds', 'aurora', 'dynamodb', 'elasticache', 'sqs', 'sns', 'kinesis', 'cloudfront', 'elb', 'apiGateway', 'route53', 'iam', 'cognito', 'cloudwatch', 'secretsManager', 'waf', 'region', 'vpc', 'subnet'],
  azure: ['vm', 'functions', 'aks', 'containerApps', 'appService', 'blob', 'sqlDatabase', 'cosmosDb', 'redis', 'serviceBus', 'eventHubs', 'frontDoor', 'appGateway', 'loadBalancer', 'apim', 'dns', 'entraId', 'monitor', 'keyVault', 'firewall', 'region', 'vnet', 'subnet'],
  gcp: ['computeEngine', 'cloudRun', 'cloudFunctions', 'gke', 'appEngine', 'cloudStorage', 'cloudSql', 'spanner', 'firestore', 'bigquery', 'memorystore', 'pubsub', 'cloudCdn', 'loadBalancing', 'apigee', 'cloudDns', 'iam', 'monitoring', 'secretManager', 'cloudArmor', 'region', 'vpc', 'subnet'],
  k8s: ['pod', 'deployment', 'statefulset', 'daemonset', 'job', 'cronjob', 'service', 'ingress', 'configmap', 'secret', 'pvc', 'pv', 'hpa', 'serviceAccount', 'node', 'namespace', 'cluster'],
  network: ['internet', 'router', 'switch', 'firewall', 'loadBalancer', 'vpn', 'waf', 'proxy', 'dnsServer', 'mail'],
  onprem: ['server', 'vm', 'hypervisor', 'nas', 'backup', 'workstation', 'rack', 'datacenter'],
  platform: ['meet', 'paasApp', 'pgManaged', 'mysqlManaged', 'redisManaged', 'mongoManaged', 'qdrantManaged', 'kafkaManaged', 'minio', 'observability', 'registry', 'ai', 'dks', 'delonixNetVpc'],
}

/** Itens que são CONTENTORES (VPC, subnet, região, namespace, rack, cluster DKS…). */
export const CATALOG_CONTAINERS: ReadonlySet<string> = new Set([
  'cloud.region',
  'cloud.zone',
  'cloud.vpc',
  'cloud.subnet',
  'aws.region',
  'aws.vpc',
  'aws.subnet',
  'azure.region',
  'azure.vnet',
  'azure.subnet',
  'gcp.region',
  'gcp.vpc',
  'gcp.subnet',
  'k8s.namespace',
  'k8s.cluster',
  'k8s.node',
  'onprem.rack',
  'onprem.datacenter',
  'platform.dks',
  'platform.delonixNetVpc',
])

export const catalogKey = (group: CatalogGroup, item: string) => `${group}.${item}`

export function catalogGroupOf(key: string | undefined): CatalogGroup | null {
  const g = (key ?? '').split('.')[0] as CatalogGroup
  return CATALOG_GROUPS.includes(g) ? g : null
}

export function isCatalogKey(key: string | undefined): boolean {
  const g = catalogGroupOf(key)
  return !!g && CATALOG_ITEMS[g].includes((key ?? '').slice(g.length + 1))
}

// ---------------------------------------------------------------------------
//  Desenho carregado por grupo
// ---------------------------------------------------------------------------

export interface Visual {
  /** Caminho SVG no viewBox 24×24 (vazio quando o cartão mostra a abreviatura). */
  glyph: string
  /** Cor de identificação do fornecedor/grupo (fundo do mosaico). */
  bg: string
  /** Cor do glifo sobre o mosaico. */
  fg: string
  /** Abreviatura (Kubernetes: `pod`, `deploy`…), desenhada no lugar do glifo. */
  abbr?: string
  /** Mosaico em heptágono (Kubernetes) em vez de quadrado. */
  heptagon?: boolean
}

export interface GroupModule {
  bg: string
  fg: string
  heptagon?: boolean
  items: Record<string, { glyph?: string; abbr?: string }>
}

const LOADERS: Record<CatalogGroup, () => Promise<{ default: GroupModule }>> = {
  cloud: () => import('./cloud'),
  aws: () => import('./aws'),
  azure: () => import('./azure'),
  gcp: () => import('./gcp'),
  k8s: () => import('./k8s'),
  network: () => import('./network'),
  onprem: () => import('./onprem'),
  platform: () => import('./platform'),
}

const visuals = new Map<string, Visual>()
const pending = new Map<CatalogGroup, Promise<void>>()
const listeners = new Set<() => void>()
let version = 0

/** Avisa quem desenha que chegou algo novo do catálogo (desenho ou rótulos). */
export function notifyCatalog() {
  bump()
}

function bump() {
  version++
  for (const l of listeners) l()
}

export function subscribeCatalog(l: () => void): () => void {
  listeners.add(l)
  return () => listeners.delete(l)
}

export const catalogVersion = () => version

export function isGroupLoaded(g: CatalogGroup): boolean {
  return visuals.has(catalogKey(g, CATALOG_ITEMS[g][0]))
}

/** Carrega o desenho de um grupo (uma vez; pedidos repetidos partilham a promessa). */
export function loadCatalogGroup(g: CatalogGroup): Promise<void> {
  if (isGroupLoaded(g)) return Promise.resolve()
  let p = pending.get(g)
  if (!p) {
    p = LOADERS[g]().then(({ default: mod }) => {
      for (const [item, v] of Object.entries(mod.items)) {
        visuals.set(catalogKey(g, item), { glyph: v.glyph ?? '', abbr: v.abbr, bg: mod.bg, fg: mod.fg, heptagon: mod.heptagon })
      }
      pending.delete(g)
      bump()
    })
    pending.set(g, p)
  }
  return p
}

/** Garante os grupos usados por um conjunto de chaves (ao abrir ou exportar um quadro). */
export async function ensureCatalog(keys: Iterable<string | undefined>): Promise<void> {
  const groups = new Set<CatalogGroup>()
  for (const k of keys) {
    const g = catalogGroupOf(k)
    if (g) groups.add(g)
  }
  await Promise.all([...groups].map(loadCatalogGroup))
}

export function catalogVisual(key: string | undefined): Visual | undefined {
  return key ? visuals.get(key) : undefined
}
