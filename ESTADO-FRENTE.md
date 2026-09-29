# Estado da frente C — Telefonia, SIP e SMS (2026-09-17, retoma concluída)

Branch `delonix-meet-backend/v3-telecom`, rebaseada sobre `origin/main` `03fecb9`. Árvore
limpa (só este ficheiro por commitar). Sem push nem PR. O servidor 8430 e o contentor
`fs-telecom` estão parados e removidos. A configuração da prova, as passwords e os logs estão
em `.fs-telecom/`, que não é seguido pelo git.

## Commits (`git log origin/main..HEAD`)

`8793f76` domínio · `c0d54c0` adaptadores/rotas/ADR · `7b7ac88` testes · `c2896b7` migrações 0065–0069 ·
`32853ee` eventos + RoomBridge · `ddd07f7` 4 defeitos do FS real + e2e · `3f7d3ea` testes FS real ·
`affb2ed` ADR §10 + conf da prova · `62ac45b` isolamento, R210–R214, OpenAPI 193/193 ·
`a483c89` HARNESS · `1547935` clippy 25 · `d22e68a` domínio SIP único.

## Portões (corridos nesta árvore)

- `cargo fmt --check` ✓
- `cargo test --release --workspace`: 542 passaram e 0 falharam, com Postgres real. O
  `telephony_freeswitch` só prova com as variáveis `FS_*` definidas; foi corrido à parte (4/4).
- `check-clippy-ratchet` 25 ✓ · `check-route-auth` 152 rotas ✓ · `check-openapi` 193/193 ✓ ·
  `check-isolamento-cobertura` 91 ✓ · `check-arquitectura-catraca` ✓ · `check-crate-deps` ✓ ·
  `check-docs-drift` ✓
- `check-repo-hygiene`: ✗ **só** o buraco de migrações esperado (0051 → 0065).
- `web/e2e/isolamento.mjs` contra o servidor 8430 (base `v3_c` recriada, Redis db 12): 178/178.

## Provado contra o FreeSWITCH real (`delonix-dev/freeswitch:1.11.3`)

`web/e2e/telefonia-freeswitch.mjs` 20/20 · `server/tests/telephony_freeswitch.rs` 4/4
- gateways `dlx-<id>` carregados por `mod_xml_curl`; `sofia xmlstatus gateway` com `NOREG`/`UP`
  e ping de 0,6 ms;
- originate com failover: 503 no tronco A e atendimento no tronco B em 363 ms, `billsec` 2;
  o 1.º canal com `origination_uuid` igual ao id da chamada;
- `mod_json_cdr`: CDR da tentativa A (`failed`, 0,0000 Kz) e da B (`answered`, 8,9000 Kz);
  o reenvio do mesmo ficheiro dá `200 duplicate` e não cria linha nova;
- `mod_xml_curl` dialplan: o INVITE ao PBX com o domínio da org é encaminhado pelo plano
  (A falha, B atende, gravado);
- `limit_execute`: `limit_usage` = 1 e a API mostra 1/1; a 2.ª chamada dá `-ERR`; a emergência
  passa ao mesmo tempo;
- eventos: `Dialing(A) → AttemptFailed(A) → Dialing(B) → Ringing(B) → Answered(B, 344 ms) → Ended`;
  ocupado termina em `USER_BUSY`; `RoomBridge` chega ao UA da ponte e o `hangup` pela porta fecha
  a chamada.

Seis defeitos encontrados só contra o real e corrigidos (ADR-0009 §10):
1. `<users>` no domínio era ignorado em silêncio (0 gateways);
2. `domain name="all"` não passa pelo `xml_curl`;
3. `origination_uuid` global partia o failover (`DESTINATION_OUT_OF_ORDER`);
4. uma chamada atendida em menos de 1 s tem `billsec` 0 (atendida passa a ler-se do `answer_epoch`);
5. com o mesmo domínio SIP em duas orgs, uma chamada saiu pelos troncos da outra (agora `409`);
6. o `HANGUP_COMPLETE` da tentativa falhada chega depois do atendimento da seguinte.

## Pedidos da frente D

- a) `AfterAnswer::RoomBridge` ✓. É SIP para um UA da ponte, não RTP cru, porque a imagem não
  tem `mod_rtp`.
- b) `CallEvent` + `CallEventSink` na porta ✓. O `place_call(…, listener)` repassa os eventos.
- c) `send_sms`/`SmsGateways` ficam com a frente C e já estão extraídos.

O `notas-ui-template/contrato-telefonia.md` está actualizado.

## Continua EXTERNAL / não validado

- **Kamailio JSON-RPC:** nunca provado; não há imagem.
- **Produção:** TLS/SRTP e NAT não provados; FreeSWITCH só em loopback; operadoras reais não
  contratadas.
- **Ponte FreeSWITCH↔SFU:** não existe.
- **Agente SMS:** não reporta bateria nem saldo.
- **Prefixos:** «A CONFIRMAR».
- **Pesquisa (ADR-0007):** a lista de chamadas ainda não está ligada ao motor.
- **Autorização:** falta trocar `require_admin_pub` por `require_capability` (frente A).
  Capacidades propostas: `telephony.manage`, `telephony.view`, `telephony.reveal_credentials`,
  `telephony.test_call`, `sms.view`.
