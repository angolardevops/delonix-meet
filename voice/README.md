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
