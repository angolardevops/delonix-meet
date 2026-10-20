-- A organização pode PROIBIR o motor de voz do browser (a Web Speech do
-- Chrome/Edge), que envia o áudio aos servidores da Google.
--
-- PORQUE EXISTE: até 2026-10-18 a escolha do motor de transcrição vivia
-- inteiramente no `localStorage` do browser (`dx_asr_server`), e por isso
-- qualquer utilizador — ou qualquer extensão instalada no browser dele — a
-- alterava. Uma instalação soberana não tinha como dizer «aqui, não».
--
-- `DEFAULT TRUE` de propósito, e NÃO é descuido: proibir por omissão mudava o
-- comportamento de todas as organizações que já existem num `UPDATE` de
-- migração. Quem quer a proibição escreve-a (`PATCH /api/orgs/{id}`); o que
-- esta coluna acrescenta é a POSSIBILIDADE, não uma política nova imposta a
-- quem não a pediu.
--
-- O recuo de uma falha do ASR do servidor passou a ser o Whisper WASM local
-- (soberano) independentemente desta coluna — ver `web/src/legendasPref.ts`.
-- Esta coluna é a segunda guarda, a que não vive no cliente.
ALTER TABLE organizations
    ADD COLUMN IF NOT EXISTS allow_web_speech BOOLEAN NOT NULL DEFAULT TRUE;
