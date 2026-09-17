/**
 * Kubernetes: heptágono na cor da comunidade com a abreviatura do recurso
 * (a mesma que o `kubectl` aceita). Desenhado por nós — o conjunto oficial é
 * CC-BY-4.0/Apache-2.0 mas não é embutido (ver quadros-formas-licencas.md), e o
 * logótipo Kubernetes é marca da Linux Foundation e não aparece aqui.
 * Cores de PAPEL (saem no SVG/PNG), como em `paint.ts`.
 */
import type { GroupModule } from './index'

const mod: GroupModule = {
  bg: '#326ce5',
  fg: '#ffffff',
  heptagon: true,
  items: {
    pod: { abbr: 'pod' },
    deployment: { abbr: 'deploy' },
    statefulset: { abbr: 'sts' },
    daemonset: { abbr: 'ds' },
    job: { abbr: 'job' },
    cronjob: { abbr: 'cronjob' },
    service: { abbr: 'svc' },
    ingress: { abbr: 'ing' },
    configmap: { abbr: 'cm' },
    secret: { abbr: 'secret' },
    pvc: { abbr: 'pvc' },
    pv: { abbr: 'pv' },
    hpa: { abbr: 'hpa' },
    serviceAccount: { abbr: 'sa' },
    node: { abbr: 'node' },
    namespace: { abbr: 'ns' },
    cluster: { abbr: 'k8s' },
  },
}

export default mod
