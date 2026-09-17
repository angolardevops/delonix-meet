-- Registo SIP da organização (ADR-0009 §5): o domínio, o SBC e a conta SIP que
-- um PBX ou softphone da empresa usa. O ESTADO não se guarda: lê-se do SBC e do
-- FreeSWITCH (porta SipControl) no momento.

CREATE TABLE telephony_sip_settings (
    org_id           UUID PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    domain           TEXT NOT NULL,
    sbc_host         TEXT NOT NULL DEFAULT '',
    transport        TEXT NOT NULL CHECK (transport IN ('udp','tcp','tls')),
    srtp             TEXT NOT NULL CHECK (srtp IN ('mandatory','optional','off')),
    -- Ordem de preferência (OPUS, G722, PCMA, PCMU…).
    codecs           TEXT[] NOT NULL DEFAULT '{}',
    username         TEXT NOT NULL DEFAULT '',
    -- `enc:v1:…`, aad telephony_sip_settings.password:<org_id>. Só sai por
    -- `reveal-credentials`, com reautenticação e auditoria.
    password_sealed  TEXT NOT NULL DEFAULT '',
    updated_by       UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- O domínio decide a org de uma chamada que ENTRA pelo PBX (xml_curl): dois
-- inquilinos com o mesmo domínio tornavam a escolha arbitrária — medido com o
-- FreeSWITCH real, a chamada de uma org saiu pelos troncos da outra.
CREATE UNIQUE INDEX telephony_sip_settings_domain_uidx ON telephony_sip_settings (lower(domain));
