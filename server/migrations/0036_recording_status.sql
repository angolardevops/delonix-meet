-- Uma gravação que falha a compor deixa de desaparecer em silêncio.
--
-- Medido a 2026-08-25, a gravar de verdade: quando o ffmpeg falhava, o erro
-- ficava no log do SERVIDOR, o directório temporário era apagado, e a
-- biblioteca não mostrava nada — nem a gravação, nem que houve uma tentativa.
-- Do lado do anfitrião, que carregou em «gravar» e viu o indicador aceso
-- durante a reunião inteira, o resultado é indistinguível de nunca ter gravado.
--
-- É o pior tipo de falha: silenciosa, e num artefacto que ninguém consegue
-- refazer depois de a reunião acabar.

ALTER TABLE recordings
    -- 'ready' = ficheiro pronto. 'failed' = houve tentativa e não há ficheiro.
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'ready',
    -- Causa em linguagem de utilizador. O detalhe técnico (stderr do ffmpeg,
    -- caminhos) fica no log: aqui só entra o que se pode mostrar a alguém.
    ADD COLUMN IF NOT EXISTS failure_reason TEXT;

-- As linhas que já existem são gravações que ficaram prontas — o default trata
-- disso, mas fica explícito para quem ler a migração daqui a um ano.
UPDATE recordings SET status = 'ready' WHERE status IS NULL;

-- O painel pergunta «falhou alguma?», não «lista tudo e filtra».
CREATE INDEX IF NOT EXISTS idx_recordings_failed
    ON recordings (created_at DESC) WHERE status <> 'ready';
