# Plano para levar o Delonix Meet a produção — sprints

**Data:** 2026-10-03 · **Alvo:** `meet.ngolacloud.com` em Kubernetes · **Ramo medido:** `develop`

Este plano junta os achados de cinco auditorias feitas no mesmo dia (deploy, segurança,
frontend, media e produto) e ordena-os por sprints. Cada item diz de onde vem, o que tem de
ficar provado e do que depende. **Um item só fecha com a prova corrida** — «está
implementado» não fecha nada.

## Veredicto de partida

| Auditoria | Veredicto | O que pesou |
|---|---|---|
| Deploy | BLOQUEADO | sete bloqueios: segredos, imagens, gravações, endereços de laboratório, voz, backups, ramos |
| Segurança | pedir alterações (bloqueado pelo caminho `make prod`) | um crítico, dois altos |
| Media | não pronto como cadeia de emissão | sincronismo áudio/vídeo; três capacidades com bandeira e sem código |
| Frontend | não pronto | build embute tudo em `data:`; gravações perdem-se; sem *error boundary* |
| Produto | não anunciar «Estúdio de TV para canais» | saída só RTMP; sem redundância; backend do estúdio sem ecrã |

O que as auditorias **não** cobriram: nenhum browser, nenhuma chamada por uma operadora,
nenhum cluster de produção. Onde um efeito é inferência e não medição, o item pede a medição.

## Como ler

- **Origem:** `DEP` deploy, `SEG` segurança, `FE` frontend, `MED` media, `PRD` produto, `DONO` pedido directo do dono do produto.
- **Tamanho:** P (um a dois dias), M (até uma semana), G (mais de uma semana). É ordem de grandeza, não compromisso: não foi medida a capacidade da equipa.
- A duração de cada sprint fica por fixar com quem a executa. A **ordem** é o que este plano defende.

---

## Sprint 0 — Parar o que sangra (antes de qualquer outra coisa)

Objectivo: nenhum caminho de deploy aplica um segredo público, e existe um ramo de onde sair um release.

| # | Item | Origem | Prova | Tam. |
|---|---|---|---|---|
| 0.1 | Tirar os segredos literais de `deploy/k8s/01-config.yaml` e dos valores Helm antigos; o servidor recusa esses valores ao arrancar, como já recusa os de voz | DEP, SEG | o servidor arrancado com o segredo publicado sai com erro; o portão de higiene falha se o valor voltar | P |
| 0.2 | Unificar os ramos: fundir a `main` no `develop`, resolver os quatro conflitos, correr a bateria inteira no resultado | DEP | `git log origin/develop..origin/main` vazio; CI verde no commit fundido | M |
| 0.3 | Pôr o CI a correr no `develop` (hoje só corre em PR e na `main`) | DEP | uma corrida de CI verde listada para o ramo | P |
| 0.4 | Reparar o portão `check-capability-claims.sh`: procura textos onde eles já não estão e passa sem ler nada | PRD | o portão falha com uma capacidade inventada posta num ficheiro de língua | P |
| 0.5 | Mudar a password exige a actual (ou reautenticação recente) e termina as outras sessões | SEG | sessão sem reautenticação recebe 403; depois da mudança, o token da outra sessão recebe 401 | P |
| 0.6 | O segredo de voz deixa de viajar na query string; as três rotas de máquina dos ramais passam para o listener interno; o hash dos ramais passa a ser guardado selado | SEG | pedido com `?secret=` recusado; as rotas não respondem no listener público | M |

## Sprint 1 — Um pacote que se instala

Objectivo: `helm install` com um ficheiro de valores e um Secret põe o Meet a correr num cluster limpo, com imagens publicadas.

| # | Item | Origem | Prova | Tam. |
|---|---|---|---|---|
| 1.1 | Chart Helm (`deploy/helm/delonix-meet`): servidor, web, ingress, coturn, voz; segredos só por Secret existente | DONO, DEP | `helm lint` e `helm template` verdes; sem Secret, falha com mensagem clara; instalação num cluster limpo com os pods prontos | G |
| 1.2 | O CI constrói, publica por digest e assina as imagens do servidor e da web | DEP | imagem com o SHA do commit no registo, assinatura verificável | M |
| 1.3 | A imagem de produção traz `ffmpeg` e `ffprobe` | DEP | e2e de gravação no servidor verde contra a imagem publicada | P |
| 1.4 | Nenhum endereço de laboratório no que o cluster recebe: host, IP do TURN, domínio WebAuthn, origens CORS, email ACME — tudo por valores | DEP | render de produção sem `.local` nem IPs privados; passkeys funcionam no domínio público | M |
| 1.5 | Estender o portão de render: falha com segredo literal, tag `latest`, volume `ReadWriteOnce` com mais de uma réplica, host `.local` em produção | DEP | controlo negativo de cada regra | P |
| 1.6 | Decidir o caminho de produção: Helm directo num cluster, ou pelo PaaS (`deploy/delonix/`). O Ansible deste repo provisiona substrato, contra a regra do workspace | DEP | ADR curto com a decisão | P |
| 1.7 | Corrigir no motor o `COPY` para uma imagem final sem shell, e apagar o `Dockerfile.server.slim` | DEP | `make build` com o `delonix` produz a imagem distroless | M |

## Sprint 2 — Dados e operação

Objectivo: perder um nó, um pod ou uma base não perde reuniões nem gravações, e alguém é avisado.

| # | Item | Origem | Prova | Tam. |
|---|---|---|---|---|
| 2.1 | Armazenamento das gravações que funcione com várias réplicas (volume partilhado ou objectos — é decisão de arquitectura) | DEP | três réplicas em nós diferentes gravam e servem a mesma gravação | G |
| 2.2 | Backup agendado do Postgres e restauro testado | DEP | restauro para uma base limpa, com a aplicação a arrancar sobre ela | M |
| 2.3 | Uma só definição de Postgres e de Redis; o servidor fala com o Redis no modo que os valores ligam, ou os valores deixam de ligar Sentinel | DEP | failover do Redis sem perder presença | M |
| 2.4 | `DATA_ENCRYPTION_KEYS` exigida em produção e presente no chart; corrigir a documentação que manda definir uma variável que o código não lê | DEP | sem a chave o servidor recusa arrancar em produção | P |
| 2.5 | Migrações por Job em todas as edições, e um procedimento de recuo de release | DEP | recuo de uma versão num cluster de ensaio | M |
| 2.6 | `/ready` mede as dependências; sem Redis com várias réplicas, o pod não fica pronto | DEP | pod sem Postgres sai da rotação | P |
| 2.7 | Alertas e SLO como regras, não como tabela num documento | DEP | um alerta dispara num ensaio | M |
| 2.8 | TURN por TCP e TLS na 443, mais de uma réplica, e credencial renovada antes de expirar | DEP, MED | reunião a funcionar numa rede com UDP bloqueado; reconexão depois de uma hora | M |
| 2.9 | Retenção de gravações e de exportações | DEP | gravação fora do prazo é apagada | P |
| 2.10 | Limites por IP que o cliente não consegue falsificar; tecto e deduplicação na sala de espera | SEG | e2e com `X-Forwarded-For` forjado; mil ligações com um token não enchem a lista | P |

## Sprint 3 — Voz em produção

Objectivo: uma chamada real entra numa sala, com áudio nos dois sentidos, pelo caminho que o cliente vai usar.

| # | Item | Origem | Prova | Tam. |
|---|---|---|---|---|
| 3.1 | A ponte telefone↔sala liga-se com pods: o servidor aceita o FreeSWITCH de forma que sobreviva a um reinício (endereço estável, ou lista por rede), e o INVITE chega ao pod que tem a sala | DEP | telefone dentro da sala WebRTC no cluster; reiniciar o FreeSWITCH e repetir | G |
| 3.2 | Servidor ligado ao FreeSWITCH por ESL (hoje fica em loopback): a consola deixa de dizer «não ligado ao servidor de voz» | DEP, DONO | o estado do registo e uma chamada de teste a partir da consola | M |
| 3.3 | Ramais com TLS; lista de acesso obrigatória fora do laboratório; interface de gestão do Kamailio fora da porta do SIP | SEG | captura sem chaves SDES em claro; sem a lista, o arranque falha | M |
| 3.4 | FreeSWITCH sem root, com registos no stdout; imagens de voz por tag imutável | DEP | `kubectl logs` mostra a chamada; o pod corre sem root | P |
| 3.5 | Kamailio de produção: só TLS para fora, limite de taxa, NAT, allowlist editável | DEP | `INVITE` de fora da lista recusado; inundação travada | M |
| 3.6 | Ramal → sala provado com um softphone real (número de acesso e PIN) | DONO | chamada do Linphone ouvida na sala e vice-versa | P |
| 3.7 | Endereço público do servidor SIP na consola; diálogos de credenciais sem sobreposição nem scroll horizontal | DONO | captura a 375 px e a 1280 px | P |
| 3.8 | Ramal automático por utilizador quando a organização adere à voz, com PIN secreto gerado e Linphone configurado por QR de uso único; ramais da empresa (sem pessoa) criados pelo administrador, sem PIN por omissão (decisão 3) | DONO | activar a voz numa organização dá um ramal e um PIN a cada pessoa; o QR regista o Linphone sem digitar a password; cinco PIN errados bloqueiam e ficam na auditoria; um ramal da empresa regista sem PIN | M |
| 3.9 | Consola de operador para o FreeSWITCH e o Kamailio (estado, canais, dispatcher, allowlist) | DONO | editar a allowlist na consola e ver um `INVITE` passar de recusado a aceite | G |
| 3.10 | Interligação com o PBX do cliente (Issabel, FreePBX) por tronco: assistente e medição | DONO | ligar um PBX de ensaio só pela consola | M |
| 3.11 | Primeira operadora: enviar o pedido de informação, contratar um tronco de ensaio, fazer as três medições da skill `delonix-meet-voip` | PRD | chamada nos dois sentidos, DTMF, controlo negativo | G |
| 3.12 | Decidir o ADR-0009 (está «Proposto» com o código em produção) | PRD | estado do ADR actualizado | P |

## Sprint 4 — Frontend: não perder o trabalho de ninguém

Objectivo: nenhum ecrã branco, nenhuma gravação perdida, e o build que vai para produção é o que foi testado.

| # | Item | Origem | Prova | Tam. |
|---|---|---|---|---|
| 4.1 | O build deixa de embutir tudo em `data:` (hoje um ficheiro tem 74,5 MB e as fontes vão em base64) | FE | ficheiros de fonte e wasm em `dist/assets`; nenhum JavaScript acima de 1 MB; supressão de ruído activa com a política de segurança de produção | P |
| 4.2 | *Error boundary* por rota, com mensagem e botão de recarregar | FE | um ficheiro em falta mostra o erro em vez do ecrã branco | P |
| 4.3 | Gravações duráveis: escrita incremental em disco local, aviso ao fechar, cópia local quando o envio falha | FE | e2e que corta a rede a meio de uma gravação e recarrega | M |
| 4.4 | Supressão de ruído: a falha aparece ao utilizador e o interruptor reflecte o estado | FE | forçar a falha e ver o aviso | P |
| 4.5 | Mudar de língua não destrói o Estúdio nem tira o moderador da sala de espera | FE | e2e: mudar de língua a meio de uma emissão | P |
| 4.6 | Fila de envio: renovação do token, sem salas duplicadas, sem reentrada, com apagar | FE | envio depois de horas sem rede | M |
| 4.7 | Sair da reunião nunca fica preso a um pedido | FE | botão Sair com a rede parada | P |
| 4.8 | Confirmação nas acções destrutivas (sete sítios, e expulsar um participante), pelo diálogo do kit | FE | cada acção pede confirmação | P |
| 4.9 | Diálogos que prendem o foco; título da página por rota; navegação por teclado nos menus | FE | teste de teclado e leitor de ecrã | M |
| 4.10 | Mensagens de erro traduzidas (hoje 92 sítios mostram o texto cru) | FE | falha de rede em inglês, francês e chinês | M |
| 4.11 | ESLint com as regras dos hooks, e `StrictMode` em desenvolvimento | FE | o lint corre no CI | P |

## Sprint 5 — Gravação e emissão que se podem ver

Objectivo: uma gravação tem o som no sítio, e uma emissão cumpre o que as plataformas pedem.

| # | Item | Origem | Prova | Tam. |
|---|---|---|---|---|
| 5.1 | Sincronismo da gravação: pedir keyframe ao começar e alinhar pelo primeiro frame real de cada faixa | MED | teste com um par clique e flash, a medir o desvio no ficheiro final | M |
| 5.2 | O gravador detecta perda e reordenação, e recua de camada quando a escolhida pára | MED | gravação íntegra com perda induzida | M |
| 5.3 | Emissão: áudio a 48 kHz, intervalo de keyframes fixo, controlo do débito | MED | medição com `ffprobe` contra um servidor de ensaio | P |
| 5.4 | Emissão que não morre com o WebSocket: controlo de fila e retoma | MED | queda induzida do socket sem terminar os destinos | M |
| 5.5 | Decidir o ADR-0013 (emissão que sobrevive à queda da rede): ligar o que está escrito, ou retirar a migração e o código morto | MED, PRD | emissão a gravar em disco com a rede cortada, ou o código removido | G |
| 5.6 | A faixa que não se consegue gravar avisa o utilizador, em vez de só ir para o registo | MED | publicador com codec não suportado vê o aviso | P |
| 5.7 | Editor: exportação em MP4, escrita em fluxo para disco, envio por partes e retomável | MED | exportar e enviar uma aula de uma hora | G |
| 5.8 | Substituir e acrescentar faixas de áudio a uma gravação, sem recodificar o vídeo | DONO, MED | gravação com a faixa de áudio trocada, vídeo intacto | M |

## Sprint 6 — Estúdio de TV

Objectivo: o que o ecrã promete existe, e o sinal sai num formato que um canal recebe.

| # | Item | Origem | Prova | Tam. |
|---|---|---|---|---|
| 6.1 | Ligar o backend do estúdio ao ecrã (hoje o ecrã diz «a aguardar servidor» com as rotas feitas) | PRD | fontes, emparelhamento e documentos a funcionar pela consola | G |
| 6.2 | Saída SRT e HLS servido pela instalação | PRD, MED | sinal recebido por um descodificador SRT; leitor HLS | G |
| 6.3 | Sonoridade: alvo configurável (−23 LUFS para televisão), medida **depois** do último processamento, com limitador no mestre | MED, PRD | medição do ficheiro emitido igual ao número mostrado | M |
| 6.4 | Atraso de áudio por canal, para acertar o som com o vídeo | MED, DONO | capturadora com atraso conhecido, corrigida | P |
| 6.5 | Mix-minus por convidado e intercomunicação | PRD, MED | convidado ouve o programa sem se ouvir | M |
| 6.6 | Gravação ISO por fonte no servidor, ou retirar a bandeira que hoje não faz nada | MED, PRD | um ficheiro por fonte depois de o browser cair | G |
| 6.7 | Entrada de áudio separada da câmara (linha estéreo, interface externa) e junção controlada na saída | DONO, MED | entrada estéreo mantida em estéreo; desvio medido | M |
| 6.8 | Taxas de emissão (25/50) com cadência certa; espaço de cor declarado | MED | medição do ficheiro emitido | M |
| 6.9 | Gráficos por programa (oráculo, rodapé), multiview numa saída própria, tally de pré-visualização | PRD | por desenhar | G |
| 6.10 | Legendas no directo | PRD, MED | legendas visíveis no destino | M |
| 6.11 | Tirar do ecrã o que não tem código (protocolos de luz sem agente) | PRD | portão de capacidades verde depois de reparado | P |

Fora deste plano, por não ter cliente que o peça: SMPTE ST 2110 (não se aplica a um estúdio
em browser), NDI, replay, marcadores de publicidade, playout contínuo.

## Sprint 7 — PWA sem rede e higiene de React

Objectivo: tudo o que não é tempo real funciona sem rede, e a sala não se redesenha por causa da presença de quem não está nela.

| # | Item | Origem | Prova | Tam. |
|---|---|---|---|---|
| 7.1 | Service worker: não apagar caches alheias, não deitar fora os ficheiros pesados a cada deploy, fluxo de actualização com aviso | FE | deploy com uma sessão aberta, sem ecrã branco | M |
| 7.2 | Precache do que o Estúdio importa dinamicamente (a vista TV incluída) | FE | instalação fria sem rede abre a vista TV | P |
| 7.3 | Biblioteca descarregada visível sem rede; cache de leitura da API; indicador de rede em toda a consola | FE | abrir uma gravação já vista com a rede desligada | G |
| 7.4 | Rascunhos persistidos (chat, agendar, notas) e envio com a aplicação fechada | FE | escrever sem rede e ver o envio quando ela volta | M |
| 7.5 | Pedir armazenamento persistente ao browser | FE | o browser confirma | P |
| 7.6 | Partir o componente da sala e separar o contexto de presença | FE | perfil de desenho: uma mudança de presença não redesenha a sala | M |
| 7.7 | Cache e deduplicação de pedidos; cancelamento a sério | FE | uma lista pedida uma vez ao mudar de página | M |
| 7.8 | Usar o que o React 19 já dá: eventos de efeito, transições, estado optimista | FE | por componente | M |

O tecto: uma reunião em directo, a sala de espera, a presença e a emissão precisam de rede.
«100% offline» aplica-se à aplicação, ao Estúdio local, à edição e ao que já foi descarregado.

## Contínuo — dizer a verdade

- Corrigir os textos que afirmam o que o código não faz («E2EE sempre», «conformidade fora da caixa») e os que dizem que falta o que já está feito (convidado sem conta, troncos). **PRD**
- Não nomear operadoras até haver uma chamada real por cada uma; o assistente da consola é um formulário pré-preenchido. **PRD**
- Actualizar as skills e o `HARNESS.md` ao fechar cada sprint. **DEP**

## O que este plano não decide

1. **O caminho de produção** (item 1.6).
2. **O armazenamento das gravações** (item 2.1).
3. ~~O que é o «PIN» de um ramal~~ — **decidido pelo dono a 2026-10-04** (item 3.8). São três coisas separadas:
   - **Número do ramal:** a identidade na lista telefónica; automático ao aderir, de um intervalo que o administrador escolhe; não é secreto.
   - **Password SIP:** a credencial do aparelho, gerada, longa e aleatória; ninguém a digita — o softphone configura-se por QR de provisionamento de uso único, e o diálogo de credenciais fica como caminho de recurso.
   - **PIN:** código secreto de 6 dígitos, gerado, guardado só em hash e mostrado uma vez; o utilizador pode mudá-lo e o administrador pode forçar a regeneração sem o ver. Bloqueio temporário após 5 tentativas falhadas, com registo na auditoria; recusa de sequências triviais e de PIN igual ao número do ramal.

   Uso do PIN: do próprio ramal registado não se pede (o aparelho já está autenticado), só o da sala; de fora, pela operadora, ramal mais PIN identificam a pessoa, que entra com o seu nome; e é o PIN que permite a quem liga por telefone agir como anfitrião. Os ramais da empresa (recepção, sala, portaria) não têm PIN por omissão; o administrador só o define se o ramal precisar de abrir reuniões como anfitrião.
4. **Se a `main` ou o `develop` é o ramo de release** depois da unificação (item 0.2).
5. **A quem se vende o Estúdio de TV**: sem um cliente de televisão nomeado, a ordem do sprint 6 é hipótese.
