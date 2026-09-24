/**
 * Modelos de exemplo — «Começar de um exemplo» num quadro vazio.
 *
 * São os diagramas do template (domínio Delonix Meet em UML; aula aberta com
 * emissão em BPMN), com a mesma disposição, para quem abre o editor pela
 * primeira vez ver as notações a funcionar e poder mexer. É conteúdo de
 * partida, editável e apagável; não descreve dados do servidor.
 *
 * Os nomes são identificadores de modelo (classes, operações) e ficam em
 * inglês como no template; as frases (notas, tarefas, pistas) vêm traduzidas
 * por quem chama.
 */
import type { DEdge, DiagramDoc, DNode, NodeProps, NodeType, Notation } from './model'
import { makeNode } from './model'

export type ExampleText = (key: string) => string

const N = (id: string, type: NodeType, x: number, y: number, name: string, props: NodeProps = {}, size?: [number, number]): DNode => {
  const n = makeNode(type, x, y, name, props, id)
  return size ? { ...n, w: size[0], h: size[1] } : n
}
const E = (id: string, type: DEdge['type'], from: string, to: string, extra: Partial<DEdge> = {}): DEdge => ({ id, type, from, to, label: '', ...extra })

/** Exemplos de cada notação, pela ordem em que se oferecem (o primeiro é o de `?exemplo=1`). */
export const EXAMPLES: Record<Notation, string[]> = {
  uml: ['uml'],
  bpmn: ['bpmn'],
  arch: ['c4', 'cloud'],
  flow: ['flow'],
  free: [],
}

export function examplesFor(n: Notation): string[] {
  return EXAMPLES[n]
}

export function hasExample(n: Notation): boolean {
  return EXAMPLES[n].length > 0
}

/** Um exemplo pelo nome (`uml`, `bpmn`, `flow`, `c4`, `cloud`). */
export function example(variant: string, tx: ExampleText): Pick<DiagramDoc, 'nodes' | 'edges'> | null {
  if (variant === 'uml') return uml(tx)
  if (variant === 'bpmn') return bpmn(tx)
  if (variant === 'flow') return flow(tx)
  if (variant === 'c4') return c4(tx)
  if (variant === 'cloud') return cloud(tx)
  return null
}

function uml(tx: ExampleText): Pick<DiagramDoc, 'nodes' | 'edges'> {
  return {
    nodes: [
      N('x_t1', 'text', 18, 6, tx('uml.tituloClasses'), {}, [620, 34]),
      N('x_session', 'class', 18, 48, 'Session', {
        stereotype: 'entity',
        package: 'meet.core',
        attributes: ['- id: UUID', '- roomCode: String', '- type: SessionType', '- startsAt: DateTime', '- recordAt4K: Boolean'],
        operations: ['+ start(): void', '+ publishStream(): Stream'],
      }, [212, 0]),
      N('x_recording', 'class', 300, 48, 'Recording', {
        package: 'meet.media',
        attributes: ['- resolution: Enum', '- sizeBytes: Long', '- storage: StorageRef'],
        operations: ['+ openInStudio()'],
      }, [196, 0]),
      N('x_transcript', 'class', 566, 48, 'Transcript', {
        package: 'meet.media',
        stereotype: 'entity',
        attributes: ['- language: Locale', '- confidence: Float'],
        operations: ['+ toCaptions(): VTT'],
        emphasis: true,
      }, [186, 0]),
      N('x_participant', 'class', 18, 232, 'Participant', { attributes: ['- role: Role', '- identity: OdooUser', '- channel: Pstn|WebRTC'] }, [196, 0]),
      N('x_target', 'interface', 300, 232, 'StreamTarget', { operations: ['+ connect(): Health', '+ bitrate(): Int'] }, [196, 0]),
      N('x_rtmp', 'class', 300, 330, 'RtmpTarget', {}, [110, 0]),
      N('x_srv', 'class', 420, 330, 'InternalSrv', {}, [110, 0]),
      N('x_note', 'note', 566, 230, '', { text: tx('uml.nota') }, [206, 80]),
      N('x_t2', 'text', 18, 446, tx('uml.tituloSequencia'), {}, [620, 34]),
      N('x_l1', 'lifeline', 18, 484, 'Participant', { length: 150 }),
      N('x_l2', 'lifeline', 166, 484, 'MeetGateway', { length: 150 }),
      N('x_l3', 'lifeline', 314, 484, 'MediaNode', { length: 150 }),
      N('x_l4', 'lifeline', 462, 484, 'StreamTarget', { length: 150 }),
      N('x_uc', 'boundary', 680, 470, tx('uml.casos'), {}, [350, 180]),
      N('x_actor', 'actor', 700, 520, tx('uml.anfitriao')),
      N('x_uc1', 'usecase', 810, 486 + 14, tx('uml.caso1'), {}, [200, 40]),
      N('x_uc2', 'usecase', 810, 540 + 14, tx('uml.caso2'), {}, [200, 40]),
      N('x_uc3', 'usecase', 810, 594 + 14, tx('uml.caso3'), {}, [200, 40]),
    ],
    edges: [
      E('x_e1', 'composition', 'x_session', 'x_recording', { srcMult: '1', dstMult: '0..*', label: tx('uml.grava') }),
      E('x_e2', 'association', 'x_recording', 'x_transcript', { srcMult: '1', dstMult: '1', label: tx('uml.transcreve') }),
      E('x_e3', 'aggregation', 'x_session', 'x_participant', { dstMult: '1..*', label: tx('uml.participa') }),
      E('x_e4', 'dependency', 'x_recording', 'x_target'),
      E('x_e5', 'realization', 'x_rtmp', 'x_target'),
      E('x_e6', 'realization', 'x_srv', 'x_target'),
      E('x_e7', 'anchor', 'x_note', 'x_transcript'),
      E('x_m1', 'message', 'x_l1', 'x_l2', { label: '1: join(roomCode, sso)', offset: 24 }),
      E('x_m2', 'message', 'x_l2', 'x_l3', { label: '2: allocateMedia()', offset: 62 }),
      E('x_m3', 'message', 'x_l3', 'x_l4', { label: '3: publish(4)', offset: 100 }),
      E('x_m4', 'reply', 'x_l4', 'x_l2', { label: '4: health(bitrate)', offset: 136 }),
      E('x_u1', 'association', 'x_actor', 'x_uc1'),
      E('x_u2', 'association', 'x_actor', 'x_uc2'),
      E('x_u3', 'extend', 'x_uc3', 'x_uc1'),
    ],
  }
}

function bpmn(tx: ExampleText): Pick<DiagramDoc, 'nodes' | 'edges'> {
  const lanes = [
    // Pistas altas como no template: a piscina ocupa a página (≈ 1,27 : 1).
    { id: 'x_la', name: tx('bpmn.formadora'), size: 278 },
    { id: 'x_lb', name: tx('bpmn.tecnico'), size: 278 },
    { id: 'x_lc', name: tx('bpmn.plataforma'), size: 278 },
  ]
  const top = [48, 48 + 278, 48 + 556]
  return {
    nodes: [
      N('x_title', 'text', 18, 4, tx('bpmn.titulo'), {}, [620, 34]),
      N('x_pool', 'pool', 18, 48, 'Delonix Meet', { lanes }, [1060, 834]),
      N('x_s', 'startEvent', 96, top[0] + 60, tx('bpmn.agendada'), { trigger: 'message' }),
      N('x_v', 'task', 160, top[0] + 46, tx('bpmn.verificar'), { taskKind: 'user' }, [124, 64]),
      N('x_g', 'gateway', 320, top[0] + 53, tx('bpmn.tipo'), { gatewayKind: 'exclusive' }),
      N('x_m', 'task', 410, top[0] + 10, tx('bpmn.reuniao'), { taskKind: 'service' }, [130, 54]),
      N('x_o', 'task', 410, top[0] + 100, tx('bpmn.estudio'), { taskKind: 'service', implementation: 'meet.studio.open', emphasis: true }, [130, 64]),
      N('x_emit', 'task', 590, top[0] + 46, tx('bpmn.emitir'), { taskKind: 'service', multiInstance: 'parallel' }, [140, 64]),
      N('x_timer', 'intermediateEvent', 766, top[0] + 60, tx('bpmn.fimTempo'), { trigger: 'timer' }),
      N('x_close', 'task', 836, top[0] + 46, tx('bpmn.encerrar'), { taskKind: 'service' }, [130, 64]),
      N('x_end', 'endEvent', 1010, top[0] + 60, ''),
      N('x_watch', 'task', 590, top[1] + 22, tx('bpmn.vigiar'), { taskKind: 'user' }, [140, 64]),
      N('x_low', 'intermediateEvent', 766, top[1] + 36, tx('bpmn.bitrate'), { trigger: 'signal' }),
      N('x_reduce', 'task', 836, top[1] + 22, tx('bpmn.reduzir'), { taskKind: 'service' }, [130, 64]),
      N('x_end2', 'endEvent', 1010, top[1] + 36, ''),
      N('x_trans', 'task', 836, top[2] + 22, tx('bpmn.transcodificar'), { taskKind: 'script' }, [130, 64]),
      N('x_ready', 'endEvent', 1010, top[2] + 36, 'recording.ready', { trigger: 'message' }),
    ],
    edges: [
      E('x_f1', 'sequenceFlow', 'x_s', 'x_v'),
      E('x_f2', 'sequenceFlow', 'x_v', 'x_g'),
      E('x_f3', 'sequenceFlow', 'x_g', 'x_m', { label: tx('bpmn.fluxoReuniao') }),
      E('x_f4', 'sequenceFlow', 'x_g', 'x_o', { label: tx('bpmn.fluxoEmissao') }),
      E('x_f5', 'sequenceFlow', 'x_o', 'x_emit'),
      E('x_f6', 'sequenceFlow', 'x_m', 'x_close'),
      E('x_f7', 'sequenceFlow', 'x_emit', 'x_timer'),
      E('x_f8', 'sequenceFlow', 'x_timer', 'x_close'),
      E('x_f9', 'sequenceFlow', 'x_close', 'x_end'),
      E('x_f14', 'sequenceFlow', 'x_o', 'x_watch'),
      E('x_f10', 'sequenceFlow', 'x_watch', 'x_low'),
      E('x_f11', 'sequenceFlow', 'x_low', 'x_reduce'),
      E('x_f12', 'sequenceFlow', 'x_reduce', 'x_end2'),
      E('x_f15', 'sequenceFlow', 'x_close', 'x_trans'),
      E('x_f13', 'sequenceFlow', 'x_trans', 'x_ready'),
    ],
  }
}

function flow(tx: ExampleText): Pick<DiagramDoc, 'nodes' | 'edges'> {
  const cx = 300
  return {
    nodes: [
      N('x_title', 'text', 18, 4, tx('flow.titulo'), {}, [620, 34]),
      N('x_start', 'terminator', cx - 70, 60, tx('flow.inicio')),
      N('x_pick', 'manualInput', cx - 75, 140, tx('flow.escolher')),
      N('x_prep', 'preparation', cx - 80, 230, tx('flow.preparar')),
      N('x_dec', 'decision', cx - 60, 320, tx('flow.legendas')),
      N('x_trans', 'predefinedProcess', cx + 120, 330, tx('flow.transcrever')),
      N('x_merge', 'merge', cx - 30, 450, ''),
      N('x_store', 'flowDatabase', cx + 150, 440, 'MinIO'),
      N('x_show', 'display', cx - 75, 540, tx('flow.link')),
      N('x_page', 'offPageConnector', cx - 22, 630, tx('flow.pagina')),
      N('x_note', 'flowAnnotation', cx + 150, 230, '', { text: tx('flow.nota') }, [200, 50]),
    ],
    edges: [
      E('x_f1', 'flow', 'x_start', 'x_pick'),
      E('x_f2', 'flow', 'x_pick', 'x_prep'),
      E('x_f3', 'flow', 'x_prep', 'x_dec'),
      E('x_f4', 'flow', 'x_dec', 'x_trans', { label: tx('flow.nao') }),
      E('x_f5', 'flow', 'x_dec', 'x_merge', { label: tx('flow.sim') }),
      E('x_f6', 'flow', 'x_trans', 'x_merge'),
      E('x_f7', 'flow', 'x_trans', 'x_store'),
      E('x_f8', 'flow', 'x_merge', 'x_show'),
      E('x_f9', 'flow', 'x_show', 'x_page'),
      E('x_n1', 'flowNote', 'x_note', 'x_prep'),
    ],
  }
}

/** C4 · diagrama de contentores do Delonix Meet. */
function c4(tx: ExampleText): Pick<DiagramDoc, 'nodes' | 'edges'> {
  return {
    nodes: [
      N('x_title', 'text', 18, 4, tx('c4.titulo'), {}, [720, 34]),
      N('x_host', 'c4Person', 60, 60, tx('c4.anfitriao'), { description: tx('c4.anfitriaoDesc') }, [190, 150]),
      N('x_guest', 'c4Person', 330, 60, tx('c4.convidado'), { description: tx('c4.convidadoDesc'), external: true }, [190, 150]),
      N('x_sys', 'c4Boundary', 20, 250, 'Delonix Meet', { boundaryKind: 'system' }, [860, 330]),
      N('x_web', 'c4Container', 50, 300, tx('c4.web'), { c4Shape: 'web', technology: 'React / TypeScript', description: tx('c4.webDesc') }, [230, 120]),
      N('x_api', 'c4Container', 330, 300, 'meet-server', { technology: 'Rust / Axum', description: tx('c4.apiDesc'), emphasis: true }, [230, 120]),
      N('x_sfu', 'c4Container', 620, 300, 'SFU', { technology: 'Rust / WebRTC', description: tx('c4.sfuDesc') }, [230, 120]),
      N('x_db', 'c4Container', 190, 450, 'PostgreSQL', { c4Shape: 'db', technology: 'PostgreSQL 17', description: tx('c4.dbDesc') }, [220, 110]),
      N('x_bus', 'c4Container', 480, 450, tx('c4.fila'), { c4Shape: 'queue', technology: 'Redis Streams', description: tx('c4.filaDesc') }, [240, 110]),
      N('x_minio', 'c4System', 940, 300, 'MinIO', { external: true, description: tx('c4.minioDesc') }, [210, 120]),
      N('x_odoo', 'c4System', 940, 460, 'Odoo', { external: true, description: tx('c4.odooDesc') }, [210, 110]),
    ],
    edges: [
      E('x_r1', 'c4Rel', 'x_host', 'x_web', { label: tx('c4.usa'), technology: 'HTTPS' }),
      E('x_r2', 'c4Rel', 'x_guest', 'x_web', { label: tx('c4.entra'), technology: 'HTTPS' }),
      E('x_r3', 'c4Rel', 'x_web', 'x_api', { label: tx('c4.chama'), technology: 'JSON / WebSocket' }),
      E('x_r4', 'c4Rel', 'x_web', 'x_sfu', { label: tx('c4.media'), technology: 'WebRTC' }),
      E('x_r5', 'c4Rel', 'x_api', 'x_db', { label: tx('c4.le'), technology: 'SQL / TLS' }),
      E('x_r6', 'c4Rel', 'x_api', 'x_bus', { label: tx('c4.publica'), technology: 'RESP3' }),
      E('x_r7', 'c4Rel', 'x_sfu', 'x_minio', { label: tx('c4.grava'), technology: 'S3 API' }),
      E('x_r8', 'c4Rel', 'x_api', 'x_odoo', { label: tx('c4.sso'), technology: 'OIDC' }),
    ],
  }
}

/** Cloud · o Delonix Meet em NGolaCloud: DKS, serviços geridos e observabilidade. */
function cloud(tx: ExampleText): Pick<DiagramDoc, 'nodes' | 'edges'> {
  const R = (id: string, catalog: string, x: number, y: number, name: string, props: NodeProps = {}, size?: [number, number]) =>
    N(id, catalog === 'platform.dks' || catalog === 'platform.delonixNetVpc' || catalog === 'k8s.namespace' || catalog === 'cloud.region' ? 'resourceGroup' : 'resource', x, y, name, { catalog, ...props }, size)
  return {
    nodes: [
      N('x_title', 'text', 18, 4, tx('cloud.titulo'), {}, [720, 34]),
      R('x_users', 'network.internet', 20, 70, tx('cloud.utilizadores')),
      R('x_region', 'cloud.region', 250, 50, tx('cloud.regiao'), {}, [900, 520]),
      R('x_lb', 'cloud.loadBalancer', 280, 110, tx('cloud.entrada'), { technology: 'HAProxy / TLS' }),
      R('x_vpc', 'platform.delonixNetVpc', 500, 90, 'meet-prod', { technology: '10.20.0.0/16' }, [620, 450]),
      R('x_dks', 'platform.dks', 520, 140, 'dks-meet', { technology: 'Kubernetes 1.31' }, [380, 380]),
      R('x_ns', 'k8s.namespace', 540, 190, 'meet', {}, [340, 310]),
      R('x_ing', 'k8s.ingress', 560, 240, 'meet-ingress'),
      R('x_svc', 'k8s.service', 560, 320, 'meet-server'),
      R('x_pods', 'k8s.deployment', 560, 400, 'meet-server', { technology: tx('cloud.replicas') }),
      R('x_sfu', 'k8s.statefulset', 560, 470, 'sfu'),
      R('x_pg', 'platform.pgManaged', 930, 200, 'meet-db', { technology: 'PostgreSQL 17 · HA' }),
      R('x_redis', 'platform.redisManaged', 930, 280, 'meet-cache'),
      R('x_kafka', 'platform.kafkaManaged', 930, 360, 'meet-eventos'),
      R('x_minio', 'platform.minio', 930, 440, 'gravacoes'),
      R('x_obs', 'platform.observability', 280, 470, 'Prometheus / Grafana'),
    ],
    edges: [
      E('x_c1', 'sync', 'x_users', 'x_lb', { technology: 'HTTPS' }),
      E('x_c2', 'sync', 'x_lb', 'x_ing'),
      E('x_c3', 'sync', 'x_ing', 'x_svc'),
      E('x_c4', 'sync', 'x_svc', 'x_pods'),
      E('x_c5', 'sync', 'x_pods', 'x_pg', { technology: 'SQL' }),
      E('x_c6', 'sync', 'x_pods', 'x_redis'),
      E('x_c7', 'async', 'x_pods', 'x_kafka', { label: tx('cloud.eventos') }),
      E('x_c8', 'dataFlow', 'x_sfu', 'x_minio', { label: tx('cloud.gravacoes') }),
      E('x_c9', 'dataFlow', 'x_pods', 'x_obs', { label: tx('cloud.metricas') }),
      E('x_c10', 'sync', 'x_users', 'x_sfu', { technology: 'WebRTC' }),
    ],
  }
}
