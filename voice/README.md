# Delonix Meet — Camada de Media do Dial-in PSTN (sub-fase 2)

Infra-as-code da **camada de media** do dial-in PSTN. O **control plane** (salas de
voz, PIN, DID, CDR, billing) vive no backend Rust (`server/src/voice.rs`) — **não**
há serviço novo. Aqui está só o media: Kamailio (borda SIP) + FreeSWITCH (IVR +
conferência), que falam com o control plane pela API interna de IVR.

## Fluxo

```
Telefone → SIP Trunk → Kamailio (ACL trunk + TLS + dispatcher)
                            │  load-balance
                            ▼
                       FreeSWITCH (N nós)
                        1) atende (SRTP obrigatório)
                        2) IVR pede PIN (DTMF)
                        3) POST /api/voice/ivr/validate  ─────► Control plane (Rust)
                           (X-Voice-Secret)               ◄───── { room_code, voice_room_id }
                        4) conference(room_code@delonix)
                        5) no fim: POST /api/voice/ivr/cdr ────► CDR + custo estimado
```

## Ficheiros
| Caminho | Papel |
|---|---|
| `kamailio/kamailio.cfg` | Proxy SIP: ACL do trunk (anti-fraude), TLS, **dispatcher** para os FreeSWITCH |
| `kamailio/dispatcher.list` | Pool de nós FreeSWITCH (acrescentar linhas para escalar) |
| `freeswitch/scripts/dialin_ivr.lua` | IVR: PIN → valida no control plane → junta à conferência → CDR |
| `freeswitch/dialplan/public/00_delonix_dialin.xml` | Encaminha inbound para o IVR |
| `freeswitch/autoload_configs/conference.conf.xml` | Perfil de conferência `delonix` (**SRTP obrigatório**) |
| `freeswitch/vars.xml.inc` | Vars globais (URL do control plane + segredo) — **do ambiente** |
| `docker-compose.voice.yml` | Serviços de dev (Kamailio + FreeSWITCH) |

## Segurança (não-negociável)
- **SRTP obrigatório**, sem fallback: `rtp_secure_media=mandatory` no IVR, no dialplan e
  no perfil de conferência.
- **SIP-TLS** (5061) no Kamailio; certificado montado por volume (`/etc/ssl/delonix`),
  nunca comitado. Em dev usar self-signed; nunca desativar a camada.
- **Anti-toll-fraud**: só se aceita inbound dos **IPs do trunk** (`ao_trunk.txt`,
  fornecido pelo provedor 5.1). Sem outbound não autenticado.
- **Segredos do ambiente**: `VOICE_INTERNAL_SECRET` (== do backend) e URLs vêm de env,
  nunca hardcoded no repo.

## Testar sem trunk (com softphone SIP)
A camada de media valida-se **sem** o SIP trunk, usando um softphone (Linphone/Zoiper):
1. Backend Rust a correr com `VOICE_INTERNAL_SECRET` definido; criar um DID + sala de
   voz (obter o número e o PIN) — ver `docs/pstn-dial-in-fase0.md` e o E2E do control plane.
2. `docker compose -f voice/docker-compose.voice.yml up -d`.
3. Registar o softphone no Kamailio e "ligar" para o número da sala.
4. Introduzir o PIN → deve entrar na conferência. Confirmar o CDR em
   `GET /api/orgs/{org}/voice/cdr`.

## Ramais internos — chamada ramal-a-ramal (Fase 1, `server/src/ramais.rs`)

Infra-as-code de um segundo fluxo, PARALELO ao dial-in PSTN acima e que não o
toca: um "ramal" é uma conta SIP permanente (1:1 com um `org_member`, número
curto atribuído, migração `0055_ramais.sql`) para chamadas **só entre ramais
da MESMA organização**. Sem PSTN, sem ponte para salas de vídeo — ambas são
fases seguintes do mesmo plano.

```
Softphone A (ramal 101, acme.ramais.delonix.meet)
     │ REGISTER + INVITE 102           DIRECTAMENTE ao FreeSWITCH — o
     ▼                                  Kamailio NÃO entra neste caminho
FreeSWITCH — perfil "internal" (porta DELONIX_RAMAIS_SIP_PORT, default 5070)
     1) REGISTER → mod_xml_curl → POST /api/voice/ivr/directory  ──► Control plane
                                  (a1-hash do digest SIP)          ◄── XML directory
     2) INVITE 102 → dialplan "delonix_ramais" → ramais_dial.lua
        → POST /api/voice/ivr/resolve-extension  ──────────────────► Control plane
          (domínio do chamador + "102")                            ◄── sip_username
     3) bridge(user/<sip_username_de_102>@acme.ramais.delonix.meet)
```

Porquê o Kamailio fica de fora: ver o comentário no topo de
`kamailio/kamailio.cfg` — hoje é um SBC puro para o trunk (sem usrloc/
registrar/auth_db/DB), e dar-lhe isso era maior risco do que esta fase pede.

| Caminho | Papel |
|---|---|
| `freeswitch/sip_profiles/internal.xml` | Perfil Sofia dos ramais — porta própria, realm por org (`challenge-realm=auto_from`) |
| `freeswitch/autoload_configs/xml_curl.conf.xml` | Directório dinâmico (REGISTER) — consulta o control plane em vez de um XML estático |
| `freeswitch/dialplan/default/00_delonix_extensions.xml` | Contexto `delonix_ramais`: números de 3–5 dígitos → `ramais_dial.lua` |
| `freeswitch/scripts/ramais_dial.lua` | Traduz (domínio do chamador, número curto) → AOR registado, e faz o bridge |

**HA1, não Argon2, para o digest SIP.** `voice_extensions.sip_password_hash`
(Argon2) é só a segurança em repouso da nossa própria base — o protocolo SIP
Digest (RFC 2617) exige `HA1 = MD5(sip_username:domínio:password)`, guardado
à parte (`sip_ha1`) porque nenhum hash genérico serve para validar um desafio
digest. MD5 aqui não é escolha nossa — é o que o protocolo pede.

**O que NÃO foi possível verificar aqui** (sem uma instância FreeSWITCH real):
os nomes exactos dos campos que o `mod_xml_curl` desta versão envia no POST
de directório, e o comportamento de `challenge-realm=auto_from` com um realm
que varia por organização. Ambos ficam documentados nos ficheiros de
configuração respectivos (`xml_curl.conf.xml`, `sip_profiles/internal.xml`) —
antes de produção, activar `debug="true"` e confirmar contra um REGISTER
real. Tudo o resto (modelo de dados, API REST de gestão, geração/regeneração
de credenciais, UI de administração) foi corrido e verificado contra um
Postgres real neste repositório.

## ⚠️ Integração que falta: ponte FreeSWITCH ↔ SFU (sub-fase 2b)
Nesta sub-fase, os chamadores PSTN entram numa **conferência do FreeSWITCH**
(`mod_conference`) — falam entre si. Para que o áudio PSTN e o áudio **WebRTC** (SFU
`webrtc-rs` existente) se **misturem** na mesma reunião, falta a ponte de media entre
os dois, que é a integração profunda (e o que exige teste de media real, fora deste
ambiente). Duas abordagens:
- **(A)** o FreeSWITCH junta-se à sala do SFU como cliente WebRTC (via `mod_verto`/WebRTC),
  ou
- **(B)** uma ponte RTP entre a conferência do FreeSWITCH e a sala do SFU.
Recomendação: decidir esta ponte quando se voltar ao SFU; o control plane e o IVR já
estão prontos para qualquer das duas.

## Produção (microVM + Cilium)
- Kamailio e cada FreeSWITCH em **microVM dedicada** (isolamento de jitter — não
  multiplexar no runtime das apps web).
- CiliumNetworkPolicy: SIP (5060/5061) e RTP (faixa dinâmica) só entre trunk↔Kamailio↔FreeSWITCH.
- Escalar 300+ canais: acrescentar nós ao `dispatcher.list`; o Kamailio balanceia.
