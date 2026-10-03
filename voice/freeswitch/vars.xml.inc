<!--
  Delonix Meet — variáveis globais do FreeSWITCH.
  Os valores vêm do AMBIENTE (nunca hardcoded): ver docker-compose.voice.yml.
  Em produção, injetar via systemd EnvironmentFile / secrets do orquestrador.

  Só vale se o vars.xml do deploy o incluir (a directiva de pré-processamento
  cmd="include" com data="vars.xml.inc"), e HOJE NENHUM FICHEIRO DO REPO O FAZ:
  o compose monta este ficheiro, não o inclui (R226).

  NÃO escrevas essa directiva por extenso num comentário, nem aqui nem noutro
  ficheiro de configuração: o pré-processador do FreeSWITCH executa-a mesmo
  dentro de um comentário. Este cabeçalho tinha-a, e quem incluísse o ficheiro
  como ele mandava punha-o a incluir-se a si próprio, até o arranque falhar
  com "unclosed comment" (medido, R226).

  Ler o ambiente é com cmd="env-set" e $NOME. Com cmd="set" e a forma de dois
  cifrões lê-se outra variável GLOBAL, não o ambiente: as três ficavam vazias,
  e o perfil dos ramais ia parar ao porto 5060 (medido, R226).
-->
<include>
  <!-- URL do control plane Rust (rede interna). -->
  <X-PRE-PROCESS cmd="env-set" data="delonix_control_url=$DELONIX_CONTROL_URL"/>
  <!-- Segredo partilhado da API interna de IVR (== VOICE_INTERNAL_SECRET). -->
  <X-PRE-PROCESS cmd="env-set" data="delonix_voice_secret=$VOICE_INTERNAL_SECRET"/>
  <!-- SRTP obrigatório em todas as pernas: é ESTA variável global que recusa
       com 488 um INVITE sem a=crypto (R226). Só vale se este ficheiro for
       incluído pelo vars.xml, e hoje nenhum ficheiro do repo o inclui; por
       isso sip_profiles/internal.xml põe a mesma global por si. -->
  <X-PRE-PROCESS cmd="set" data="rtp_secure_media=mandatory"/>

  <!-- Ramais internos (Fase 1, ver server/src/ramais.rs) — porta SIP do
       perfil "internal" (sip_profiles/internal.xml), DISTINTA da do
       Kamailio (5060/5061), que continua só para o trunk PSTN. -->
  <X-PRE-PROCESS cmd="env-set" data="delonix_ramais_sip_port=$DELONIX_RAMAIS_SIP_PORT"/>
</include>
