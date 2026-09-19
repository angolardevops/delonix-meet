# Quadros de diagramas — decisão sobre ícones de cloud

Decidido a 2026-09-17, antes de embutir qualquer forma. Cópia de trabalho em
`notas-ui-template/quadros-formas-licencas.md`.

## Pergunta

O dono do produto pediu «os componentes que o draw.io já oferece» para cloud.
O draw.io (jgraph/drawio, Apache-2.0) redistribui dentro das bibliotecas de formas
os ícones oficiais dos fornecedores. **A licença Apache do draw.io não cobre esses
ícones**: cada conjunto tem os termos do seu dono. O Delonix Meet é uma aplicação
self-hosted e offline, entregue a terceiros — embutir um ícone é redistribuí-lo.

## Termos lidos

| Conjunto | Termos (lidos a 2026-09-17) | Redistribuir dentro da app? |
|---|---|---|
| AWS Architecture Icons — <https://aws.amazon.com/architecture/icons/> | A página só concede: «We allow customers and partners to use these toolkits and assets to create architecture diagrams». Não há concessão de redistribuição numa ferramenta; as marcas seguem as *AWS Trademark Guidelines* (<https://aws.amazon.com/trademark-guidelines/>). | **Não** — a concessão é para CRIAR diagramas, não para distribuir o conjunto numa aplicação. |
| Azure Architecture Icons — <https://learn.microsoft.com/en-us/azure/architecture/icons/> | «Icon terms»: uso permitido em diagramas de arquitectura, material de formação e documentação; «You can copy, distribute, and display the icons only for the permitted use unless granted explicit permission by Microsoft. Microsoft reserves all other rights.» Não se recorta, roda nem distorce. | **Não** — distribuir numa aplicação de terceiros não é um uso permitido. |
| Google Cloud icons — <https://cloud.google.com/icons> | A página não deixou ler termos que concedam redistribuição (só a biblioteca para diagramas). Marcas: <https://about.google/brand-resource-center/>. | **Não** — sem concessão escrita e verificada, não se assume. |
| Kubernetes Icons Set — <https://github.com/kubernetes/community/tree/master/icons> | «licensed under a choice of either Apache-2.0 or CC-BY-4.0»; o logótipo Kubernetes é marca da Linux Foundation (<https://www.linuxfoundation.org/trademark-usage/>). | **Sim**, com atribuição — mas ver decisão. |

## Decisão

1. **AWS, Azure e Google Cloud: formas genéricas desenhadas por nós.** Um cartão
   com um glifo genérico do TIPO de recurso (compute, base de dados, fila…) a
   traço, uma faixa com a cor de identificação do fornecedor e o nome do
   serviço como rótulo («Amazon EC2», «Azure Functions», «Cloud Run»). Nenhum
   byte de ícone oficial entra no repositório. O nome é uso nominativo da
   marca para identificar o serviço; a cor não é um logótipo.
2. **Kubernetes: também glifos nossos**, dentro de um heptágono na cor da
   comunidade, com a abreviatura do recurso (`pod`, `deploy`, `svc`…). O
   conjunto oficial PODIA entrar (CC-BY-4.0/Apache-2.0), mas: (a) buscá-lo
   implica descarregar e embutir ficheiros que ninguém reviu aqui; (b) o
   resultado visual ficaria desigual face aos outros grupos, que são
   genéricos; (c) os glifos nossos são ~100 bytes cada, contra 2–6 KB de cada
   SVG oficial. Se o dono do produto quiser o conjunto oficial, entra com
   `web/src/pages/diagrams/catalog/THIRD_PARTY_NOTICES.md` (texto CC-BY-4.0,
   autor «The Kubernetes Authors», link, e nota de alterações).
3. **C4**: as cores são as da convenção C4 (C4-PlantUML, MIT); não há ícones.
4. **NGolaCloud/Delonix**: formas genéricas com os rótulos do produto.
5. **Sem CDN.** Os glifos vivem em módulos TypeScript por grupo, carregados por
   `import()` quando o grupo abre na paleta ou quando um quadro aberto os usa.

## Consequência para o `THIRD_PARTY_NOTICES`

Como nenhum ícone de terceiros é embutido, **não há avisos de terceiros a
acrescentar** nesta entrega. O ficheiro nasce no dia em que entrar o primeiro
ícone com licença própria — e a revisão deve recusar um ícone sem essa linha.
