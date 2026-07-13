-- Amostras de qualidade de chamada (QoS) reportadas pelos clientes (~1/30s).
-- Alimenta o cartão "Qualidade das chamadas" do admin (org_stats): RTT médio,
-- perda média e % de amostras boas (<2% perda) vs fracas (>5%).
-- O agregado é org-scoped via org_members (rooms não têm org_id).
CREATE TABLE IF NOT EXISTS call_quality_samples (
    id BIGSERIAL PRIMARY KEY,
    room_id UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    rtt_ms INT,
    loss_pct REAL NOT NULL DEFAULT 0,
    up_kbps INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_cqs_user_time ON call_quality_samples (user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_cqs_time ON call_quality_samples (created_at);
