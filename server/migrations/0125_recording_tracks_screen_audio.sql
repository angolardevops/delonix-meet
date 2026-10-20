-- A pista de ÁUDIO DO ECRÃ passa a caber na tabela (fuga 1 do `049948388`).
--
-- O que estava errado: o `CHECK` da 0124 foi escrito a partir do COMENTÁRIO do
-- `open_track` («"video" | "screen" | "audio"»), não a partir do SFU. O SFU
-- produz um quarto valor — `screen-audio`, o som do sistema de quem partilha
-- ecrã (`sfu.rs`: `is_screen_audio` → `kind = "screen-audio"`), com slot,
-- subscrição e FICHEIRO próprios, e o `activate_publication` grava-o
-- explicitamente. Resultado medido a 2026-10-18 no `049948388`: o `INSERT`
-- dessa pista era recusado pela base, o `registar_pistas_iso` escrevia um
-- `tracing::error!` e seguia, e o ficheiro ficava em `iso-<gravação>/` para
-- sempre sem nenhuma linha que o pudesse pedir. Ninguém via um erro.
--
-- PORQUE SE ACRESCENTA O VALOR em vez de o mapear para um dos três:
--
--   * para `audio` perdia-se a distinção que é o PROPÓSITO do ISO. Quem
--     partilha ecrã com som publica DUAS pistas de áudio — o microfone
--     (`audio`) e o som do sistema (`screen-audio`) — e o ADR-0014 §3 pede
--     cada faixa «à parte, antes de qualquer mistura». Duas linhas `audio` do
--     mesmo `publisher_id` são indistinguíveis na listagem, e a pós-produção
--     fica sem saber qual é a voz;
--   * para `screen` mentia-se sobre o tipo de media. Quem serve escolhe o
--     `Content-Type` por `kind.ends_with("audio")` (`recordings.rs`), logo um
--     OGG registado como `screen` saía como `application/octet-stream`.
--
-- Ou seja: o `CHECK` é que estava dessincronizado do SFU, não o `kind`. O
-- vocabulário passa a ter UMA fonte de verdade em código
-- (`recorder::REC_TRACK_KINDS`) e o `scripts/check-recording-track-kinds.sh`
-- recusa que as duas listas voltem a divergir.
ALTER TABLE recording_tracks DROP CONSTRAINT IF EXISTS recording_tracks_kind_check;
ALTER TABLE recording_tracks
    ADD CONSTRAINT recording_tracks_kind_check
    CHECK (kind IN ('video', 'screen', 'audio', 'screen-audio'));
