# FreeSWITCH da prova da ponte telefone↔sala (ADR-0010, R222)

Ficheiros juntos à configuração segura de `.worktrees/freeswitch-build/conf/`
(imagem `delonix-dev/freeswitch:1.11.3`). **Só desenvolvimento**, tudo em 127.0.0.1.

| Ficheiro | Para quê |
|---|---|
| `sip_profiles/external.xml` (5280) | gateway FALSO `dlx-0c0a1500-0000-4000-8000-00000000d0d0` → 5260; é daqui que sai a perna `room-<sala>` para o UA da ponte |
| `sip_profiles/carrier.xml` (5260) + `dialplan/zz_carrier_canais.xml` | o «telefone»: atende, grava a chamada (`record_session`, estéreo) e toca 1 kHz |

Portas: SIP 5260/5280, ESL 8221, RTP 32800–33000 (`switch.conf.xml`).

A base segura vem de `$FS_BASE_CONF`, que por omissão é `../../freeswitch-build/conf`
— certo a partir de um worktree em `.worktrees/<repo>/<tarefa>`, errado a partir da
checkout principal, onde é preciso passar a variável.

```bash
bash scripts/fs-canais.sh up      # copia a base, aplica isto, arranca o contentor fs-canais
FS_ESL_ADDR=127.0.0.1:8221 FS_ESL_PASSWORD=$(cat .fs-canais/esl-password.txt) \
  FS_CANAIS_GW=dlx-0c0a1500-0000-4000-8000-00000000d0d0 \
  FS_CANAIS_RECORDINGS=$PWD/.fs-canais/recordings \
  cargo test --release --lib ponte_com_freeswitch_real -- --nocapture --test-threads=1
bash scripts/fs-canais.sh down    # pára e remove SÓ o fs-canais
```

Sem as quatro variáveis o teste imprime «NÃO CORREU» e passa — não deixa o CI
vermelho, e também não prova lá nada (`scripts/e2e-fora-do-ci.txt`).
