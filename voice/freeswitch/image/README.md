# Imagem FreeSWITCH do Meet (R223)

O gateway de voz **partilhado** do Meet — dial-in, ramais e a ponte telefone↔sala do
ADR-0010. Um PBX de cliente (FreePBX, Issabel, 3CX…) liga-se a ele por tronco SIP;
não há um FreeSWITCH por inquilino.

| | |
|---|---|
| Base | `debian:bookworm`, fixada por digest |
| FreeSWITCH | `v1.11.3` = `ef32e205…` |
| sofia-sip | `ad36ac8f…` |
| spandsp | `8f1e1646…` |
| Local | `delonix-meet/freeswitch:1.11.3` (`make freeswitch-image`) |
| Publicada | `ghcr.io/angolardevops/delonix-meet-freeswitch:1.11.3-<sha8>`, só a partir da `main` (`.github/workflows/freeswitch-image.yml`) |

As três fontes são as da imagem local que provou a R222 (`/REF-*` dentro dela), para a
prova continuar comparável. Os commits vivem nos `ARG` do `Containerfile`, e a prova de
fumo confere-os contra os `/REF-*` da imagem construída.

## O que mudou face à imagem local de 2026-09-17

- **Entram `mod_lua` e `mod_curl`.** Os dialplans do Meet chamam `dialin_ivr.lua` e
  `ramais_dial.lua`, e estes fazem `curl` ao control plane — sem os dois módulos, o IVR
  não corria nessa imagem.
- **Sai o `mod_rtp`.** Não existe na v1.11.3: o build anterior saltava-o com um aviso
  perdido no log. Agora um módulo pedido e não compilado **parte o build**.
- **Entra o `luac5.2`**, com que o `scripts/check-lua-sintaxe.sh` compila os scripts com
  o mesmo Lua que o `mod_lua` liga.

## Sons do IVR

A imagem traz os sons oficiais do FreeSWITCH a 8 kHz, em português (`pt/BR/karina`, a
única voz portuguesa que o projecto publica) e em inglês (`en/us/callie`), fixados por
versão e conferidos pelo SHA-256 publicado. Só as pastas `ivr`, `conference`, `voicemail` e
`digits`. Sem eles o `dialin_ivr.lua` não chega a pedir o PIN: desliga em silêncio.
A voz escolhe-se no arranque (`DELONIX_IVR_VOICE`, por omissão `pt/BR/karina`).

## Configuração

A imagem traz a configuração **vanilla** do FreeSWITCH, que escuta SIP com passwords por
omissão: **nunca a arranques com rede assim**. A prova de fumo corre-a com
`--network none`. Para uso real monta-se uma configuração por cima de
`/usr/local/freeswitch/etc/freeswitch` — hoje, a base segura de
`.worktrees/freeswitch-build/conf/` (fora do repo) mais `voice/freeswitch/canais-prova/`
(`scripts/fs-canais.sh`).

**A vanilla não carrega o `mod_curl`** (`autoload_configs/modules.conf.xml`): quem
montar uma configuração para o IVR tem de o carregar.

## Provas

```bash
make freeswitch-image                         # build + prova de fumo
bash scripts/freeswitch-image-smoke.sh <img>  # só a prova de fumo, contra outra imagem
bash scripts/check-lua-sintaxe.sh             # sintaxe dos *.lua (também no make fitness e no CI)
```

A prova de fumo **não** prova a media. Para isso corre-se a R222 contra a imagem:
`FS_IMAGE=<img> bash scripts/fs-canais.sh up` e o teste descrito em
`voice/freeswitch/canais-prova/README.md`.
