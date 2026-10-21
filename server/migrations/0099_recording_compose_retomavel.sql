-- Composição de gravações RETOMÁVEL.
--
-- O QUE ESTAVA MAL: `recorder::finalize` era um `tokio::spawn` nu. A linha
-- nascia em `processing` (bom: a biblioteca mostra progresso) mas o que a
-- composição precisa — o directório `tmp-<uuid>` dos segmentos e o instante do
-- primeiro pacote de cada pista — só existia na memória da tarefa. Um reinício
-- do servidor, ou seja QUALQUER rollout, deixava a gravação em `processing`
-- sem ninguém capaz de a retomar, e o varredor marcava-a `failed` ao fim de
-- `FFMPEG_TIMEOUT_SECS + 600` (4200 s por omissão). Quem gravou olhava para
-- «a compor, 37 %» durante setenta minutos e não tinha como voltar a pedir:
-- a reunião já tinha acabado.
--
-- Os orçamentos medidos a 2026-10-07 faziam disso uma certeza, não um risco:
-- o ffmpeg tem 3600 s, o drain tem 52 s e o K8s manda SIGKILL aos 60 s.
--
-- O DESENHO é o mesmo da fila da transcrição (reserva + token + tentativas,
-- colunas na própria `recordings`), com as regras em
-- `delonix_meet_domain::content::composition`. Não há fila nova nem broker: o
-- padrão da casa é tabela + varredor + `FOR UPDATE SKIP LOCKED`.

-- O manifesto: o que basta para compor sem a sessão em memória (directório,
-- pistas com `starts_at_ms` já resolvido, duração, quem gravou). JSONB e não
-- colunas porque a forma é da composição e muda com ela, não com o esquema.
--
-- NÃO leva a chave E2EE, e não é esquecimento: os segmentos no disco já estão
-- em claro (a chave cedida pelo anfitrião é usada nos writers, à entrada, e
-- morre com a sessão). Retomar não precisa dela — este manifesto não guarda
-- nada que o servidor não guardasse já.
-- A reserva é CURTA (3 min) e renovada a cada 30 s por quem compõe — o mesmo
-- batimento que já escrevia `progress_at`, agora também para a posse. A versão
-- longa (o tecto do ffmpeg mais folga, 75 min) estragava o caso que isto existe
-- para salvar: num rollout o pod morria com 75 minutos de reserva na mão e o
-- pod novo esperava-os inteiros. Trocar setenta minutos de espera por setenta e
-- cinco não é um remédio. Regras em `composition::{LEASE, RENEW_EVERY}`.
ALTER TABLE recordings
    ADD COLUMN compose_manifest          JSONB       NULL,
    ADD COLUMN compose_lease_token       TEXT        NULL,
    ADD COLUMN compose_lease_expires_at  TIMESTAMPTZ NULL,
    ADD COLUMN compose_attempts          INT         NOT NULL DEFAULT 0
                                         CHECK (compose_attempts >= 0);

-- A reivindicação: `processing`, com manifesto, sem reserva em vigor. O índice
-- é parcial porque em regime quase nenhuma linha o é — as gravações passam a
-- vida em `ready`.
CREATE INDEX recordings_compose_claim_idx
    ON recordings (compose_lease_expires_at NULLS FIRST, created_at)
    WHERE status = 'processing' AND compose_manifest IS NOT NULL;

-- As que já estão em `processing` nesta base nasceram sem manifesto e são
-- irrecuperáveis: ficam a zero tentativas e sem manifesto, e o varredor
-- fecha-as como antes. Não se inventa um manifesto que não se tem.
