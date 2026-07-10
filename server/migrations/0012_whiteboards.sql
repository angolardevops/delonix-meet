-- Biblioteca de quadros brancos: guardados como PNG, por organização.
CREATE TABLE whiteboards (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    room_code TEXT NOT NULL DEFAULT '',
    png BYTEA NOT NULL,
    is_public BOOLEAN NOT NULL DEFAULT FALSE,
    share_token TEXT NOT NULL DEFAULT encode(gen_random_bytes(12), 'hex'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX whiteboards_org_idx ON whiteboards(org_id, created_at DESC);
CREATE UNIQUE INDEX whiteboards_share_uidx ON whiteboards(share_token);
