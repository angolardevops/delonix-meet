#!/usr/bin/env bash
# ============================================================
#  Fitness function: UMA crypto provider do rustls na árvore (R308, e antes dela).
#
#  PORQUE EXISTE. O rustls 0.23 não escolhe sozinho entre `ring` e `aws_lc_rs`:
#  com as duas compiladas, a primeira ligação TLS do processo entra em panic —
#
#      Could not automatically determine the process-level CryptoProvider
#
#  — e o panic é em QUALQUER TLS, não só no de quem trouxe a segunda. Este repo
#  está todo em `ring` (o `dtls` do `webrtc`, que é o SFU, e o `tls-rustls` do
#  `sqlx`), e o comentário do `rustls` no `server/Cargo.toml` diz porquê.
#
#  JÁ ACONTECEU DUAS VEZES, as duas por uma dependência nova trazer o `aws-lc`
#  sem ninguém reparar: a primeira com o `rustls` directo do SMPP, a segunda com
#  a feature `default-https-client` do `aws-sdk-s3` (R308). Na segunda rebentou
#  17 testes do SFU e do SMPP — nenhum deles com nada a ver com S3 —, e só no
#  CI, depois de 28 minutos de testes.
#
#  O que se mede: as features do `rustls` na árvore REAL do binário. Se
#  aparecerem as duas, falha e diz quem as trouxe.
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

r=$'\e[31m'; g=$'\e[32m'; y=$'\e[33m'; z=$'\e[0m'
command -v cargo >/dev/null 2>&1 || { echo "  ${y}· sem cargo — a crypto provider NÃO foi medida${z}"; exit 0; }

arvore=$(cargo tree --manifest-path server/Cargo.toml -e features -i rustls --no-dedupe 2>/dev/null)
if [ -z "$arvore" ]; then
  echo "${r}✗ crypto: não consegui ler a árvore de features do rustls${z}"
  exit 1
fi

tem_ring=0; tem_awslc=0
grep -qE 'rustls feature "ring"' <<<"$arvore" && tem_ring=1
grep -qE 'rustls feature "aws_lc_rs"' <<<"$arvore" && tem_awslc=1

if [ "$tem_ring" = 1 ] && [ "$tem_awslc" = 1 ]; then
  echo "${r}✗ crypto: o rustls tem as DUAS providers («ring» e «aws_lc_rs») na árvore.${z}"
  echo "     O rustls recusa-se a escolher e a primeira ligação TLS do processo"
  echo "     entra em panic — incluindo o DTLS do SFU, que não tem nada a ver"
  echo "     com quem trouxe a segunda. Quem as traz:"
  grep -E 'feature "(aws_lc_rs|rustls-aws-lc|aws-lc-rs)"' <<<"$arvore" | sed 's/^/       /' | sort -u | head -10
  echo "     Põe a dependência nova em «ring» (ver o comentário do rustls no server/Cargo.toml)."
  exit 1
fi
if [ "$tem_ring" = 0 ] && [ "$tem_awslc" = 0 ]; then
  echo "${r}✗ crypto: o rustls está na árvore sem provider nenhuma — nada faz TLS${z}"
  exit 1
fi

qual=$([ "$tem_ring" = 1 ] && echo ring || echo aws_lc_rs)
if [ "$qual" != ring ]; then
  echo "${r}✗ crypto: a árvore está em «$qual» e este repo está em «ring».${z}"
  echo "     Mudar de provider é uma decisão com ADR, não um efeito de uma feature."
  exit 1
fi
echo "${g}✓ crypto: uma só provider do rustls na árvore («ring»), como o Cargo.toml manda${z}"
