# ADR-0029 — A IA em produção passa pelo chart, e as legendas ligam-se sozinhas

**Estado:** Aceite · **Data:** 2026-10-13 · **Decisor:** o dono do produto

## Contexto

A 2026-10-13 o dono pediu para confirmar que o Ollama estava instalado em
produção e que o Meet o usava para escrever a acta (MoM), e para activar a
legenda automática. A medição em produção (`ngolacloud-meet` no `delonix-lda`,
só leitura) disse o contrário:

* `kubectl get all | grep ollama` — **nenhum recurso**. O namespace tinha
  coturn, três `delonix-server`, três `delonix-web`, `meet-pg-1/2/3` e o Redis;
* o ConfigMap vivo `delonix-config` tinha 10 chaves e **nenhuma `OLLAMA_URL`**.
  Logo `Config.ollama_url = None` e a IA estava desligada — em silêncio, que é
  como o `ai.rs` degrada por desenho;
* nos registos das 48 horas anteriores, zero linhas de MoM ou de Ollama;
* nenhum dos 8 nós anuncia `nvidia.com/gpu`.

A causa não é código em falta: o resumo da acta está fundido desde 2026-10-09
(#285, #287, #289) e funciona. A causa é uma **divergência de caminhos de
instalação**. O ADR-0024 abre com «O Ollama já está deployado como parte do
caminho de produção do Meet (`deploy/k8s/07-ollama.yaml`, incluído na
`kustomization.yaml` base)» — e isso era verdade quando foi escrito, para o
caminho kustomize. A produção REAL é servida pelo chart Helm desde o ADR-0021,
e o chart nunca teve Ollama nenhum: o seu próprio `NOTES.txt` dizia, em letra
pequena, «whisper e ollama (transcrição e IA): não são instalados».

O mesmo vale para as legendas. Funcionavam — mas pelo motor do browser, e no
Chrome esse motor é a Web Speech, que **envia o áudio para servidores da
Google**. O ASR soberano existe no repo (`whisper-server/`, `09-whisper.yaml`)
e nunca foi instalado; a rota `/asr` do chart existia atrás de um valor que
ninguém preenchia. E as legendas começavam DESLIGADAS, o que fecha o círculo:
sem ninguém a ligá-las, o servidor não recebia transcrição e a acta não tinha
o que resumir.

## Decisão

1. **O chart instala a IA, e é ele a fonte.** `templates/ai.yaml` ganha o
   Ollama (com PVC dos modelos) e o Whisper, os dois desligados por omissão.
   O `values-production.yaml` liga o Ollama; o Whisper liga-se por `--set`,
   porque a sua imagem embute o modelo, tem tag própria e este ficheiro não
   pode saber qual é — e inventar uma era pior do que não a ter. A quota conta
   os dois de qualquer forma. O caminho kustomize continua a existir para
   laboratório e diz, no cabeçalho, que não é o de produção.
2. **O que estava solto passa a ser derivado.** O `OLLAMA_URL` sai do Service
   do chart e não de um valor escrito à mão — foi exactamente aí que a
   produção se perdeu. A rota `/asr` sai de `whisper.enabled`. Os modelos que
   o servidor pede têm de estar na lista que o pod puxa, e o chart recusa a
   divergência. O `check-helm.sh` tem as quatro recusas com controlos
   negativos medidos.
3. **As legendas ligam-se ao entrar na sala**, e o ASR do servidor passa à
   frente do motor do browser. Guarda-se só a recusa explícita de quem
   desliga o botão; uma falha de microfone não se guarda.
4. **Sem GPU**, com o custo escrito e medido e não escondido:
   * a tradução de uma legenda com `qwen2.5:1.5b` levou **1,9 a 3,1 s** por
     frase (contentor limitado a 4 CPU / 8Gi, como o chart; anfitrião com
     `load average` 22 em 32 CPU). O primeiro pedido levou 9,3 s porque
     inclui carregar o modelo. Isto NÃO é tempo real: a legenda original
     aparece já e a traduzida substitui-a segundos depois;
   * a acta é trabalho em lote no fim da reunião e aguenta o CPU;
   * o puxão dos modelos (~5,5GB) **não** é condição de prontidão. Até
     chegarem, o servidor responde `ModelMissing`, a fila da acta devolve a
     tentativa e nada se perde: a acta nasce quando o modelo chegar. É também
     por isso que as reuniões já em fila são resumidas retroactivamente.
5. **O Ollama corre como não-root** no chart (`OLLAMA_MODELS` no volume e
   `fsGroup`), ao contrário do manifesto kustomize. Medido com o motor a
   2026-10-13: uid 1000, o `ollama serve` atende, o `/api/tags` responde e o
   `ollama pull` escreveu 941MB no volume.
6. **A consola deixa de afirmar e passa a medir.** O painel de IA mostrava
   «activa» em código fixo para a tradução de legendas e para o resumo da
   reunião. Passa a ler `GET /api/orgs/{id}/ai/status` — barato, lista
   modelos, não gera texto — e a mostrar a razão quando não está.

## Consequências

* **A quota do namespace sobe**, e isto pede-se a um cluster partilhado: pico
  medido do render 50,20 CPU e 64,44Gi de limits, 10,01 CPU e 22,45Gi de
  requests, 25 pods, 8 PVCs. A quota passa a 62 CPU / 80Gi de limits e
  13 CPU / 28Gi de requests. Os 28Gi de requests são pouco mais de um terço da
  memória dos workers: é o que fica reservado ao Meet e indisponível para os
  vizinhos.
* **Os PVCs vão a 12 e não a 9.** O namespace tinha 10 PVCs vivos, e três
  (`redis-data-redis-node-0/1/2`) são órfãos da topologia de Sentinel que saiu
  a 2026-10-11. Com o tecto em 10, o PVC do Ollama era recusado na admissão.
  Apagar os órfãos é ordem do dono, não efeito secundário de um upgrade.
* **A imagem do ASR não acompanha a tag do servidor.** Embute o modelo (~2GB) e
  constrói-se com `BUILD_WHISPER=1 deploy/build-images.sh`: tag própria,
  passada no `helm upgrade`.
* **O egress do cluster passa a importar uma vez.** O Ollama puxa os modelos do
  `registry.ollama.ai` no primeiro arranque. Não foi possível medir se um pod
  de produção o alcança (`kubectl exec` em produção é recusado): ou o puxão o
  prova, ou prova-se antes.
* **Mais áudio e mais texto dentro do cluster, nenhum fora.** O que cada um diz
  passa a ser transcrito e enviado aos outros participantes sem ninguém
  carregar em nada — é o que a legenda automática significa, e é o que faz a
  acta existir.
* O ADR-0024 continua a valer no que decidiu (imagem fixada por versão,
  caminho declarativo para a GPU). O que este corrige é a sua premissa de
  abertura: o caminho de produção é o chart.

## Alternativas consideradas

* **Aplicar o `deploy/k8s/07-ollama.yaml` ao namespace de produção.** Era mais
  rápido e deixava dois donos para o mesmo namespace: o `helm upgrade`
  seguinte não saberia do Ollama, e a quota do chart continuaria a não o
  contar. Recusada.
* **Só ligar a legenda por omissão, sem instalar o ASR soberano.** Uma linha de
  código. Mas tornava omissão o motor que envia o áudio dos clientes para a
  Google — o contrário do que este produto vende. Recusada pelo dono.
* **Esperar por GPU para ligar a tradução de legendas.** Deixava a legenda
  monolingue até haver hardware. Ligada com a latência medida escrita, porque
  degrada bem: a legenda original nunca espera pelo modelo.
