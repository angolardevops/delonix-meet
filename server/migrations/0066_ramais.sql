-- Ramais internos (extensão SIP) — Fase 1: chamada ramal-a-ramal, SÓ interna.
--
-- Diferença para voice_room (migração 0014): aquela é EFÉMERA (por reunião,
-- PIN aleatório, morre com a sala). Um ramal é PERMANENTE — 1:1 com um membro
-- da org, número curto atribuído, nunca expira. As duas tabelas não se tocam:
-- este ficheiro é estritamente aditivo, infraestrutura paralela.
--
-- Fora de âmbito nesta fase (fases seguintes do mesmo plano, não aqui):
-- alcançável do PSTN, ou ponte para uma sala de reunião em vídeo.

CREATE TABLE voice_extensions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    -- org_members tem chave composta (org_id, user_id) — sem PK própria — por
    -- isso a FK também é composta, não um UUID solto a apontar para users(id).
    member_id UUID NOT NULL,
    -- Número curto, único DENTRO da org (nunca globalmente — ver o comentário
    -- em config.rs::voice_ramais_domain_suffix sobre porque isso importa para
    -- o directório SIP).
    extension TEXT NOT NULL,
    -- AOR do SIP: globalmente único porque é o que o registar (FreeSWITCH)
    -- usa como identidade da conta, independente da org.
    sip_username TEXT NOT NULL,
    -- Argon2 (auth::hash_password) — SÓ para a nossa própria segurança em
    -- repouso (auditoria, eventual reset futuro), igual a qualquer outra
    -- password guardada neste código. NÃO é o que o SIP digest usa — ver
    -- sip_ha1 a seguir.
    sip_password_hash TEXT NOT NULL,
    -- HA1 do SIP Digest — RFC 2617: MD5(sip_username ":" realm ":" password).
    -- O `realm` aqui é o domínio SIP da org (<slug>.<VOICE_RAMAIS_DOMAIN_SUFFIX>,
    -- ver ramais.rs::sip_domain_for_org), fixo no momento em que o ramal é
    -- criado ou a password é regenerada. Existe porque o digest SIP não aceita
    -- Argon2 — precisa de decidir, a partir do desafio, se a password bate, e
    -- só um hash calculado com o MESMO algoritmo (MD5, imposto pelo protocolo,
    -- não escolhido por nós) permite isso sem guardar a password em claro.
    sip_ha1 TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, member_id) REFERENCES org_members(org_id, user_id) ON DELETE CASCADE,
    -- Um membro, um ramal (fase 1: mantém o modelo simples; nada impede uma
    -- migração futura de relaxar isto com um ADR se surgir um motivo real).
    CONSTRAINT voice_extensions_org_member_uidx UNIQUE (org_id, member_id),
    -- O número só precisa de ser único dentro da própria org.
    CONSTRAINT voice_extensions_org_ext_uidx UNIQUE (org_id, extension),
    -- A AOR SIP é global — dois ramais com o mesmo sip_username colidiriam no
    -- directório do registar, venham de que org vierem.
    CONSTRAINT voice_extensions_sip_username_uidx UNIQUE (sip_username)
);
CREATE INDEX voice_extensions_org_idx ON voice_extensions(org_id);
