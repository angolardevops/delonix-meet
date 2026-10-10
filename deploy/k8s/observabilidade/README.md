# A observabilidade do Meet

Fase 4 do [ADR-0020](../../../docs/adr/0020-producao-do-meet-em-meet-ngolacloud-com.md).
Corre **depois** da plataforma: o Loki e o Tempo guardam no MinIO.

```bash
kubectl -n observabilidade create secret generic meet-grafana-credenciais \
  --from-literal=admin-user=admin --from-literal=admin-password="$(openssl rand -base64 24)"
# O Loki e o Tempo escrevem no MinIO com as mesmas credenciais do resto:
kubectl -n ngolacloud-meet get secret meet-minio-credenciais -o yaml \
  | sed 's/namespace: ngolacloud-meet/namespace: observabilidade/' | kubectl apply -f -

bash deploy/k8s/observabilidade/instalar.sh
```

## O que mede o quê

| | Vai por onde | Porquê assim |
|---|---|---|
| **Métricas** | o Prometheus lê o `/metrics` do servidor **directamente** | O servidor já expõe **41 métricas** e, até 2026-10-06, **ninguém as lia** — não havia um único `ServiceMonitor` no repositório. Pôr o colector no meio seria um salto a mais entre o número e quem o lê. |
| **Registos** | Alloy → Loki → MinIO | Retenção de 30 dias, a mesma do backup do Postgres. |
| **Rastos** | servidor → OTel Collector → Tempo → MinIO | O que **não existe de todo**: `grep -i 'otlp\|opentelemetry'` em `server/src` dava zero. Sem rastos, um pedido que atravessa ingress, servidor, Postgres e Redis diagnostica-se a adivinhar. |

## Os alertas

**16 alertas e 2 regras de SLO**, escritos sobre as métricas que o servidor **já emite** —
não sobre um catálogo genérico de Kubernetes. Cada métrica usada foi confrontada com
`server/src/metrics.rs`: **as 15 existem**. Um alerta sobre uma métrica que não existe é
um alerta que nunca dispara, e é assim que se tem painéis verdes com o produto em baixo.

A regra da casa está escrita no ficheiro: **um alerta acorda alguém.** Se não há nada a
fazer com ele às três da manhã, é um painel — e fica como `info`. É o caso do
`MeetQuotaDeOrganizacaoAtingida`: é uma decisão comercial, não uma avaria.

Três que valem a pena conhecer:

- **`MeetTudoPorTurn`** — se quase todas as chamadas passam pelo relay, ou o coturn
  anuncia o IP errado ou as portas UDP estão fechadas. É o sintoma de uma instalação mal
  exposta, e sem este alerta parece só «a rede está lenta».
- **`MeetCpuLimitaAChamada`** — usa o contador do próprio servidor, que sabe quando é
  **ele** a degradar. O CPU do nó pode estar alto por outra coisa qualquer.
- **`MeetAuditoriaAFalhar`** — é `critico` por uma razão que não é óbvia: não se perde
  serviço, perde-se a **prova** do que aconteceu.

## O que isto NÃO faz, e é deliberado

- **Nenhum alerta é encaminhado para ninguém.** O receptor do Alertmanager chama-se
  `ninguem` e está vazio. Encaminhar para email ou Slack precisa de um segredo e de uma
  decisão sobre **quem é acordado** — inventar isso aqui daria alertas que ninguém recebe
  com a aparência de alertas que alguém recebe.
- **O servidor ainda não emite rastos.** O colector está de pé à espera; falta o
  `OTEL_EXPORTER_OTLP_ENDPOINT` e a instrumentação em Rust. É a fase 5, e é código.
- **Nada disto foi instalado.** As retenções, os tamanhos e os limiares são escolhas
  justificadas, não medições.
