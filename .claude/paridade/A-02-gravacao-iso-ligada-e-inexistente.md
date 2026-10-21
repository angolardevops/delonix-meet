---
gap: A-02
grupo: A
title: A gravação ISO está LIGADA por omissão e não existe
severidade: AFIRMAÇÃO FALSA LATENTE
estado: coluna e interruptor sem implementação
medido: 2026-10-18
base: develop @ f21e6fc47
---

Objectivo: ou a gravação ISO entrega ficheiros por fonte, ou deixa de estar ligada e de aparecer no ecrã — mas não continua a prometer-se sozinha.

Done when:

* **A decisão está tomada e escrita** num dos dois sentidos, e o ecrã concorda com a base de dados. Se for implementar: `GET /api/recordings/{id}/tracks` lista as pistas e `…/tracks/{id}/content` serve cada uma, e a observação é **o `ffprobe` de um ficheiro descarregado** a mostrar uma só fonte — não a listagem. Se for recuar: `iso_recording` passa a `DEFAULT FALSE` por migração, o interruptor sai do ecrã, e o motivo fica no ADR-0014
* Nenhum ecrã apresenta «ISO» enquanto não houver pista servida. O contrato do estúdio **já proíbe** mostrar «REC · ISO n» sem as rotas; confirmar que o `check-capability-claims.sh` apanha a palavra em locale nenhum
* Um teste prova o caminho escolhido com Postgres real: ou as pistas existem e são distintas (duas fontes → dois ficheiros com durações próprias), ou o interruptor está desligado e a API recusa a pedir ISO. Medido o controlo negativo

Fora de âmbito: a composição multi-faixa por cópia (`-itsoffset`) do ADR-0014 §2.3, que é a fatia seguinte desta mesma frente; o custo de CPU da composição (`B-05`); a gravação de chamada telefónica que escreve para um disco que ninguém lê (`C-01`, defeito gémeo noutro módulo).

Evidência já lida (2026-10-18, `f21e6fc47`):
- `server/migrations/0081_studios.sql:12` — `iso_recording BOOLEAN NOT NULL DEFAULT TRUE`. **Ligada por omissão.**
- `grep -rl "recording_tracks" server/src server/migrations` = **zero**. A tabela não existe
- `docs/reference/estudio-tv.md:196-201` — o próprio contrato nomeia o que falta: «`recording_tracks`, `GET /api/recordings/{id}/tracks` e `…/tracks/{id}/content`, e a gravação ISO em ficheiro por fonte»
- `server/src/studio.rs:125,507,589` lê e escreve a coluna; `web/src/pages/studio/tv/Fontes.tsx:179` mostra o interruptor
- ADR-0014, tabela do §Contexto — o SFU **já escreve um ficheiro por publicação** durante a gravação e **apaga-os** no fim da composição. A matéria-prima existe e é descartada
- a mesma medição do ADR-0014: ISO por cópia é **~500× mais barato** que a composição VP9 que já se faz. O custo não é o obstáculo

Nota que decide a fatia: isto é o caso mais puro de «capacidade meio feita» de todo o repositório — a base de dados diz sim, o ecrã diz sim, e não há uma linha que produza o artefacto. Um cliente de produção que ligue o estúdio vai procurar as pistas por câmara e não as encontra. Recuar é legítimo; ficar como está não é.

Portões em risco: `check-docs-drift.sh` (migração nova tem de aparecer nos docs); `check-capability-claims.sh`; e se a saída for implementar, o portão de autorização das rotas novas.
