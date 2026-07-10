<!--
  Delonix Meet — variáveis globais do FreeSWITCH (incluir a partir de vars.xml).
  Os valores vêm do AMBIENTE (nunca hardcoded): ver docker-compose.voice.yml.
  Em produção, injetar via systemd EnvironmentFile / secrets do orquestrador.

  Incluir em /etc/freeswitch/vars.xml com:
    <X-PRE-PROCESS cmd="include" data="vars.xml.inc"/>
-->
<include>
  <!-- URL do control plane Rust (rede interna). -->
  <X-PRE-PROCESS cmd="set" data="delonix_control_url=$${DELONIX_CONTROL_URL}"/>
  <!-- Segredo partilhado da API interna de IVR (== VOICE_INTERNAL_SECRET). -->
  <X-PRE-PROCESS cmd="set" data="delonix_voice_secret=$${VOICE_INTERNAL_SECRET}"/>
  <!-- SRTP obrigatório por omissão em todos os perfis. -->
  <X-PRE-PROCESS cmd="set" data="rtp_secure_media=mandatory"/>
</include>
