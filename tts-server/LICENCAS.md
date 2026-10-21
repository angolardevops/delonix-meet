# Licenças desta imagem — e porque ela existe sozinha

Esta imagem é **GPL-3.0-or-later**, e é a razão de ser uma imagem à parte.

O resto do Delonix Meet não é. A decisão **D3** já tinha posto esta regra de pé
para o ffmpeg: o binário da imagem do servidor é LGPL
(`scripts/check-ffmpeg-licenca.sh` impede que deixe de ser) e o que é GPL vive
**confinado à sua própria imagem**, com a sua própria oferta de código. O
`tts-server` segue esse molde, por imposição da licença e não por gosto.

## O motor

| | |
|---|---|
| Pacote | `piper-tts` **1.8.0** (PyPI), de `OHF-Voice/piper1-gpl` |
| Licença | **GPL-3.0-or-later**, como o próprio pacote a declara |
| Porquê GPL | desde a v1.3.0 o Piper **embute o espeak-ng** (GPLv3) para fonemização |
| SHA-256 do código-fonte | `830588aded347df579c91a32703e0fc2a3685d84f1e3533b14f2de69135d4904` (`piper_tts-1.8.0.tar.gz`) |

**A oferta de código cumpre-se por acompanhamento, não por promessa.** O
`Dockerfile` descarrega o *sdist* do PyPI, verifica aquele SHA-256 e deixa-o em
`/usr/share/piper-src/` **dentro da imagem**. Quem recebe o binário recebe a
fonte correspondente no mesmo artefacto — que é a forma mais simples de
satisfazer a §6 da GPL-3.0 num contentor, e a que não depende de nós mantermos
um servidor de downloads vivo durante três anos.

O repositório antigo (`rhasspy/piper`, MIT) está **arquivado e só de leitura
desde 2025-10-06** e aponta para este. Não é uma alternativa: é uma versão
congelada.

## A voz

| | |
|---|---|
| Voz | `pt_PT-tugão-medium` |
| Conjunto de dados | **CC0**, como o `MODEL_CARD` declara |
| Treino | afinada a partir da voz inglesa `lessac` (qualidade média) |
| Origem | `rhasspy/piper-voices`, revisão **`c10ece1aade47bb51c153c893d14e5bf8e5b7117`** |
| SHA-256 do modelo | `223a7aaca69a155c61897e8ada7c3b13bc306e16c72dbb9c2fed733e2b0927d4` (63 201 294 bytes) |

Uma voz é um **artefacto de terceiros**, não um detalhe de configuração: fixa-se
por revisão e por SHA-256, e a sua licença fica escrita aqui. CC0 não exige
atribuição — este registo existe para que a próxima voz, que pode exigir, não
entre sem ninguém reparar.

**É português de PORTUGAL, e o produto di-lo.** Não existe voz de português de
Angola em catálogo nenhum que se tenha encontrado (2026-10-13), e o produto não
disfarça uma pela outra: onde a pessoa escolhe, lê-se «Português (Portugal)».
Ver o ADR-0030.

## O que esta imagem NÃO faz

Não clona a voz de ninguém. Clonar a voz de um participante é tratá-la como
**biometria** — consentimento informado, prazo de retenção, decisão de
conformidade — e isso não é desta frente.
