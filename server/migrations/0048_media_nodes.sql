-- Inventário de nós de media (G10). Cada pod faz upsert da sua linha a cada
-- 15 s; o estado (a servir / a drenar / sem sinal) deriva-se da idade do
-- último batimento na leitura — um nó que morre não escreve que morreu.
CREATE TABLE media_nodes (
    node_id          UUID PRIMARY KEY,
    hostname         TEXT NOT NULL,
    version          TEXT NOT NULL,
    edition          TEXT NOT NULL,
    started_at       TIMESTAMPTZ NOT NULL,
    last_seen_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    draining         BOOLEAN NOT NULL DEFAULT FALSE,
    rooms            INT NOT NULL DEFAULT 0,
    peers            INT NOT NULL DEFAULT 0,
    ws_connections   INT NOT NULL DEFAULT 0,
    live_broadcasts  INT NOT NULL DEFAULT 0,
    peer_capacity    INT NULL
);
CREATE INDEX media_nodes_last_seen_idx ON media_nodes (last_seen_at);
