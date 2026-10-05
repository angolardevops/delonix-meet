-- «Ligar a…» a partir da sala (docs/ligar-a-partir-da-sala.md, F1).
--
-- 1) Capacidade `sessions.dial_out` (catálogo v4, ADR-0008, decisão D2): owner/admin
--    permitem; member/external_guest recusam; os papéis personalizados recusam até
--    um admin a conceder (fail-closed, como na 0088 e na 0089). AINDA SEM IMPOSIÇÃO:
--    a rota chega na PR seguinte, por isso `enforced_at` está vazio no catálogo.
-- 2) `room_dial_outs` passa a poder apontar para um RAMAL da própria organização
--    (`extension_id`) em vez de um número E.164: `number_e164` deixa de ser
--    obrigatório. Que uma linha nova tenha número OU ramal é regra do SERVIÇO, não
--    um CHECK: o `ON DELETE SET NULL` actualiza a linha quando o ramal é apagado, e
--    um CHECK (mesmo NOT VALID) chumbaria esse apagamento. O histórico fica sem
--    destino mas com `display_name`.
-- 3) Um só pedido vivo por (sala, ramal): dois cliques ou duas anfitriãs não fazem
--    tocar o mesmo ramal duas vezes.
INSERT INTO system_role_capability_defaults (system_key, capability, value)
VALUES ('owner', 'sessions.dial_out', 'allow'),
       ('admin', 'sessions.dial_out', 'allow'),
       ('member', 'sessions.dial_out', 'deny'),
       ('external_guest', 'sessions.dial_out', 'deny')
ON CONFLICT DO NOTHING;

SELECT seed_system_roles(id) FROM organizations;

ALTER TABLE room_dial_outs
    ALTER COLUMN number_e164 DROP NOT NULL,
    ADD COLUMN extension_id UUID REFERENCES voice_extensions(id) ON DELETE SET NULL;

CREATE UNIQUE INDEX room_dial_outs_ramal_vivo_uidx
    ON room_dial_outs (room_id, extension_id)
    WHERE extension_id IS NOT NULL AND status IN ('queued','dialing','ringing','in_call');
