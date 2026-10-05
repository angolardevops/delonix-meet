# FreeSWITCH da prova de telefonia (ADR-0009)

> **Desde a R291 a configuração que CORRE já tem isto ligado** — o binding dos
> troncos, o `mod_json_cdr` e os gateways no perfil `external` entram pelo
> `voice/cluster/freeswitch-entrypoint.sh`, no compose, no cluster e no chart,
> e provam-se com `bash scripts/troncos-prova.sh`. Esta pasta fica para o
> `web/e2e/telefonia-freeswitch.mjs` e o `tests/telephony_freeswitch.rs`, que
> precisam do que a distribuída ainda não tem: o ESL aberto ao servidor e um
> perfil por onde um PBX entra no plano de marcação.

Os ficheiros que a frente C juntou à configuração segura de
`.worktrees/freeswitch-build/conf/` (imagem `delonix-dev/freeswitch:1.11.3`) para provar a
telefonia contra um FreeSWITCH real. **Só desenvolvimento**: tudo em 127.0.0.1, operadoras
falsas, sem TLS.

| Ficheiro | Para quê |
|---|---|
| `autoload_configs/xml_curl.conf.xml` | plano de marcação (`dialplan`) e gateways (`directory`, `purpose=gateways`) servidos por `/internal/v1/telephony/freeswitch-config` |
| `autoload_configs/json_cdr.conf.xml` | CDRs para `/internal/v1/telephony/call-records`; `log-b-leg=true` (o CDR que conta é o da perna do gateway) |
| `sip_profiles/pbx.xml` (5160) | por onde ENTRA uma chamada de um PBX; contexto `delonix-outbound` (xml_curl) |
| `sip_profiles/external.xml` (5180) | onde vivem os gateways `dlx-<trunk_id>`; `<domain name="delonix-trunks" parse="true"/>` |
| `sip_profiles/carrier.xml` (5190), `carrier-down.xml` (5191) + `dialplan/zz_carriers.xml` | operadoras FALSAS: uma atende (`…000` ocupado, `…777` chamada longa), a outra responde 503 |

Como correr (na raiz de uma worktree, com o servidor em 127.0.0.1:8430):

```bash
cp -r .worktrees/freeswitch-build/conf .fs-telecom/conf        # a base segura
cp -r voice/freeswitch/telefonia-prova/{sip_profiles,dialplan,autoload_configs} .fs-telecom/conf/
rm .fs-telecom/conf/sip_profiles/{internal,external}.xml 2>/dev/null; # os de cima substituem-nos
# ESL em 8121 com password nova; activar mod_xml_curl e mod_json_cdr em modules.conf.xml;
# SUBSTITUIR_VOICE_INTERNAL_SECRET pelo VOICE_INTERNAL_SECRET do servidor.
docker run -d --name fs-telecom --network host \
  -v $PWD/.fs-telecom/conf:/usr/local/freeswitch/etc/freeswitch delonix-dev/freeswitch:1.11.3 \
  freeswitch -nonat -nf -nc
node web/e2e/telefonia-freeswitch.mjs                            # ESL, ESL_PASSWORD, VOICE_SECRET
FS_ESL_ADDR=127.0.0.1:8121 FS_ESL_PASSWORD=… FS_GW_DOWN=dlx-… FS_GW_UP=dlx-… \
  cargo test --release --test telephony_freeswitch -- --test-threads=1
```
