# A plataforma do cluster de produção do Meet

O que corre **por baixo** da aplicação: entrada, certificados, base de dados, Redis e
ficheiros. A decisão está no [ADR-0020](../../../docs/adr/0020-producao-do-meet-em-meet-ngolacloud-com.md).

```bash
export KUBECONFIG=.kube/producao.conf   # o que o `make prod-k8s` trouxe
bash deploy/k8s/plataforma/instalar.sh
```

## Segredos — e porque é que este script se recusa a inventá-los

Os três Secrets têm de existir **antes**, e o `instalar.sh` pára se faltarem. Não gera
passwords: uma password gerada por um script de instalação é uma password que ninguém
anotou e que ninguém consegue rodar depois.

```bash
NS=ngolacloud-meet
kubectl -n $NS create secret generic meet-pg-credenciais \
  --from-literal=username=delonix --from-literal=password="$(openssl rand -base64 32)"
kubectl -n $NS create secret generic meet-redis-credenciais \
  --from-literal=REDIS_PASSWORD="$(openssl rand -base64 32)"
kubectl -n $NS create secret generic meet-minio-credenciais \
  --from-literal=ACCESS_KEY_ID=meet --from-literal=SECRET_ACCESS_KEY="$(openssl rand -base64 32)"
```

**Guarda-os no teu gestor de segredos antes de os perderes de vista.** O do MinIO é
preciso em dois sítios — o próprio MinIO e o backup do Postgres — e por isso vive no
namespace da aplicação.

## O que fica de pé

| | O quê | Porquê assim |
|---|---|---|
| **Postgres** | CloudNativePG, 3 instâncias, **1 réplica síncrona** | O que custa num Postgres replicado não é levantá-lo, é o **failover**. O operador promove; um StatefulSet deixa-o para quem estiver acordado às três da manhã. |
| **Backup** | WAL contínuo para o MinIO + completo às 02:30, retenção 30 dias | **Réplicas copiam o erro, backups não.** Sem isto, apagar uma tabela replica-se em três sítios à velocidade da luz. |
| **Redis** | Sentinel, 3 nós, **com autenticação** | Guarda presença, sessões e lugares — perdê-lo tira pessoas de salas. A password é nova: hoje não há nenhuma. |
| **MinIO** | 4 nós distribuídos, buckets versionados | Tira as gravações do volume ReadWriteMany, que amarrava o produto a blocos partilhados (D2). |
| **ingress-nginx** | 2 réplicas, `externalTrafficPolicy: Local` | **Preserva o IP de quem chega.** Sem isso, todos os pedidos pareceriam vir do nó e o limite por IP (R282) deixava de limitar. |
| **cert-manager** | `letsencrypt-prod` e `letsencrypt-staging` | O chart já pede o `letsencrypt-prod` pelo nome. O de ensaio existe para não gastar o limite de cinco por semana — esgotá-lo bloqueia sete dias. |
| **Classes de prioridade** | `meet-critico` (10000) e `meet-normal` (1000), ambas com `globalDefault: false` | **O primeiro passo do `instalar.sh`**, porque o `values-production.yaml` refere-lhes os nomes e um `priorityClassName` que não exista faz o API server **recusar o pod**. No cluster partilhado do ADR-0021 é o que decide quem se aguenta sob pressão de nó: o caminho da chamada e os dados ficam em `meet-critico`, o resto degrada. O `globalDefault: false` é obrigatório — a `true`, a classe passaria a ser a prioridade dos pods de **outras equipas**. Os valores são um ponto de partida: só têm significado comparados com as classes dos vizinhos, que não são nossas. |

## O que isto NÃO resolve

- **Com três workers, quatro nós de MinIO não ficam um por nó** — um worker leva dois. O
  erasure coding aguenta perder um nó de MinIO, não aguenta perder o worker que tem dois.
  Com um quarto worker deixa de ser assim.
- **O certificado só nasce quando `meet.ngolacloud.com` apontar para este cluster.** Até
  lá o desafio HTTP-01 não passa e o ingress serve o certificado próprio do nginx. Não é
  avaria: é DNS.
- **Nada disto foi instalado.** Os valores são escolhas justificadas, não medições: as
  memórias do Postgres, o `maxmemory` do Redis e os tamanhos dos discos são pontos de
  partida para a primeira instalação corrigir.
- **A observabilidade ainda não existe** — os `serviceMonitor` destes valores apontam para
  um namespace `observabilidade` que a fase 4 cria. Até lá ficam declarados e sem quem os
  leia, o que é inofensivo.
