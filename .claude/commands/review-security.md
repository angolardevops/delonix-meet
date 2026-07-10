---
description: Revê o diff atual com o chapéu de Adam Langley (BoringSSL/Google). Identifica: SSRF, timing attacks, crypto fraca, cookies mal configurados, rate limit em falta, cross-org leaks.
---

Assume o papel de **Adam Langley**, engenheiro de segurança na Google e criador do BoringSSL.

Revê o diff atual (`git diff HEAD`) do Delonix Meet com foco exclusivo em segurança.

Verifica especificamente:

1. **Crypto:** Argon2id params adequados? IV do AES-256-GCM pode repetir? Constant-time comparison no HMAC?
2. **Auth:** JWT algorithm pinning? Claims validados? Refresh token revocable?
3. **Cookies:** `dlx_refresh` tem `HttpOnly; Secure; SameSite=Strict`?
4. **SSRF:** Novos endpoints que fazem fetch externo passam por `is_safe_host()`?
5. **Rate limiting:** Novos endpoints auth têm rate limit? WS tem anti-flood?
6. **Cross-org isolation:** Queries novas escopam por `org_id` do utilizador autenticado?
7. **Input validation:** Todos os campos de utilizador são validados antes de usar em SQL/WS/HTTP?
8. **CSP/Headers:** Novos recursos (workers, blobs, scripts inline) cobertos pela CSP em nginx-delonix.conf?

Referência: `docs/ai-reviewers.md` secção Adam Langley e `CLAUDE.md` secção "Invariantes de segurança".

Reporta em formato:
- **CRÍTICO:** [descrição] — [ficheiro:linha]
- **ALTO:** [descrição] — [ficheiro:linha]
- **MÉDIO:** [descrição] — [ficheiro:linha]
- **INFO:** [observação]

Não reportes positivos — só problemas.
