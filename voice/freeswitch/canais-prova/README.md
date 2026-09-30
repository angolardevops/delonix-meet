# FreeSWITCH da prova da ponte telefone↔sala (frente D, ADR-0010)

Ficheiros juntos à configuração segura de `.worktrees/freeswitch-build/conf/`
(imagem `delonix-dev/freeswitch:1.11.3`). **Só desenvolvimento**, tudo em 127.0.0.1.

| Ficheiro | Para quê |
|---|---|
| `sip_profiles/external.xml` (5280) | gateway FALSO `dlx-0c0a1500-0000-4000-8000-00000000d0d0` → 5260; é daqui que sai a perna `room-<sala>` para o UA da ponte |
| `sip_profiles/carrier.xml` (5260) + `dialplan/zz_carrier_canais.xml` | o «telefone»: atende, grava a chamada (`record_session`, estéreo) e toca 1 kHz |

Portas: SIP 5260/5280, ESL 8221, RTP 32800–33000 (`switch.conf.xml`).

```bash
bash scripts/fs-canais.sh up      # copia a base, aplica isto, arranca o contentor fs-canais
FS_ESL_ADDR=127.0.0.1:8221 FS_ESL_PASSWORD=$(cat .fs-canais/esl-password.txt) \
  FS_CANAIS_RECORDINGS=$PWD/.fs-canais/recordings \
  cargo test --release --test phone_bridge_freeswitch -- --nocapture
bash scripts/fs-canais.sh down    # pára e remove SÓ o fs-canais
```
