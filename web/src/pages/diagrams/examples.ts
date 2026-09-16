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

export function hasExample(n: Notation): boolean {
  return n === 'uml' || n === 'bpmn'
}

export function example(n: Notation, tx: ExampleText): Pick<DiagramDoc, 'nodes' | 'edges'> | null {
  if (n === 'uml') return uml(tx)
  if (n === 'bpmn') return bpmn(tx)
  return null
}

function uml(tx: ExampleText): Pick<DiagramDoc, 'nodes' | 'edges'> {
  return {
    nodes: [
      N('x_t1', 'text', 18, 12, tx('uml.tituloClasses'), {}, [520, 26]),
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
      N('x_t2', 'text', 18, 452, tx('uml.tituloSequencia'), {}, [520, 26]),
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
    { id: 'x_la', name: tx('bpmn.formadora'), size: 190 },
    { id: 'x_lb', name: tx('bpmn.tecnico'), size: 170 },
    { id: 'x_lc', name: tx('bpmn.plataforma'), size: 170 },
  ]
  const top = [44, 44 + 190, 44 + 360]
  return {
    nodes: [
      N('x_title', 'text', 18, 8, tx('bpmn.titulo'), {}, [520, 26]),
      N('x_pool', 'pool', 18, 44, 'Delonix Meet', { lanes }, [1060, 530]),
      N('x_s', 'startEvent', 96, top[0] + 60, tx('bpmn.agendada'), { trigger: 'message' }),
      N('x_v', 'task', 160, top[0] + 46, tx('bpmn.verificar'), { taskKind: 'user' }, [124, 64]),
      N('x_g', 'gateway', 320, top[0] + 53, tx('bpmn.tipo'), { gatewayKind: 'exclusive' }),
      N('x_m', 'task', 410, top[0] + 10, tx('bpmn.reuniao'), { taskKind: 'service' }, [130, 54]),
      N('x_o', 'task', 410, top[0] + 100, tx('bpmn.estudio'), { taskKind: 'service', implementation: 'meet.studio.open', emphasis: true }, [130, 64]),
      N('x_emit', 'task', 590, top[0] + 46, tx('bpmn.emitir'), { taskKind: 'service', multiInstance: 'parallel' }, [140, 64]),
      N('x_timer', 'intermediateEvent', 766, top[0] + 60, tx('bpmn.fimTempo'), { trigger: 'timer' }),
      N('x_close', 'task', 836, top[0] + 46, tx('bpmn.encerrar'), { taskKind: 'service' }, [130, 64]),
      N('x_end', 'endEvent', 1010, top[0] + 60, ''),
      N('x_watch', 'task', 590, top[1] + 50, tx('bpmn.vigiar'), { taskKind: 'user' }, [140, 64]),
      N('x_low', 'intermediateEvent', 766, top[1] + 64, tx('bpmn.bitrate'), { trigger: 'signal' }),
      N('x_reduce', 'task', 836, top[1] + 50, tx('bpmn.reduzir'), { taskKind: 'service' }, [130, 64]),
      N('x_end2', 'endEvent', 1010, top[1] + 64, ''),
      N('x_trans', 'task', 836, top[2] + 50, tx('bpmn.transcodificar'), { taskKind: 'script' }, [130, 64]),
      N('x_ready', 'endEvent', 1010, top[2] + 64, 'recording.ready', { trigger: 'message' }),
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
