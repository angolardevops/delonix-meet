import { describe, expect, it } from 'vitest'
import enCat from '../../locales/en/diagramCatalog'
import frCat from '../../locales/fr/diagramCatalog'
import pt from '../../locales/pt/diagrams'
import ptCat from '../../locales/pt/diagramCatalog'
import zhCat from '../../locales/zh/diagramCatalog'
import { CATALOG_CONTAINERS, CATALOG_GROUPS, CATALOG_ITEMS, catalogKey, catalogVisual, ensureCatalog, isGroupLoaded, loadCatalogGroup } from './catalog'
import { example, EXAMPLES } from './examples'
import { toC4PlantUml } from './exporters'
import { canConnect, CONTAINERS, DEdge, DiagramDoc, DNode, emptyDoc, makeNode, NodeProps, NodeType, PALETTES } from './model'
import { validate } from './validate'

const N = (id: string, type: NodeType, x: number, y: number, name = id, props: NodeProps = {}): DNode => makeNode(type, x, y, name, props, id)
const E = (id: string, type: DEdge['type'], from: string, to: string, extra: Partial<DEdge> = {}): DEdge => ({ id, type, from, to, label: '', ...extra })
const doc = (nodes: DNode[], edges: DEdge[] = []): DiagramDoc => ({ ...emptyDoc('d1', 'Arq', 'arch', '', '2026-09-17T00:00:00Z'), nodes, edges })
const codes = (d: DiagramDoc) => validate(d).map((i) => i.code).sort()
const tx = (k: string) => k.split('.').reduce<unknown>((o, p) => (o as Record<string, unknown>)?.[p], (pt as { exemplos: unknown }).exemplos) as string

describe('arquitectura · catálogo', () => {
  it('os grupos pedidos existem na paleta, pela ordem, e só o genérico abre por omissão', () => {
    const keys = PALETTES.arch.map((g) => g.key)
    expect(keys).toEqual(['c4', 'components', 'links', 'cloud', 'aws', 'azure', 'gcp', 'k8s', 'network', 'onprem', 'platform'])
    for (const g of PALETTES.arch.filter((x) => CATALOG_GROUPS.includes(x.key as never))) expect(!!g.closed).toBe(g.key !== 'cloud')
    const k8s = CATALOG_ITEMS.k8s
    for (const r of ['pod', 'deployment', 'service', 'ingress', 'configmap', 'secret', 'namespace', 'node', 'pvc']) expect(k8s).toContain(r)
    const cloud = CATALOG_ITEMS.cloud
    for (const r of ['compute', 'container', 'serverless', 'objectStorage', 'database', 'cache', 'queue', 'stream', 'cdn', 'loadBalancer', 'apiGateway', 'dns', 'vpc', 'subnet', 'firewall', 'waf', 'identity', 'monitoring', 'secret']) expect(cloud).toContain(r)
  })

  it('cada item tem rótulo nas quatro línguas e desenho no módulo do grupo', async () => {
    const cats = { pt: ptCat, en: enCat, fr: frCat, zh: zhCat } as Record<string, { itens: Record<string, Record<string, string>> }>
    const missing: string[] = []
    for (const g of CATALOG_GROUPS) {
      expect(isGroupLoaded(g)).toBe(false)
      await loadCatalogGroup(g)
      expect(isGroupLoaded(g)).toBe(true)
      for (const item of CATALOG_ITEMS[g]) {
        for (const [l, c] of Object.entries(cats)) if (!c.itens[g]?.[item]?.trim()) missing.push(`${l}:${g}.${item}`)
        const v = catalogVisual(catalogKey(g, item))
        if (!v || !(v.glyph || v.abbr)) missing.push(`visual:${g}.${item}`)
      }
    }
    expect(missing).toEqual([])
    // O que o locale tem a mais também é defeito: um item que ninguém usa.
    for (const g of CATALOG_GROUPS) expect(Object.keys(ptCat.itens[g as keyof typeof ptCat.itens]).sort()).toEqual([...CATALOG_ITEMS[g]].sort())
    for (const k of CATALOG_CONTAINERS) expect(CATALOG_ITEMS[k.split('.')[0] as never] as string[]).toContain(k.split('.')[1])
    await ensureCatalog(['aws.ec2', undefined, 'nada.x'])
  })

  it('contentores do catálogo e do C4 desenham-se por baixo e levam o que têm dentro', () => {
    for (const t of ['c4Boundary', 'c4DeploymentNode', 'resourceGroup'] as NodeType[]) expect(CONTAINERS.has(t)).toBe(true)
    const vpc = PALETTES.arch.find((g) => g.key === 'aws')!.items.find((i) => i.key === 'aws.vpc')!
    expect(vpc.kind === 'node' && vpc.type).toBe('resourceGroup')
  })

  it('a relação C4 exige um elemento C4 numa ponta; recursos ligam-se por chamada', () => {
    const [p, c, r] = [N('p', 'c4Person', 0, 0), N('c', 'c4Container', 0, 0), N('r', 'resource', 0, 0, 'r', { catalog: 'aws.s3' })]
    expect(canConnect('c4Rel', p, c)).toBe(true)
    expect(canConnect('c4Rel', c, r)).toBe(true)
    expect(canConnect('c4Rel', r, N('r2', 'resource', 0, 0))).toBe(false)
    expect(canConnect('sync', r, N('r2', 'resource', 0, 0))).toBe(true)
    expect(canConnect('sync', r, N('g', 'resourceGroup', 0, 0))).toBe(false)
  })
})

describe('arquitectura · validação C4', () => {
  it('relações sem descrição e sem tecnologia entre contentores, e pessoa dentro da fronteira', () => {
    const d = doc(
      [N('b', 'c4Boundary', 0, 0, 'Sistema', { boundaryKind: 'system' }), N('p', 'c4Person', 20, 20, 'Ana'), N('c', 'c4Container', 300, 60, 'API'), N('db', 'c4Container', 300, 200, 'DB', { c4Shape: 'db' })],
      [E('r1', 'c4Rel', 'p', 'c', { label: 'usa', technology: 'HTTPS' }), E('r2', 'c4Rel', 'c', 'db', { technology: 'SQL' })],
    )
    expect(codes(d)).toEqual(['c4PessoaDentroFronteira', 'c4RelSemDescricao'])
    d.edges = d.edges.map((e) => (e.id === 'r2' ? { ...e, label: 'lê', technology: '' } : e))
    expect(codes(d)).toEqual(['c4PessoaDentroFronteira', 'c4RelSemTecnologia'])
    // Uma pessoa numa fronteira de EMPRESA é legítima.
    d.nodes = d.nodes.map((n) => (n.id === 'b' ? { ...n, props: { ...n.props, boundaryKind: 'enterprise' } } : n))
    expect(codes(d)).toEqual(['c4RelSemTecnologia'])
  })

  it('os exemplos de arquitectura são válidos', () => {
    expect(EXAMPLES.arch).toEqual(['c4', 'cloud'])
    for (const v of EXAMPLES.arch) {
      const ex = example(v, tx)!
      expect(validate(doc(ex.nodes, ex.edges))).toEqual([])
    }
  })
})

describe('arquitectura · C4-PlantUML', () => {
  it('macros C4, fronteiras aninhadas pelo desenho e relações com tecnologia', () => {
    const ex = example('c4', tx)!
    const pu = toC4PlantUml(doc(ex.nodes, ex.edges))
    expect(pu).toMatch(/^@startuml arq_c4\n!include <C4\/C4_Container>\ntitle Arq\n/)
    expect(pu).toContain('Person(e_x_host, "Anfitrião", "Marca e conduz reuniões e emissões.")')
    expect(pu).toContain('Person_Ext(e_x_guest, "Convidado"')
    expect(pu).toMatch(/System_Boundary\(e_x_sys, "Delonix Meet"\) \{\n {2}Container\(e_x_web, "Consola web", "React \/ TypeScript"/)
    expect(pu).toContain('  ContainerDb(e_x_db, "PostgreSQL", "PostgreSQL 17"')
    expect(pu).toContain('  ContainerQueue(e_x_bus, "Fila de eventos", "Redis Streams"')
    expect(pu).toContain('System_Ext(e_x_minio, "MinIO"')
    expect(pu).toContain('Rel(e_x_web, e_x_sfu, "Envia média", "WebRTC")')
    expect(pu.trim().endsWith('@enduml')).toBe(true)
    // Chavetas equilibradas.
    expect((pu.match(/\{/g) ?? []).length).toBe((pu.match(/\}/g) ?? []).length)
  })

  it('diagrama cloud: grupos saem como nós de implantação e recursos com o tipo como tecnologia', () => {
    const ex = example('cloud', tx)!
    const pu = toC4PlantUml(doc(ex.nodes, ex.edges), (n) => `T:${n.props.catalog}`)
    expect(pu).toContain('!include <C4/C4_Deployment>')
    expect(pu).toMatch(/Deployment_Node\(e_x_dks, "dks-meet", "Kubernetes 1.31"\) \{\n {6}Deployment_Node\(e_x_ns, "meet", "T:k8s.namespace"\) \{/)
    expect(pu).toContain('Container(e_x_pg, "meet-db", "PostgreSQL 17 · HA")')
    expect(pu).toContain('Container(e_x_ing, "meet-ingress", "T:k8s.ingress")')
    expect(pu).toContain('Rel(e_x_pods, e_x_kafka, "eventos")')
    expect((pu.match(/\{/g) ?? []).length).toBe((pu.match(/\}/g) ?? []).length)
  })
})
