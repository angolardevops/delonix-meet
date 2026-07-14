-- Ata AI: quando o resumo via LLM local (ai.rs/Ollama) ficou pronto.
-- NULL = a ata ainda é a versão por regras do cliente (ou não há ata).
-- Usado pela integração Odoo (nk_delonix_meet) para saber se o MoM é final.
ALTER TABLE meetings ADD COLUMN IF NOT EXISTS minutes_ai_at TIMESTAMPTZ;
