-- Configuração de armazenamento remoto ao nível da plataforma.
-- Permite apontar gravações e anexos para TrueNAS (NFS) ou Nextcloud (WebDAV).
-- Um único registo (id=1); upsert na aplicação.

CREATE TABLE IF NOT EXISTS platform_storage (
    id              SERIAL PRIMARY KEY,
    storage_type    TEXT    NOT NULL DEFAULT 'local'
                            CHECK (storage_type IN ('local','nfs','webdav')),
    -- NFS: endereço e path de exportação (ex.: 192.168.1.10, /mnt/pool/meet)
    nfs_server      TEXT,
    nfs_path        TEXT,
    -- WebDAV / Nextcloud: URL base, credenciais, path remoto
    webdav_url      TEXT,
    webdav_user     TEXT,
    webdav_password TEXT,   -- guardado em claro (cifrado pelo servidor se STORAGE_ENCRYPT=1)
    webdav_path     TEXT    NOT NULL DEFAULT '/remote.php/dav/files/{user}/Delonix',
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
