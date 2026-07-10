# Delonix Meet — Scaling & Multi-Region Blueprint

*Este documento estabelece as regras estritas de arquitetura e infraestrutura para escalar o Delonix Meet horizontalmente e operar em múltiplas regiões geográficas (Edge computing).*

---

## 1. Visão Geral (Topology)

Para atingirmos a capacidade concorrencial do Google Meet/Zoom, a plataforma evolui de um modelo *Single-Node* para um modelo **Global Edge Network**.
Isto exige o desacoplamento do *Control Plane* (Sinalização e Metadados) do *Data Plane* (Media/SFU).

```mermaid
flowchart TD
    User_US(User US) <-->|WSS + RTP| Edge_US[Edge SFU US]
    User_EU(User EU) <-->|WSS + RTP| Edge_EU[Edge SFU EU]
    
    subgraph Control Plane
        Edge_US <-->|Pub/Sub| Redis[(Redis Global Bus)]
        Edge_EU <-->|Pub/Sub| Redis
    end
    
    subgraph Data Plane (SFU Cascading)
        Edge_US <-->|S2S WebRTC/UDP| Edge_EU
    end
    
    subgraph State
        DB_Primary[(PostgreSQL Primary)] <--> Edge_US
        DB_Replica[(PostgreSQL Replica EU)] <--> Edge_EU
        DB_Primary -.->|Async Replication| DB_Replica
    end
```

## 2. Control Plane (Sinalização Distribuída)

O estado atual no `sfu.rs` / `signaling.rs` depende de um `DashMap` em memória.

### Regras de Implementação para Agentes:
1. **Mensagens WS em Edge**: O utilizador conecta-se ao WebSocket mais próximo (baseado em GeoDNS).
2. **Redis como Message Bus**: Todas as mensagens de estado (join, leave, chat, ICE, offers/answers) **devem** ser publicadas num canal Redis com o padrão `room:{room_id}:events`.
3. **Escuta de Canais**: Cada Edge Node subscreve aos canais das salas que possuem participantes locais. Ao receber um evento do Redis emitido por outro Edge, converte de volta para o cliente local WS via `ServerMsg`.

## 3. Data Plane (SFU Cascading)

Não podemos forçar os clientes a cruzar oceanos para atingir um SFU central (evitando o *jitter* que degrada o VoIP).

### Regras de Implementação para Agentes:
1. **Tree Topology**: Em reuniões *cross-region*, os Edge Nodes assumem-se como pares de infraestrutura (`Server-to-Server WebRTC` ou túneis UDP próprios). 
2. **Seletividade de Cascading**: Um Node (ex: EU) apenas requer a *track* RTP de um publicador (ex: US) se existir pelo menos **um** subscritor ativo no Node EU que pretenda visualizar esse publicador.
3. **Simulcast Routing**: O Node US só retransmite a camada de simulcast mais alta requisitada por toda a região EU, para poupar banda inter-region. A conversão final de sub-camadas (se necessário, de `h` para `q` por exemplo) acontece no *fan-out* do Edge EU.
4. **Tratamento de Perdas (FEC/NACK)**: 
   - Habilitar e configurar buffers de NACK na ponte S2S.
   - Expandir a stack RTP com redundância FEC (Forward Error Correction) no `sfu.rs` para compensar o packet loss típico em redes 3G/4G na última milha do cliente móvel.

## 4. Persistência e Latência

### Regras de Implementação para Agentes:
1. **Writes no Primary**: Criação de Salas, organizações (`org.rs`), registos de gravação e atualizações de agenda ocorrem **sempre** contra a base de dados principal (Primary DB) localizada tipicamente numa região intermédia, para consistência estrita.
2. **Reads no Replica**: Verificações de JWT, acessos, políticas de retenção (`require_admin`, `org_stats`) nas views e requests com `GET` (onde alguma latência de replicação é tolerável) acedem às *Read Replicas* do PostgreSQL das respetivas localizações.

## 5. Resolução de Conflitos em Agenda Mundial (Quarentena Distribuída)

A deteção de duplicação num DB Replica está sujeita a *replication lag*. Não podemos confiar apenas no DB.

### Regras de Implementação para Agentes:
1. **Redlock antes do INSERT**: Qualquer *handler* de criação de reuniões físicas / agendamentos (ex: `POST /api/calendar/events`) deve, obrigatoriamente antes da transação na BD, adquirir um Lock no cluster Redis da respetiva chave `lock:org:{org_id}:resource:{resource_id}`.
2. **Fallback / Quarentena**: Se o Lock não for possível ou houver latência fatal, o evento entra em `Collision Quarantine` na DB para resolução humana assíncrona, mantendo e acionando a regra introduzida na `0006_collision_quarantine.sql`.
