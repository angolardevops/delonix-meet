# Quadros de diagramas — inventário de formas por contexto

Medido a 2026-09-17 contra `integra/validacao-l2` (`445e41a`), ficheiros
`web/src/pages/diagrams/{model,shapes,Palette,validate,exporters,examples}.ts*`.
Cópia de trabalho em `notas-ui-template/quadros-formas-inventario.md`.

Legenda de prioridade: **P1** falta que impede desenhar um diagrama normal
dessa notação; **P2** falta que um utilizador de draw.io nota ao fim de minutos;
**P3** completa a especificação, raro no dia-a-dia. «Modelo» = já existe no
`model.ts` mas não aparece na paleta (é só expor e testar).

Todas as formas novas cumprem o mesmo contrato das existentes: arrastar/carregar
para colocar, propriedades no inspector, camadas e desfazer, persistência no
IndexedDB (guardar e reabrir), SVG/PNG, formato próprio do contexto quando faz
sentido, «Procurar elemento» e regras de validação quando as há.

## Linha de base (o que existe)

| Contexto | Paleta hoje | Arestas no modelo | Validação | Exportação |
|---|---|---|---|---|
| UML 2 | Classe, Interface, Enum, Pacote, Nota, Herança, Associação, Composição · Linha de vida, Mensagem, Resposta, Fragmento · Actor, Caso de uso, Inclusão, Fronteira | + agregação, realização, dependência, âncora, extensão (**modelo**, sem item de paleta) | 13 regras | XMI 2.5.1, PlantUML, SVG, PNG, JSON |
| BPMN 2.0 | Início, Mensagem, Temporizador, Fim · Tarefa, Utilizador, Serviço, Subprocesso · 4 gateways · Piscina, Pista, Dados, Anotação | + fluxo de mensagem, associação (**modelo**) · tarefas script/manual/envio/recepção e multi-instância (**modelo**, só pelo inspector) · gatilho sinal (**modelo**) | 16 regras | .bpmn com DI, SVG, PNG, JSON |
| Arquitectura | Serviço, Base de dados, Fila, Cliente, Externo, Zona · Chamada, Assíncrona, Dados, Nota | — | 2 regras | SVG, PNG, JSON |
| Fluxograma | Início/fim, Processo, Decisão, Entrada/saída, Documento, Fluxo | — | 4 regras | SVG, PNG, JSON |
| Livre | Caneta, Borracha, Texto, Nota, 4 cores | — | — | SVG, PNG, JSON |

Exemplos «Começar de um exemplo»: só UML e BPMN.
«Procurar elemento» procura só nome/texto/estereótipo/membros dos elementos já no
quadro — não procura na paleta, nem por tecnologia ou tipo.
Grupos da paleta: fixos, não recolhíveis.

## UML 2.5

| Falta | Face a | Prioridade | Nota de implementação |
|---|---|---|---|
| Agregação, realização, dependência, extensão, generalização em casos de uso | spec, draw.io | **P1** (modelo) | expor na paleta; XMI/PlantUML já saem |
| **Actividade**: nó inicial, final de actividade, final de fluxo, acção, decisão/junção, fork/join, partição (swimlane), nó de objecto, fluxo de controlo com guarda | spec, draw.io «UML» | **P1** | `uml:Activity` no XMI; PlantUML em sintaxe de estados (`<<choice>>`, `<<fork>>`, `[*]`), que aceita um grafo arbitrário |
| **Estados**: estado, estado composto, inicial, final, escolha, histórico (superficial/profundo), transição `evento [guarda] / efeito` | spec, draw.io | **P1** | `uml:StateMachine`/`region`/`Pseudostate` no XMI; `@startuml` de estados |
| **Componentes/implantação**: componente, porto, interface fornecida (lollipop), requerida (socket), nó, artefacto, `«deploy»`, `«manifest»` | spec, draw.io | **P2** | `uml:Component`/`Port`/`Node`/`Artifact`/`Deployment`/`Usage` |
| **Objecto** (`nome: Classe` sublinhado, slots) e ligação | spec, draw.io | **P2** | `uml:InstanceSpecification` com `classifier` quando a classe existe |
| Mensagem própria, mensagem perdida/encontrada, barra de activação | spec, draw.io | **P2** | a própria já é suportada pelo modelo (from = to); perdida/encontrada são arestas novas; activação é nó |
| Operadores `ref`, `neg`, `strict`, `seq`, `ignore`, `consider`, `assert` | spec | **P3** | `alt/opt/loop/par/break/critical` já existem |
| Timing, comunicação, perfil, estrutura composta, visão geral de interacção | spec | P3 | **fora** desta entrega — cada um é um editor à parte |

## BPMN 2.0

| Falta | Face a | Prioridade | Nota |
|---|---|---|---|
| Eventos intermédios catch/throw: mensagem, temporizador, sinal, erro, escalonamento, compensação, condicional, ligação | spec, draw.io «BPMN General» | **P1** | `intermediateCatchEvent` vs `intermediateThrowEvent` |
| Evento de fronteira (interruptor/não interruptor) | spec | **P1** | `boundaryEvent attachedToRef`; a actividade é a que contém o centro, como as pistas |
| Fim tipado: erro, terminar, mensagem, sinal, escalonamento, compensação | spec | **P1** | `errorEventDefinition`, `terminateEventDefinition`, … |
| Tarefas: envio, recepção, manual, script (**modelo**), regra de negócio, actividade de chamada | spec | **P1** | `businessRuleTask`, `callActivity` |
| Marcadores: ciclo, multi-instância (**modelo**), compensação, ad-hoc | spec | **P2** | `standardLoopCharacteristics`, `isForCompensation`, `adHocSubProcess` |
| Gateways: complexo, baseado em eventos exclusivo/paralelo de arranque | spec | **P2** | `complexGateway`, `eventBasedGateway eventGatewayType instantiate` |
| Armazém de dados, entrada/saída de dados | spec | **P2** | `dataStoreReference`; `ioSpecification` do processo |
| Fluxo de mensagem e associação (**modelo**), grupo | spec | **P1**/P2 | `group` sai como artefacto com DI |
| Coreografia, conversação | spec | P3 | **fora**: diagramas de outro tipo (`choreography`), sem regras nem exportadores úteis sem motor |

Validação pedida: início/fim obrigatórios (existe), gateways com saídas
(novo: gateway que nem diverge nem converge; gateway de eventos sem ≥ 2 saídas),
evento de fronteira solto ou com entrada, ligação (link) sem par.

## Fluxograma (ISO 5807 / draw.io «Flowchart»)

| Falta | Prioridade |
|---|---|
| Processo predefinido, preparação, entrada manual, operação manual, atraso, junção (merge), conector na página, conector fora da página, dados armazenados, base de dados, ecrã (display), vários documentos, limite de ciclo, anotação, armazenamento sequencial, armazenamento de acesso directo, armazenamento interno | **P1** para predefinido/preparação/entrada manual/conector/base de dados/anotação; **P2** o resto |

Validação: as regras existentes passam a contar as formas novas; anotação e
conectores não contam como «inalcançáveis» sem ligação obrigatória.

## Arquitectura

Grupos pesquisáveis e **recolhíveis** (hoje não são).

| Grupo | Formas | Prioridade | Nota |
|---|---|---|---|
| **C4** (separado) | Pessoa (interna/externa), Sistema de software (interno/externo), Contentor (aplicação, base de dados, fila, web, móvel), Componente, Código, Fronteira de sistema, Fronteira de contentor, Nó de implantação, Relação com descrição e tecnologia | **P1** | cores da convenção C4; exportação C4-PlantUML; regras: relações com tecnologia, pessoas fora de fronteiras |
| **Cloud genérico** | compute, contentor, serverless, armazenamento, base de dados, cache, fila/stream, CDN, balanceador, API gateway, DNS, VPC, subnet, firewall/WAF, identidade, monitorização, segredo | **P1** | formas nossas |
| **AWS**, **Azure**, **Google Cloud** | os serviços mais usados de cada (compute, contentores, serverless, objecto, SQL, NoSQL, cache, filas, CDN, LB, API, DNS, rede, IAM, segredos, monitorização) | **P2** | ver `quadros-formas-licencas.md`: **formas genéricas nossas** com o nome do serviço e a cor do fornecedor — nenhum ícone oficial embutido |
| **Kubernetes** | pod, deployment, service, ingress, configmap, secret, namespace, node, pvc (+ statefulset, job) | **P1** | glifos nossos no heptágono; o conjunto oficial é CC-BY-4.0/Apache-2.0 mas não se embute (ver licenças) |
| **Rede e segurança** | firewall, router, switch, balanceador, VPN, WAF | **P2** | |
| **On-prem** | servidor, rack, VM, storage/NAS, hipervisor | **P2** | |
| **NGolaCloud / Delonix** | DKS, Postgres gerido, Redis gerido, Kafka gerido, VPC delonix-net, Delonix Meet | **P2** | formas genéricas com rótulos do produto |

Carregamento: os glifos e as cores de cada grupo de catálogo em módulo próprio
(`import()` ao abrir o grupo ou ao abrir um quadro que o usa); os rótulos numa
área de locale carregada com o editor. Medir bundle inicial e chunks.

## Livre

| Falta | Prioridade | Nota |
|---|---|---|
| Marcador (traço largo translúcido) | **P1** | local ao diagrama |
| Formas rápidas: rectângulo, elipse, seta, linha | **P1** | traços com forma, apagáveis pela borracha |
| Laço de selecção e mover | **P2** | selecção múltipla de traços e elementos, mover e apagar |
| Espessura do traço, mais cores, opacidade | **P1** | |
| Post-it com cores | **P1** | nó `sticky`, cor no separador Estilo |
| Sincronização com a sala | — | **fora**: o modelo vive no IndexedDB; não há backend para isso |

## Exemplos

Arquitectura: diagrama de contentores C4 e diagrama cloud. Fluxograma: um processo
com decisão, conector e base de dados. (UML e BPMN já existem.)

## Fora desta entrega, com razão

- Ícones oficiais de AWS/Azure/GCP: termos não permitem redistribuição numa
  aplicação (ver licenças).
- Coreografia/conversação BPMN, diagramas UML de timing/comunicação/perfil.
- Colaboração em tempo real no diagrama: sem `doc JSONB` nem mensagem de sinalização.

## Estado da entrega (2026-09-17, branch `frontend/quadros-formas`)

Entregue tudo o que está acima com prioridade P1 e P2, e os P3 de UML
(operadores de fragmento). Ficou de fora, com a razão já escrita: coreografia e
conversação BPMN, diagramas UML de timing/comunicação/perfil, ícones oficiais
de fornecedores, sincronização do diagrama com a sala.

Prova: `web/e2e/quadros-formas.mjs` (1440×900 e 1920×1080, contra a API) e os
testes `web/src/pages/diagrams/formas-*.test.ts`.

Bundle medido com `npm run build` (bytes; gzip entre parênteses):

| | antes (`445e41a`) | depois |
|---|---|---|
| JS inicial (`index-*.js`) | 483 889 (154 543) | 496 889 (158 431) |
| chunk do editor (`Diagram-*.js`) | 121 632 (36 585) | 216 029 (60 480) |
| grupos do catálogo (8 chunks) | — | 303–773 cada (218–409) |
| glifos partilhados do catálogo | — | 3 173 (1 500) |
| rótulos do catálogo (1 por língua) | — | 3 417–3 539 (1 517–1 737) |

Os +13 KB do inicial são as chaves novas do `pt/diagrams.ts`, que o `i18n.ts`
importa de forma síncrona (o português é a língua de recurso). Os rótulos do
catálogo já ficaram fora disso; mover o resto das chaves dos diagramas para uma
área carregada com o editor é o passo seguinte, e mexe no `i18n.ts`.
