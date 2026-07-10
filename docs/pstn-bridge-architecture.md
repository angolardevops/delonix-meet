# Arquitetura da Ponte de Media PSTN ↔ SFU (Fase 3)

Para integrar utilizadores PSTN (chamadas telefónicas via SIP Trunk + FreeSWITCH) na mesma sala virtual que os clientes WebRTC, implementaremos uma abordagem de **RTP Inbound Mapping (Phantom Tracks)**.

Esta decisão (RTP Bridge direta) foi selecionada face ao uso do `mod_verto` por ser mais agnóstica a nível de servidor e remover o overhead de negociações SDP repetidas e estado de SIP num worker WebRTC. A conversão SIP para RTP puro é tratada de forma excecional pelo próprio dialplan/Lua do FreeSWITCH.

## 1. Fluxo Inbound (PSTN → SFU)

1. O **FreeSWITCH** atende a chamada, corre o IVR (`dialin_ivr.lua`) e o utilizador insere o PIN.
2. Em vez de se juntar imediatamente a uma conferência local do FreeSWITCH (que isolaria o áudio), o FreeSWITCH encaminha o media RTP do utilizador para uma porta UDP dedicada no host do SFU (Rust).
3. No `server/src/sfu.rs`, iremos instanciar um **UDP Listener dinâmico** por cada sala ativa.
4. Os pacotes RTP que chegam a este socket são envolvidos numa `TrackLocalStaticRTP` fantasma (com um `peer_id` virtual, e.g., "00000000-0000-0000-0000-00000000PSTN").
5. O `SignalingHub` anuncia a "TrackPublished" deste peer fantasma.
6. Todos os clientes Web (React) assinam automaticamente a track fantasma e ouvem os chamadores telefónicos de imediato, exatamente como se fossem um utilizador Web.

## 2. Fluxo Outbound (SFU → PSTN)

1. Para que o PSTN consiga ouvir os utilizadores WebRTC, o FreeSWITCH precisa de receber áudio. 
2. Como o FreeSWITCH `mod_conference` sabe realizar o mix de várias fontes RTP recebidas, o SFU enviará as streams de áudio dos clientes Web para o FreeSWITCH via RTP unicast (`mod_conference` inbound).
3. **Modificação em `sfu.rs`**: Aquando do método `handle_publish` (quando um utilizador Web liga o microfone), o Rust fará um *fork* (bomba RTP bifurcada):
   - Um caminho escreve o RTP para as `TrackLocalStaticRTP` dos browsers locais.
   - O segundo caminho envia os pacotes `packet.marshal()` via um `UdpSocket::send_to` diretamente para a porta alocada à conferência do FreeSWITCH correspondente.

## 3. Planeamento de Código

### A. Modificações em `server/src/sfu.rs`

Adicionar a gestão de `phantom_tracks`:

```rust
// No `SfuHub`
pub struct SfuHub {
    // ...
    // Mapeia RoomID para a porta UDP local aberta e o sender RTP
    phantom_listeners: DashMap<Uuid, u16>,
}

impl SfuHub {
    pub async fn spawn_phantom_listener(&self, room_id: Uuid) -> Result<u16> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        let port = socket.local_addr()?.port();
        self.phantom_listeners.insert(room_id, port);
        
        // Spawn do loop que lê UDP e injeta na Phantom Track
        tokio::spawn(async move {
            let track = Arc::new(TrackLocalStaticRTP::new(
                RTCRtpCodecCapability {
                    mime_type: "audio/opus".to_owned(),
                    ..Default::default()
                },
                "pstn-audio".to_owned(),
                "freeswitch".to_owned(),
            ));
            
            // ... lógica de inject loop
        });
        
        Ok(port)
    }
}
```

### B. Modificações em `voice/freeswitch/scripts/dialin_ivr.lua`

Assim que o POST de validação de PIN é bem-sucedido, o control plane retorna a porta UDP em que o SFU está à escuta (`sfu_rtp_port`). 
O Lua deve executar o comando `bridge` ou `conference` enviando unicast.

```lua
-- Depois da validação do PIN
local sfu_port = json_str(resp, "sfu_rtp_port")
local sfu_ip = "127.0.0.1" -- (ou lido da resposta)

-- Envia o áudio do chamador diretamente para o SFU WebRTC
session:execute("bridge", string.format("sofia/internal/sip:sfu@%s:%s", sfu_ip, sfu_port))
```

*Nota para implementação*: O SRTP deve ser desencriptado no FreeSWITCH para ser enviado como RTP puro para o bind interno do UDP no Rust, ou partilhar as chaves SRTP.

---
**Esta arquitetura garante isolamento de carga. O FreeSWITCH trata o transcoding GSM/PCMA para Opus, e o Rust continua a ser exclusivamente um relé (SFU) super rápido.**
