{{/* Etiquetas comuns. */}}
{{- define "dm.labels" -}}
app.kubernetes.io/part-of: delonix-meet
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
{{- end }}

{{/* Nome do Secret da aplicação: o do operador, ou o que o chart gera em laboratório. */}}
{{- define "dm.secretName" -}}
{{- .Values.secrets.existingSecret | default "delonix-secrets" -}}
{{- end }}

{{/* Tag do servidor e da web: a própria, ou a partilhada. */}}
{{- define "dm.appTag" -}}
{{- .img.tag | default .root.Values.image.tag -}}
{{- end }}

{{- define "dm.serverImage" -}}
{{- printf "%s:%s" .Values.server.image.repository (include "dm.appTag" (dict "img" .Values.server.image "root" .)) -}}
{{- end }}

{{- define "dm.webImage" -}}
{{- printf "%s:%s" .Values.web.image.repository (include "dm.appTag" (dict "img" .Values.web.image "root" .)) -}}
{{- end }}

{{- define "dm.imagePullSecrets" -}}
{{- with .Values.imagePullSecrets }}
imagePullSecrets:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- end }}

{{/* O servidor corre com mais de uma réplica? */}}
{{- define "dm.serverMulti" -}}
{{- if or .Values.server.autoscaling.enabled (gt (int .Values.server.replicas) 1) -}}true{{- end -}}
{{- end }}

{{- define "dm.redisUrl" -}}
{{- if .Values.redis.enabled -}}
{{- printf "redis://delonix-redis.%s.svc.cluster.local:6379" .Release.Namespace -}}
{{- else -}}
{{- .Values.externalRedis.url -}}
{{- end -}}
{{- end }}

{{/* host:porta do TURN, como o browser o vê. */}}
{{- define "dm.turnHost" -}}
{{- if .Values.server.turnHost -}}
{{- .Values.server.turnHost -}}
{{- else if .Values.coturn.externalIP -}}
{{- printf "%s:%v" .Values.coturn.externalIP .Values.coturn.port -}}
{{- end -}}
{{- end }}

{{/* Contexto de segurança de um contentor sem privilégios. */}}
{{- define "dm.restricted" -}}
allowPrivilegeEscalation: false
readOnlyRootFilesystem: true
capabilities:
  drop: ["ALL"]
{{- end }}

{{/*
  Variáveis do servidor que vêm do Secret. Cada chave é referida pelo nome —
  nunca `envFrom` sobre o Secret inteiro — para que a lista do README seja a
  lista do que o pod lê.
*/}}
{{- define "dm.serverSecretEnv" -}}
{{- $s := include "dm.secretName" . -}}
- name: DATABASE_URL
  valueFrom: { secretKeyRef: { name: {{ $s }}, key: DATABASE_URL } }
- name: JWT_SECRET
  valueFrom: { secretKeyRef: { name: {{ $s }}, key: JWT_SECRET } }
- name: TURN_SECRET
  valueFrom: { secretKeyRef: { name: {{ $s }}, key: TURN_SECRET } }
- name: DATA_ENCRYPTION_KEYS
  valueFrom: { secretKeyRef: { name: {{ $s }}, key: DATA_ENCRYPTION_KEYS } }
{{- end }}

{{/*
  As recusas do chart. Juntam-se TODAS e falha-se uma vez, para o operador não
  descobrir os erros um por instalação.
*/}}
{{- define "dm.validate" -}}
{{- $e := list -}}
{{- $v := .Values -}}
{{- $prod := $v.production -}}
{{- if not $v.host -}}
{{- $e = append $e "host: falta o nome público (p.ex. --set host=meet.ngolacloud.com)" -}}
{{- end -}}

{{- /* segredos */ -}}
{{- if and $prod (not $v.secrets.existingSecret) -}}
{{- $e = append $e "secrets.existingSecret: em produção o chart não gera nem traz segredos — cria o Secret fora do chart (chaves DATABASE_URL, JWT_SECRET, TURN_SECRET, DATA_ENCRYPTION_KEYS e, com voz, VOICE_INTERNAL_SECRET; ver README §Segredos) e indica o nome" -}}
{{- end -}}
{{- if and $prod $v.secrets.create -}}
{{- $e = append $e "secrets.create=true é só para laboratório (production=false)" -}}
{{- end -}}
{{- if and (not $prod) (not $v.secrets.existingSecret) (not $v.secrets.create) -}}
{{- $e = append $e "secrets: indica secrets.existingSecret, ou secrets.create=true para gerar aleatórios (laboratório)" -}}
{{- end -}}
{{- if and $v.secrets.create $v.secrets.existingSecret -}}
{{- $e = append $e "secrets.create e secrets.existingSecret são exclusivos" -}}
{{- end -}}

{{- /* imagens */ -}}
{{- $imgs := dict "server.image" (include "dm.appTag" (dict "img" $v.server.image "root" .)) "web.image" (include "dm.appTag" (dict "img" $v.web.image "root" .)) -}}
{{- if $v.coturn.enabled -}}{{- $_ := set $imgs "coturn.image" $v.coturn.image.tag -}}{{- end -}}
{{- if $v.voice.enabled -}}
{{- $_ := set $imgs "voice.kamailio.image" $v.voice.kamailio.image.tag -}}
{{- $_ := set $imgs "voice.freeswitch.image" $v.voice.freeswitch.image.tag -}}
{{- if $v.voice.labPbx.enabled -}}{{- $_ := set $imgs "voice.labPbx.image" $v.voice.labPbx.image.tag -}}{{- end -}}
{{- end -}}
{{- if $v.postgresql.enabled -}}{{- $_ := set $imgs "postgresql.image" $v.postgresql.image.tag -}}{{- end -}}
{{- if $v.redis.enabled -}}{{- $_ := set $imgs "redis.image" $v.redis.image.tag -}}{{- end -}}
{{- range $nome, $tag := $imgs -}}
{{- if not $tag -}}
{{- $e = append $e (printf "%s.tag: falta a tag da imagem (o chart não escolhe uma por omissão; para o servidor e a web chega --set image.tag=<git describe>)" $nome) -}}
{{- else if and $prod (eq (toString $tag) "latest") -}}
{{- $e = append $e (printf "%s.tag=latest: em produção a imagem é referida por tag imutável" $nome) -}}
{{- end -}}
{{- end -}}

{{- /* dados */ -}}
{{- if and $prod $v.postgresql.enabled -}}
{{- $e = append $e "postgresql.enabled: o Postgres do chart é um pod único sem réplica nem backup — só laboratório. Em produção a base é externa (DATABASE_URL no Secret)" -}}
{{- end -}}
{{- if and $prod $v.redis.enabled -}}
{{- $e = append $e "redis.enabled: o Redis do chart é um pod único sem persistência nem password — só laboratório. Em produção usa externalRedis" -}}
{{- end -}}
{{- if ne (toString $v.postgresql.enabled) (toString $v.secrets.create) -}}
{{- $e = append $e "postgresql.enabled e secrets.create andam juntos (laboratório): a password da base e o DATABASE_URL são gerados no mesmo Secret. Com base externa usa secrets.existingSecret" -}}
{{- end -}}
{{- if and (include "dm.serverMulti" .) (not (include "dm.redisUrl" .)) (not $v.externalRedis.fromSecret) -}}
{{- $e = append $e "Redis: com mais de uma réplica do servidor (ou HPA) o REDIS_URL é obrigatório (ADR-0006 §2) — externalRedis.url, externalRedis.fromSecret=true, ou redis.enabled em laboratório" -}}
{{- end -}}

{{- /* migrações */ -}}
{{- if not (has $v.server.migrations.mode (list "startup" "job")) -}}
{{- $e = append $e "server.migrations.mode: startup | job" -}}
{{- end -}}
{{- if eq $v.server.migrations.mode "job" -}}
{{- if or (not $v.secrets.existingSecret) $v.postgresql.enabled -}}
{{- $e = append $e "server.migrations.mode=job corre ANTES de o chart criar recursos: exige secrets.existingSecret e base externa" -}}
{{- end -}}
{{- end -}}

{{- /* gravações */ -}}
{{- if and (include "dm.serverMulti" .) (not $v.recordings.existingClaim) (eq $v.recordings.accessMode "ReadWriteOnce") (not $v.recordings.allowReadWriteOnceWithReplicas) -}}
{{- $e = append $e "recordings.accessMode=ReadWriteOnce com mais de uma réplica do servidor: num cluster multi-nó as réplicas ficam presas ao nó do volume (ou em Pending). Usa ReadWriteMany com uma StorageClass que o suporte, um existingClaim RWX, uma réplica, ou — só num cluster de UM nó — recordings.allowReadWriteOnceWithReplicas=true" -}}
{{- end -}}

{{- /* ingress */ -}}
{{- if and $prod $v.ingress.enabled (not $v.ingress.tls.clusterIssuer) (not $v.ingress.tls.existingSecret) -}}
{{- $e = append $e "ingress.tls: em produção indica ingress.tls.clusterIssuer (cert-manager) ou ingress.tls.existingSecret" -}}
{{- end -}}
{{- if and $v.ingress.tls.clusterIssuer $v.ingress.tls.existingSecret -}}
{{- $e = append $e "ingress.tls.clusterIssuer e ingress.tls.existingSecret são exclusivos" -}}
{{- end -}}

{{- /* media */ -}}
{{- if and $v.coturn.enabled $prod (not $v.coturn.externalIP) -}}
{{- $e = append $e "coturn.externalIP: falta o IP público do relay (o do LoadBalancer). É o que o coturn anuncia e o que o TURN_HOST do servidor leva" -}}
{{- end -}}
{{- if and (not $v.coturn.enabled) (not $v.server.turnHost) -}}
{{- $e = append $e "server.turnHost: sem o coturn do chart, indica o host:porta do TURN externo (media em Kubernetes é relay-only)" -}}
{{- end -}}

{{- /* gRPC */ -}}
{{- if and $v.server.grpc.enabled (not $v.server.grpc.tlsSecretName) (not $v.server.grpc.certManager.enabled) -}}
{{- $e = append $e "server.grpc.enabled: o gRPC interno exige mTLS — server.grpc.tlsSecretName (tls.crt, tls.key, ca.crt) ou server.grpc.certManager.enabled=true" -}}
{{- end -}}

{{- /* voz */ -}}
{{- if and $v.voice.enabled (not $v.server.internal.enabled) -}}
{{- $e = append $e "voice.enabled exige server.internal.enabled=true: o IVR do FreeSWITCH fala com o listener interno (:8181)" -}}
{{- end -}}
{{- if and $v.voice.enabled $prod $v.voice.labPbx.enabled -}}
{{- $e = append $e "voice.labPbx.enabled: o PBX de laboratório não entra em produção — o PBX é do cliente e vive fora do cluster" -}}
{{- end -}}
{{- if and $v.voice.enabled $prod (not $v.voice.kamailio.tls.existingSecret) -}}
{{- $e = append $e "voice.kamailio.tls.existingSecret: em produção o certificado do bordo SIP (5061) vem de um Secret kubernetes.io/tls criado fora do chart" -}}
{{- end -}}
{{- range $i, $t := $v.voice.trunks -}}
{{- if or (not $t.ip) (not (hasKey $t "mask")) (not $t.tag) -}}
{{- $e = append $e (printf "voice.trunks[%d]: cada origem leva ip, mask e tag (group e port são opcionais: 1 e 0)" $i) -}}
{{- end -}}
{{- end -}}

{{- /* a central de uma organização (ADR-0016) */ -}}
{{- if and $v.voice.enabled $v.voice.centrais.enabled (not $v.voice.centrais.edgeCidrs) -}}
{{- $e = append $e "voice.centrais.edgeCidrs: com as centrais ligadas, o FreeSWITCH tem de saber de onde fala o bordo — vazia, o IVR rejeita todas as chamadas de centrais (fecha por omissão, voice/cluster/freeswitch-entrypoint.sh)" -}}
{{- end -}}

{{- /* ponte telefone↔sala */ -}}
{{- if $v.server.phoneBridge.enabled -}}
{{- if not $v.server.phoneBridge.freeswitchIPs -}}
{{- $e = append $e "server.phoneBridge.freeswitchIPs: a ponte só aceita o FreeSWITCH por IP EXACTO e, vazia, não arranca (fail-closed, server/src/config.rs). Um pod não tem IP fixo: ver README §«Ponte telefone↔sala» antes de ligar" -}}
{{- end -}}
{{- if include "dm.serverMulti" . -}}
{{- $e = append $e "server.phoneBridge.enabled com mais de uma réplica do servidor: o IVR pede o destino a UMA réplica qualquer e a sala vive noutra (o SFU é por pod) — ver README §«Ponte telefone↔sala»" -}}
{{- end -}}
{{- end -}}
{{- if and $v.server.telephony.eslAddr (not $v.secrets.existingSecret) -}}
{{- $e = append $e "server.telephony.eslAddr exige a chave TELEPHONY_ESL_PASSWORD num secrets.existingSecret" -}}
{{- end -}}

{{- if $e -}}
{{- fail (printf "\n\ndelonix-meet: a configuração foi recusada (%d):\n  - %s\n" (len $e) (join "\n  - " $e)) -}}
{{- end -}}
{{- end }}
