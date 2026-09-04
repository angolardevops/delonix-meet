-- Amostras de qualidade de chamada: das TRÊS colunas originais (rtt, perda,
-- uplink) para o conjunto que permite diagnosticar em vez de só constatar.
--
-- Porquê: com três números não se distingue rede do cliente, CPU do cliente, ou
-- o nó a servir camada a mais — e sem isso não há SLO defensável (§4.4/§10 do
-- mandato). Uma meta publicada sobre uma medição que não existe é uma meta
-- inventada.
--
-- Todas as colunas são NULLABLE de propósito: um cliente que ainda não tenha a
-- versão nova continua a reportar as três de sempre, e a amostra dele continua
-- a valer. Uma coluna NOT NULL aqui faria a ingestão recusar clientes antigos —
-- e perder-se-iam exactamente as amostras das sessões mais problemáticas, que
-- costumam ser as de quem tem a app em cache velha.

ALTER TABLE call_quality_samples
    -- Pontuação Delonix (0–100). Ver web/src/callQuality.ts para o modelo E
    -- para o que ele não é: não é MOS, não está calibrado contra ouvido humano.
    ADD COLUMN IF NOT EXISTS score           SMALLINT,
    ADD COLUMN IF NOT EXISTS down_kbps       INT,
    ADD COLUMN IF NOT EXISTS jitter_ms       INT,
    -- Imagem congelada no intervalo da amostra.
    ADD COLUMN IF NOT EXISTS freeze_ms       INT,
    -- Áudio ocultado por PLC: som que o utilizador NÃO ouviu. É a métrica que
    -- melhor prevê «não se percebia nada» e não existia de todo.
    ADD COLUMN IF NOT EXISTS concealment_pct REAL,
    ADD COLUMN IF NOT EXISTS frames_dropped  INT,
    ADD COLUMN IF NOT EXISTS nack            INT,
    ADD COLUMN IF NOT EXISTS pli             INT,
    ADD COLUMN IF NOT EXISTS fir             INT,
    -- A media está a pagar o desvio pelo TURN? Distingue «relay configurado»
    -- de «relay em uso», que é a pergunta que interessa na factura e na latência.
    ADD COLUMN IF NOT EXISTS turn_relay      BOOLEAN,
    -- Ex.: "relay/relay", "srflx/host".
    ADD COLUMN IF NOT EXISTS candidate_pair  TEXT,
    -- "cpu" | "bandwidth" | "other": porque é que o encoder baixou a qualidade.
    ADD COLUMN IF NOT EXISTS limited_by      TEXT;

-- O painel pergunta sempre «as más, das últimas 24 h/30 d». Um índice parcial
-- serve essa pergunta sem carregar as amostras boas, que são a esmagadora
-- maioria das linhas.
CREATE INDEX IF NOT EXISTS idx_cqs_poor_time
    ON call_quality_samples (created_at DESC)
    WHERE score IS NOT NULL AND score < 60;
