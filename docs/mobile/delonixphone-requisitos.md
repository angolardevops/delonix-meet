# DelonixPhone — requisitos funcionais e não funcionais

**Estado:** Proposta · **Data:** 2026-10-08 · **Âmbito:** app móvel (Flutter) para Android e
iPhone, softphone SIP/VoIP com SRTP, integrada no Delonix Meet. Este documento é um
desenho de requisitos: **nada aqui está implementado nem medido**, excepto onde a coluna
«Base» cita um facto lido em `origin/develop` a 2026-10-08.

## 1. Viabilidade e o que o repo já dá

**Sim, é viável com Flutter**, com uma ressalva estrutural: o Flutter desenha a interface;
o *motor* SIP/media e a integração com o sistema de chamadas (CallKit, ConnectionService,
push) são **nativos** e têm de existir atrás de uma porta (§4). Um «Linphone completo» em
Dart puro não existe.

Factos medidos em `origin/develop`:

| Facto | Onde | Consequência |
|---|---|---|
| Um **ramal** é uma extensão SIP 1:1 com um membro da org (ou da empresa, sem pessoa), com `sip_username`, PIN e estado | `server/src/ramais.rs` | O telefone da app **é** um ramal. Não há conceito novo no servidor para a identidade. |
| Os ramais registam-se **directamente no FreeSWITCH** (perfil Sofia `internal`), com digest via `mod_xml_curl`; o Kamailio só serve o tronco PSTN e **não tem `usrloc`/`registrar`** | `ramais.rs` (cabeçalho), `voice/kamailio/kamailio.cfg` | **Não existe ponto onde ligar o push** (§5.3). É a maior lacuna. |
| A API devolve `sip_server` (`host`, `port`, `transport`, `uri`) por ramal; o exemplo da documentação usa `transport=udp` | `ramais.rs` `SipServerInfo` | A app tem de **recusar** `udp`/`tcp` em claro (ADR-0009: SRTP exige TLS). Falta confirmar a configuração real. |
| SRTP é **SDES** no SDP, e «sem TLS não há central» | ADR-0009, ADR-0010, ADR-0016 | A app tem de fazer SDES sobre TLS. DTLS-SRTP/ZRTP só se o servidor os anunciar. |
| Um Linphone Android 6.2.8 já negociou **Opus 48 kHz** com o ramal, e a ponte aceita banda larga | ADR-0018 | Há uma linha de base de interoperabilidade; é a que a app tem de igualar ou melhorar. |
| Existe `/api/v1` com OpenAPI, chaves de acesso (WebAuthn) e **sessões revogáveis** («terminar o iPhone») | `lib.rs`, ADR-0011 | O cliente Dart gera-se do OpenAPI; revogar a sessão tem de desregistar o telefone e anular o token de push. |
| Contas **particular** e **empresarial**; tudo pende de uma `org_id` | ADR-0019 | Por decidir: uma conta particular tem ramal? (§9, Q3) |
| A entrada numa reunião por telefone faz-se marcando o **número de acesso** e o PIN da sala; existe a ponte telefone↔sala | `ramais.rs` Fase 3 (R273), ADR-0010 | Dá a «entrar na reunião» desde o dia 1, sem WebRTC. |
| Nenhum código nem doc de app móvel, push (FCM/APNs) ou SIP sobre WebSocket | `git ls-tree`, só `docs/templates/v5/…/DelonixMobile.html` (maqueta) | Tudo o que é móvel parte do zero. |

## 2. Princípios

1. **A chamada tem de tocar com a app morta.** Se não toca, o produto não é um telefone
   (§5.3). Todo o resto é secundário.
2. **Cifrado por omissão.** TLS na sinalização, SRTP na media, sem opção «desligar».
3. **Uma identidade, vários dispositivos.** Login pela conta Meet; o telefone é um ramal
   provisionado, nunca configurado à mão (o «assistente de contas SIP» do Linphone é
   exactamente o passo manual que aqui é um bloqueio).
4. **Isolamento multi-tenant a 100%.** Nenhum directório, histórico, presença ou chamada
   cruza organizações. Provado com teste de duas orgs.
5. **Prova medida.** Cada requisito fecha com prova num aparelho real, não com «compila».

## 3. Âmbito

**Dentro:** chamadas áudio e vídeo SIP, ramais da org, entrada em reuniões Meet, contactos
da org, histórico, voicemail, push, Android e iOS.
**Fora (v1):** emissão de TV/estúdio, gravação e transcrição no servidor (continuam no
Meet), E2EE de ponta a ponta, tablets/desktop/web, Android Auto/CarPlay (Fase 5),
mensagens de grupo ricas.

## 4. Arquitectura-alvo (resumo)

```
Flutter UI (Dart)  ── tokens/i18n do Meet ──┐
  │ porta SipEngine (Dart)                  │
  ├─ adaptador nativo: Android (Kotlin) / iOS (Swift) — motor SIP+media
  ├─ CallKit (iOS) / ConnectionService (Android) — chamadas do sistema
  ├─ Cliente /api/v1 (gerado do OpenAPI) — login, ramal, directório, histórico
  └─ Armazém local cifrado (histórico, contactos em cache)
Servidor: FreeSWITCH (SIP+SRTP) · API Meet · NOVO: pasarela de push
```

- **Localização:** `mobile/delonixphone/` no repo `delonix-meet`, para partilhar OpenAPI,
  i18n e portões (a decidir, Q1).
- **`SipEngine` é uma porta** (como os providers do resto da casa): a UI nunca importa o
  motor. Isto permite trocar de motor sem reescrever a app, e correr a mesma bateria de
  testes contra dois adaptadores.
- **Escolha do motor — por decidir, não por assumir.**

| Opção | SIP | SRTP | Vídeo/conf. | Licença | Risco |
|---|---|---|---|---|---|
| **liblinphone** (SDK do Linphone) | completo | SDES, ZRTP, DTLS | sim, nativo | **GPLv3 ou comercial** (Belledonne) | A GPL obriga a abrir a app, ou a comprar licença; e a GPL tem historial de atrito com a App Store. **Decisão de negócio.** |
| **PJSIP** | completo | SDES, DTLS | sim | GPLv2 ou comercial | Mesma questão de licença; wrapper nativo a escrever. |
| **baresip** (+ wrapper) | bom | SDES, DTLS | parcial | BSD | Menos maduro em iOS; muito trabalho de integração. |
| **flutter_webrtc + SIP sobre WSS** (JsSIP/`sip_ua`) | via WebSocket | **DTLS-SRTP**, não SDES | sim | BSD/MIT | Exige `wss` no FreeSWITCH (hoje não configurado) e muda o contrato de media face ao ADR-0010; perde-se fiabilidade em background. |

  **Decidido (ADR-0022, proposto):** liblinphone atrás da `SipEngine`, com **licença comercial da
  Belledonne** para tudo o que saia da equipa (AGPLv3 só em desenvolvimento e CI). Plano B decidido já:
  WebRTC com SIP sobre WSS. Razões, custos e critérios de saída no ADR; o preço e o contrato são do dono.

## 5. Requisitos funcionais

Prioridade MoSCoW: **M** obrigatório v1 · **S** devia (v1.x) · **C** podia · **W** fora.

### 5.1 Conta e provisionamento

| ID | Requisito | P | Aceitação |
|---|---|---|---|
| RF-01 | Iniciar sessão com a conta Meet (password + 2º factor TOTP/chave de acesso, ADR-0011) | M | Login com MFA ligado numa conta de teste |
| RF-02 | **Provisionamento automático do ramal** após login: `sip_username`, domínio/realm, proxy TLS, número de acesso às reuniões. Zero campos SIP à mão | M | Do login ao «Registado» sem digitar nada SIP |
| RF-03 | Provisionamento por **QR / deep link** (admin gera, o utilizador lê) para o primeiro arranque | S | Ler QR numa instalação limpa regista o ramal |
| RF-04 | **Credencial por dispositivo**: cada telemóvel regista-se com a sua; revogar um não derruba os outros | M | Dois telemóveis no mesmo ramal; revogar um, o outro continua |
| RF-05 | Revogar a sessão (ADR-0011) **desregista** o telefone e invalida o token de push | M | «Terminar o iPhone» na consola: o aparelho deixa de tocar em ≤ 60 s |
| RF-06 | Várias contas/organizações no mesmo aparelho, com uma activa e as outras visíveis | S | Trocar de org sem terminar sessão |
| RF-07 | Estado de registo visível (registado, a registar, erro com causa legível) | M | Cada causa de falha tem texto próprio |
| RF-08 | Conta sem ramal atribuído: a app explica e só oferece reuniões | M | Estado vazio sem erro |

### 5.2 Chamadas de voz

| ID | Requisito | P | Aceitação |
|---|---|---|---|
| RF-10 | Fazer chamada a ramal da org (número curto, 3–5 dígitos) | M | Ramal→ramal, áudio bidireccional |
| RF-11 | Fazer chamada a número PSTN, via tronco/plano de marcação da org, com formato E.164 e regras de país (+244) | M | Chamada a um DID de teste |
| RF-12 | Receber chamada com a app em primeiro plano, **segundo plano e terminada** | M | Ver RNF-01/02 |
| RF-13 | Atender, recusar, recusar com mensagem, terminar | M | — |
| RF-14 | Espera (hold/resume) e silenciar | M | Música/silêncio do outro lado; sem fuga de áudio |
| RF-15 | DTMF (RFC 4733, com INFO como alternativa) e teclado durante a chamada | M | Navegar um IVR |
| RF-16 | Transferência **cega** e **assistida** | M | Resultados medidos nos dois |
| RF-17 | Segunda chamada em espera, alternar, fundir | S | — |
| RF-18 | **Conferência a 3**: promover a chamada a uma **sala Meet** em vez de misturar no telefone | S | A sala tem os dois + o novo; ver §5.5 |
| RF-19 | Encaminhamento, «não incomodar», horário (sincronizados com o servidor, não só locais) | S | DND impede o toque, regista chamada perdida |
| RF-20 | Rotas de áudio: auricular, alta-voz, Bluetooth, fones com fio; mudança a meio da chamada | M | Trocar com chamada activa sem cortar |
| RF-21 | Continuidade: a chamada sobrevive a Wi-Fi↔dados (re-INVITE/ICE restart) | S | Cortar o Wi-Fi a meio; recupera em ≤ 5 s |
| RF-22 | Chamadas de emergência: **não** passam pela app; abre o marcador nativo | M | 112/113/… nunca vão por SIP |
| RF-23 | **Retorno de chamada por GSM** quando não há dados utilizáveis: a app pede ao servidor, que liga ao telemóvel do utilizador pelo tronco e faz a ponte para o destino. A SIM é do próprio utilizador e o SIP não corre no aparelho | S | Sem dados, o pedido chega por SMS/dados mínimos; o telemóvel toca e fala-se com o destino |
| RF-24 | **Marcador nativo como reserva** (`TelecomManager.placeCall` / `tel:`), sempre por escolha do utilizador | S | Sem dados, a app oferece «ligar pela rede móvel» |
| RF-25 | **Convivência com chamadas GSM**: uma chamada celular a entrar põe a chamada SIP em espera, e esta retoma-se ao terminar; sem áudio cruzado (Android `TelephonyCallback`, iOS interrupção da `AVAudioSession`/`CXCallObserver`) | M | Emulador: `gsm call` durante uma chamada SIP; a SIP fica em espera e retoma |

### 5.3 Chamadas recebidas com a app terminada (o núcleo)

| ID | Requisito | P | Aceitação |
|---|---|---|---|
| RF-30 | **iOS:** push **PushKit/VoIP** → a app reporta a chamada ao **CallKit** de imediato. Cada push TEM de reportar uma chamada, ou o iOS corta o canal | M | Chamada com a app morta toca no ecrã de bloqueio |
| RF-31 | **Android:** mensagem **FCM de alta prioridade** (só dados) → **ConnectionService/Telecom** + notificação de ecrã inteiro; serviço em primeiro plano do tipo `phoneCall` | M | Idem, em Android 12, 13 e 14 |
| RF-32 | O servidor **segura a chamada** enquanto o aparelho acorda e se regista (não responde 480/486 ao primeiro INVITE) | M | Timeout configurável; chamada atendida após acordar |
| RF-33 | Se o aparelho não acordar, o chamador segue para voicemail/encaminhamento; fica «chamada perdida» no histórico | M | Aparelho em modo avião |
| RF-34 | Toque que respeita silêncio/DND do sistema e canais de notificação | M | — |
| RF-35 | Tocar em **vários** aparelhos do mesmo ramal; o primeiro a atender cancela os outros | S | «Atendida noutro dispositivo» |

### 5.4 Vídeo

| ID | Requisito | P | Aceitação |
|---|---|---|---|
| RF-40 | Chamada de vídeo SIP (H.264 + VP8), câmara frontal/traseira, desligar vídeo a meio | S | Entre dois aparelhos |
| RF-41 | Adaptação à rede (resolução/fps/bitrate); degradar para áudio em vez de cortar | S | Limitar a rede e medir |
| RF-42 | Picture-in-picture | C | — |

### 5.5 Integração com o ecossistema Meet

| ID | Requisito | P | Aceitação |
|---|---|---|---|
| RF-50 | **Entrar numa reunião por código/link** com áudio, via número de acesso + PIN (ponte existente) | M | Entrar numa sala de teste a partir da app |
| RF-51 | Entrar numa reunião **com vídeo e participantes** via WebRTC contra o SFU, no mesmo aparelho (reutiliza o sinal `/rtc`) | S | Ver quem fala; partilha de ecrã só recepção |
| RF-52 | **Promover chamada a reunião** (RF-18) e **adicionar alguém** que seja ramal ou número | S | — |
| RF-53 | Receber «ligar a partir da sala» (`docs/ligar-a-partir-da-sala.md`): a sala convida um ramal e a app toca | S | — |
| RF-54 | Agenda: reuniões do dia, «entrar» com um toque, lembrete | S | Notificação 5 min antes |
| RF-55 | Directório da **org** (ramais + membros), pesquisa e estado de presença da sala (em reunião, livre) | M | Pesquisa por nome e por número |
| RF-56 | **Contactos do telemóvel** só como opção, com consentimento explícito, e **nunca enviados ao servidor** sem pedido | C | Auditoria de rede: zero upload por omissão |
| RF-57 | Ligações profundas (`delonixphone://`, universal links) para chamar e entrar | M | — |
| RF-58 | Voicemail: caixa, reprodução, transcrição quando o servidor a tiver, indicador de mensagens (MWI) | S | Deixar e ouvir uma mensagem |

### 5.6 Histórico, mensagens e definições

| ID | Requisito | P | Aceitação |
|---|---|---|---|
| RF-60 | Histórico (feitas, recebidas, perdidas) com duração, sincronizado entre aparelhos a partir do CDR do servidor | M | Chamada feita no telefone A aparece no B |
| RF-61 | Chamada perdida: notificação e botão «ligar de volta» | M | — |
| RF-62 | Mensagens curtas entre ramais (SIP MESSAGE) | C | Fase 5 |
| RF-63 | Definições: codecs (ordem), eco/ganho, notificações, idioma, tema, diagnóstico | M | — |
| RF-64 | Ecrã de **diagnóstico**: registo, transporte, codec, SRTP activo, RTT, perda, jitter, ICE; exportar traço **sem credenciais** | M | Um engenheiro de suporte resolve com ele |
| RF-65 | Indicador de **cifra**: cadeado só quando TLS **e** SRTP estão realmente activos | M | Teste negativo: media sem SRTP não mostra cadeado |
| RF-66 | Bloqueio da app por biometria/PIN | S | — |
| RF-67 | Acessibilidade e quatro línguas do Meet (pt, en, fr, es) | M | Leitor de ecrã percorre o marcador e a chamada |

## 6. Requisitos não funcionais

Os alvos são **propostas a validar na Fase 0**, não medições.

### 6.1 Fiabilidade de chamada

| ID | Requisito | Alvo |
|---|---|---|
| RNF-01 | Toque com app terminada, aparelho em rede | ≥ 98% das chamadas tocam, em ≥ 200 chamadas por plataforma |
| RNF-02 | Do INVITE ao toque, com push | p95 ≤ 4 s (iOS), ≤ 5 s (Android) |
| RNF-03 | Registo após mudança de rede | ≤ 5 s |
| RNF-04 | Chamada não cai em fundo/ecrã bloqueado | 60 min sem corte |
| RNF-05 | Android com restrições de bateria de fabricantes (Samsung, Xiaomi, Tecno/Infinix, Huawei) | Assistente que guia a isenção de bateria; teste nesses fabricantes |

### 6.2 Qualidade de media

| ID | Requisito | Alvo |
|---|---|---|
| RNF-10 | Opus por omissão, G.722/PCMA/PCMU como reserva | Sem transcodificação quando as duas pernas falam Opus |
| RNF-11 | MOS estimado (E-model) em rede boa / rede degradada (3% perda, 150 ms RTT) | ≥ 4,0 / ≥ 3,5 |
| RNF-12 | Atraso boca-a-ouvido | ≤ 250 ms em rede boa |
| RNF-13 | Cancelamento de eco, supressão de ruído, controlo de ganho | Do sistema (AEC do SO) com respaldo do motor; medido em alta-voz |
| RNF-14 | Rede móvel ruim: bitrate Opus adaptativo a partir de ~12 kb/s, FEC/DTX | Áudio inteligível a 20 kb/s |
| RNF-15 | Consumo de dados | ≤ 40 MB/hora de voz (Opus ~24 kb/s + overhead) |
| RNF-16 | Sincronismo som/imagem em vídeo | ≤ 80 ms de desvio |

### 6.3 Segurança e privacidade

| ID | Requisito | Alvo |
|---|---|---|
| RNF-20 | Sinalização só **TLS ≥ 1.2** com validação de certificado; recusar `udp`/`tcp` em claro, e **sem opção** de os activar | Teste: servidor só-UDP → app recusa |
| RNF-21 | Media só **SRTP**; se o par não negociar, a chamada falha com mensagem, não cai em RTP | Teste negativo com perna sem SDP cifrado |
| RNF-22 | **Limite honesto do SDES:** as chaves viajam no SDP, logo a confidencialidade depende do TLS salto a salto e da confiança no servidor. Não se vende como E2EE; ZRTP/DTLS só se o servidor os suportar | Texto de produto revisto contra isto |
| RNF-23 | Segredos no **Keychain/Keystore**, nunca em ficheiros, logs ou backups da nuvem | Auditoria estática + inspecção do pacote |
| RNF-24 | Anti-spoofing do CLI: a app mostra o número que o servidor autentica, nunca o que o chamador afirma | Consistente com a regra de CLI do ADR-0016 |
| RNF-25 | Resistência a fraude de tarifação: limite de chamadas PSTN e destinos bloqueados são do servidor; a app **nunca** é a defesa | Sem regra de custo apenas no cliente |
| RNF-26 | Histórico e cache **cifrados em repouso** (SQLCipher ou equivalente); apagados ao terminar sessão | Inspecção do armazém num aparelho com root |
| RNF-27 | Sem telemetria de terceiros com identificadores pessoais; relatórios de falhas sem PII, números truncados | Revisão dos SDK incluídos |
| RNF-28 | Conformidade **LGPD/BNA**: finalidade, consentimento de contactos e gravação, retenção definida | Checklist da casa |
| RNF-29 | **Isolamento entre orgs**: directório, histórico e presença só da org activa; provado com 2 orgs | Teste automático de contrato |
| RNF-30 | Fixação do certificado do servidor (pinning) com rotação sem nova versão da app | Rotação de teste sem actualizar a app |

### 6.4 Plataforma, desempenho e distribuição

| ID | Requisito | Alvo |
|---|---|---|
| RNF-40 | Versões mínimas | Android 8 (API 26) · iOS 15 |
| RNF-41 | Aparelhos de gama baixa (2 GB RAM, ARM de 32 bit **excluído**: só arm64) | Chamada estável num aparelho de referência dessa gama |
| RNF-42 | Arranque a frio até ao marcador | ≤ 2,5 s no aparelho de referência |
| RNF-43 | Bateria | ≤ 2%/hora em espera registada; ≤ 12%/hora em chamada de voz |
| RNF-44 | Tamanho do pacote | ≤ 60 MB por arquitectura |
| RNF-45 | Rede atrás de **CGNAT** (comum nas operadoras móveis): ligação TLS persistente, *keep-alive* ajustado, media ancorada no FreeSWITCH (RTP simétrico) | Chamadas medidas em pelo menos duas operadoras de Angola |
| RNF-46 | IPv4/IPv6 e dupla pilha | Sem falha em rede só-IPv6 (NAT64) |
| RNF-47 | **Lojas:** iOS exige CallKit para VoIP push; Android exige declarar serviço `phoneCall`, `USE_FULL_SCREEN_INTENT` e política de dados | Publicação em TestFlight e pista interna da Play sem rejeição |
| RNF-48 | Chaves, certificados e IDs de contas de loja geridos fora do repo | Portão de segredos da CI |
| RNF-49 | **Soberania:** nenhum serviço obrigatório fora do nosso controlo para a chamada tocar, além dos do SO (APNs/FCM, inevitáveis); a pasarela de push é nossa | Documento de dependências |

### 6.5 Operabilidade e qualidade de engenharia

| ID | Requisito | Alvo |
|---|---|---|
| RNF-50 | Telemetria de qualidade por chamada (MOS, perda, jitter, RTT) enviada ao servidor com consentimento, para SLO | Painel por org |
| RNF-51 | SLO de serviço: disponibilidade de registo e taxa de toque, com *error budget* | Definido com a `ngolacloud-sre` |
| RNF-52 | Versões forçadas: o servidor pode recusar uma versão antiga com mensagem | Teste com versão simulada |
| RNF-53 | Testes: unidade, *widget*, contrato (contra o OpenAPI e contra o laboratório), SIPp para o servidor, aparelhos reais em CI nocturna | Portão `delonixphone_gate` com `PASS/WARN/FAIL` |
| RNF-54 | Acessibilidade WCAG 2.2 AA nos ecrãs críticos | Auditoria manual por plataforma |
| RNF-55 | Documentação do utilizador e runbook de suporte | Antes do piloto |

### 6.6 O que o GSM NÃO é (limite das APIs nativas)

Nenhuma API pública do Android ou do iOS dá a uma app o áudio de uma chamada celular, nem
permite injectar áudio nela: no Android a fonte `VOICE_CALL` é reservada ao sistema, e o
`CallKit` do iOS só observa o estado. Portanto **a app não faz de ponte GSM↔SIP**, e um
telemóvel com SIM a terminar tráfego seria uma «SIM box», que a regra de casa de
`delonix-meet-voip` exclui. O GSM entra só como RF-23/24/25. O CGNAT trata-se com ligação
TLS de saída persistente, *keep-alive*, push e media ancorada no FreeSWITCH (RNF-45), nunca
com GSM.

### 6.7 Estratégia de testes sem aparelho

| Camada | Ferramenta | Prova |
|---|---|---|
| Unidade e *widget* | Flutter `flutter test` | Lógica de estado de chamada, formatação E.164, máquina de registo |
| Fluxos na app | Flutter `integration_test` + **Patrol** (diálogos, permissões e notificações nativos) | Login → ramal registado → chamada |
| Fluxos declarativos | **Maestro** (YAML) | Os mesmos percursos, escritos por quem não programa |
| Android | **Emulador com KVM**, imagem com Play (FCM) e outra de gama baixa | `adb emu gsm call/cancel/busy`, `sms send`, `gsm signal`, `network delay/speed` |
| Telecom | `adb shell dumpsys telecom` | A chamada aparece como chamada do sistema; RF-25 |
| CGNAT | *Network namespace* + `nftables` (SNAT, temporizadores de *conntrack* curtos) com um cliente SIP de linha de comandos atrás | O mapeamento expira; a app re-regista em ≤ 5 s (RNF-03) |
| Servidor SIP | **SIPp** contra o laboratório | Carga de REGISTER/INVITE, TLS, SRTP |
| iPhone | Runner **macOS** na nuvem para compilar e testar no simulador; depois uma farm de aparelhos reais | O simulador **não** prova PushKit nem áudio CallKit |
| Push | Projecto Firebase gratuito para FCM; APNs exige Apple Developer pago | Sem a conta Apple, RNF-01/02 no iPhone ficam por provar |

Um atalho de push só para *debug* serve o desenvolvimento, e fica marcado como **não
representativo** da produção.

## 7. Trabalho necessário no servidor (lacunas medidas ou prováveis)

| ID | Lacuna | Porquê | Base |
|---|---|---|---|
| S-01 | **Pasarela de push** (APNs VoIP + FCM) e tabela de dispositivos (`device_id`, token, plataforma, sessão, último registo) | Sem isto RF-12/30/31 são impossíveis | Kamailio sem `usrloc`; ramais directos no FreeSWITCH |
| S-02 | **Segurar a chamada enquanto o aparelho acorda** (hook no dialplan/ESL que dispara o push e re-tenta o *bridge* até haver REGISTER) | RF-32 | A medir contra FreeSWITCH |
| S-03 | **Credencial SIP por dispositivo** e revogação ligada às sessões do ADR-0011 | RF-04/05 | Hoje 1 ramal = 1 `sip_username` (a confirmar o resto) |
| S-04 | Endpoint de **provisionamento** em `/api/v1` (devolve a configuração SIP e o token de provisionamento por QR) | RF-02/03 | Dados já existem em `VoiceExtensionInfo` |
| S-05 | Perfil Sofia em **TLS** (e, se a Fase 0 o decidir, `wss`) como único perfil público dos ramais | RNF-20 | Exemplo da doc diz `udp`. **Feito no laboratório** (branch `delonix-meet-telefonia/ramais-tls`, TLS opcional por `DELONIX_RAMAIS_TLS_PORT`, medido com `scripts/ramais-tls-prova.py`); por fazer: helm/cluster, `TLS_ONLY` em produção, e a rotação do certificado (S-09) |
| S-06 | Histórico e voicemail por **API** a partir do CDR (`telephony_cdr.rs`) e de uma caixa de voz | RF-58/60 | A confirmar o que o CDR expõe por ramal |
| S-07 | Presença: estado do ramal/sala exposto com isolamento por org | RF-55 | A medir |
| S-08 | Endpoint de telemetria de qualidade e recusa de versões antigas | RNF-50/52 | — |
| S-09 | Rotação de certificado documentada para o *pinning* | RNF-30 | — |
| S-10 | **Só no plano B (ADR-0022):** perfil `wss` com DTLS-SRTP no FreeSWITCH, e medir o que o `rtp_secure_media=mandatory` global faz a ofertas DTLS | RF-12, RNF-21 | Não medido |

## 8. Plano por fases (cada uma fecha com prova num aparelho real)

| Fase | Entrega | Portão de saída |
|---|---|---|
| **0 — Provas** (1–2 semanas) | Decisão de licença e de motor; protótipo mínimo que regista e liga a um ramal em Android e iPhone contra o laboratório; medição do toque com app morta | Relatório com o RNF-01/02 **medido**, e ADR do motor |
| **1 — Servidor** | S-01…S-05 | Chamada ao aparelho morto a tocar em 2 telemóveis |
| **2 — Núcleo** | RF-01…16, 20, 22, 30…34, 50, 55, 57, 60, 61, 64, 65 | Piloto interno de 10 pessoas, 2 semanas, taxa de toque ≥ 98% |
| **3 — Meet** | RF-18, 51…54, 58, 35 | Promover chamada a sala; reunião com vídeo |
| **4 — Vídeo e robustez** | RF-40…42, 17, 19, 21, RNF-45 em 2 operadoras | Chamada de vídeo estável 30 min |
| **5 — Alargar** | RF-62, 66, Android Auto/CarPlay, multi-conta | Fora do piloto |

## 9. Perguntas para o dono do produto

1. **Q1 — Onde vive?** `mobile/delonixphone/` no repo do Meet (recomendado) ou repo novo?
2. **Q2 — Licença do motor.** Respondida no ADR-0022: licença comercial da Belledonne. **Falta pedir a
   cotação** (perguntas no ADR) e assiná-la antes de qualquer binário sair da equipa.
3. **Q3 — Conta particular tem ramal?** Se sim, quem o atribui, já que não há admin.
4. **Q4 — Voicemail e transcrição** entram na v1 ou ficam para depois do piloto?
5. **Q5 — Mercado de lançamento e operadoras** para medir o CGNAT e a qualidade (Unitel,
   Africell, Movicel?).
6. **Q6 — Contas de loja:** já existem Apple Developer e Google Play, e com que entidade?

## 10. O que este documento NÃO prova

- Nenhum número do §6 foi medido; são alvos a validar na Fase 0.
- Não corri o laboratório nem inspeccionei o FreeSWITCH: S-02, S-03, S-06 e S-07 são
  hipóteses lidas do código, a confirmar com o servidor ligado.
- Não confirmei se o `transport` real dos ramais é TLS nas instalações existentes.
- O quadro de licenças e o comportamento actual das lojas vêm do meu conhecimento geral
  (até Jun/2026) e **têm de ser reconfirmados** nas fontes oficiais antes de decidir.

## 11. Estado medido da Fase 0 (2026-10-08)

| Pergunta da Fase 0 | Estado | Prova |
|---|---|---|
| O motor regista com o QR do Meet e chama com SRTP/Opus? | **Sim**, no emulador | `spike-fase0-liblinphone.md` (4 em 4) |
| O servidor sabe acordar um ramal sem registo? | **Sim** (S-01 e S-02), com o fornecedor `lab` | PR #279, ADR-0023, 11 testes e prova 6 em 6 |
| A app morta acorda, regista-se e toca? | **Sim**, no emulador, com push de **laboratório** | `mobile/ambiente/prova-acordar.py` (8 em 8): `180` ao chamador, atender no ecrã dá `200 OK` |
| Toca com um push **real** (FCM, APNs) num telemóvel? | **Por provar** | falta conta Firebase (Android) e Apple Developer (iOS) |

Por fazer: FCM e APNs reais (RNF-01/02 só se medem com eles); serviço em primeiro plano `phoneCall`, Doze e
restrições de fabricantes; iPhone (CallKit/PushKit); ligar a partir da sala por ESL e o PSTN (o *wake* é do
servidor); credencial por aparelho e atender num só aparelho (S-03, RF-35); CGNAT combinado com o motor; áudio
real; a licença comercial (ADR-0022).

