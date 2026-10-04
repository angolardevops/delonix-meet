# Prova do tronco de uma central FreePBX até ao bordo do Meet

`scripts/pbx-tronco-prova.sh` ergue uma **réplica** do bordo de voz (este `compose.yaml`:
Kamailio, FreeSWITCH, servidor, Postgres e Redis, com projecto, rede e nomes próprios),
arranca a appliance FreePBX 17 real em QEMU com a configuração que o `kind: PbxService` do
`delonix-paas` gera (`spec.meet_trunk`, ADR 0063 I4), e mede a chamada nos dois lados.

Não toca no laboratório do `compose.yaml` da raiz, e pode correr ao lado dele.

```bash
bash scripts/pbx-tronco-prova.sh up          # réplica + sala de prova; imprime a CA do bordo

# No delonix-paas, a configuração da central, tal como o Kind a gera:
PBX_MEET_HOST=172.30.50.14 PBX_MEET_CA_FILE=<ca.pem> PBX_SEED_OUT=<ficheiro> \
  cargo test -p delonix-orchestrator --lib dump_the_generated_user_data_when_asked

bash scripts/pbx-tronco-prova.sh freepbx --seed <ficheiro>   # a central real, duas chamadas
bash scripts/pbx-tronco-prova.sh negativos   # sem SRTP → 488; origem fora da allowlist → 403
bash scripts/pbx-tronco-prova.sh longa       # 40 s: a chamada passa dos 32 s (R277)
bash scripts/pbx-tronco-prova.sh down
```

Precisa de docker, de QEMU com KVM, das ferramentas libguestfs, e da imagem da appliance
(`FREEPBX_IMAGE`, por omissão a do store do `delonix`). `PBX_PROVA_TIMEOUT` alarga o
limite da VM (25 min por omissão; com o host carregado, 15 não chegaram).

## O que está medido (2026-10-04)

A réplica: Kamailio 5.8.6, o FreeSWITCH 1.11.3 da imagem do repo com o arranque do
`voice/cluster/`, e o servidor da `develop`. A central: FreePBX 17.0.33 com Asterisk 22.11.0.

| Prova | Resultado |
|---|---|
| O guião do Kind na central | `exit=0`, com o tronco carregado |
| Tronco central → bordo | `Avail` na 5061, transporte `meet-tls`, certificado do bordo verificado |
| Ligação TLS vista do bordo | aberta enquanto a central correu (`kamcmd tls.info`) |
| Transacções no bordo para duas chamadas | 4 (um `INVITE` e um `BYE` por chamada) |
| IVR do Meet | correu nas duas chamadas |
| SRTP | activo nas duas, visto do FreeSWITCH e da central |
| PIN certo (`123456`) por DTMF | aceite — entrou na conferência da sala |
| PIN errado (`999999`) | recusado — o IVR voltou a falar |
| Voz do IVR ouvida pela central | 7 de 10 janelas de 0,5 s com voz nos primeiros 5 s |
| Fim das chamadas | as duas `NORMAL_CLEARING` |
| Oferta sem SRTP, da origem permitida | `488` |
| Origem fora da allowlist | `403` do bordo |
| Chamada de 40 s | `Record-Route` com o endereço real, `ACK` enviado, sem corte |

## O que esta prova encontrou

1. **O bordo anunciava-se como `0.0.0.0`** e todas as chamadas por ele caíam aos 32 s —
   R277, corrigido em `voice/kamailio/kamailio.cfg`.
2. **Uma prova curta não prova que a chamada se aguenta.** As chamadas de prova dos
   laboratórios duram 4 s; o defeito só aparece depois dos 32.
3. **Os contadores valem mais do que as linhas de log.** O `xlog` do bordo não sai no
   nível com que o Kamailio corre, e o `Secure Type` do FreeSWITCH é DEBUG, que a R227
   desligou. Mede-se por `kamcmd tm.stats`/`tls.info` e por `Activating audio Secure RTP`.
4. **Atrás de NAT, o FreeSWITCH só sabe para onde mandar a voz depois do primeiro pacote
   de quem liga.** Uma central calada nos primeiros segundos não ouve o IVR — numa chamada
   real o telefone fala desde o início.

## O que NÃO está provado

- **A central dentro da sala WebRTC.** A ponte para o SFU (ADR-0010) não está ligada na
  réplica: com o PIN certo a chamada entra na conferência local do FreeSWITCH.
- **A organização de origem.** O bordo só filtra por IP; não sabe de que inquilino é a
  central que entra. Vários inquilinos pedem um desenho próprio.
- **Chamadas do Meet para a central.** O contexto `from-meet` desliga tudo, de propósito.
- **Um bordo atrás de NAT** (`DELONIX_SIP_ADVERTISE`): só validado com `kamailio -c`.
- **O laboratório do `compose.yaml` e o cluster local com a correcção da R277**: correm o
  mesmo `kamailio.cfg`, mas não foram reiniciados e medidos nesta entrega.
- Um Issabel, e uma operadora.
