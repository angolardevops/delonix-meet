# E2EE do Delonix Meet — protocolo, e o que ele NÃO protege

> Estado a 2026-08-25, escrito a partir do código (`web/src/e2ee.ts`,
> `server/src/recorder.rs`), não de intenções.
>
> O mandato é explícito: **não se declara segurança só porque se usa AES-GCM**.
> Este documento existe para dizer o que a implementação faz, com precisão
> suficiente para ser auditada — e, sobretudo, **o que ela não faz**. A metade
> de baixo é a que interessa a quem tem de decidir se isto serve para o seu caso.

---

## 1. O que está implementado

Encriptação de frames com **Insertable Streams** (`RTCRtpScriptTransform`, ou
`createEncodedStreams` nos browsers que ainda não o têm). Cada frame já
codificado (VP8 ou Opus) é cifrado **antes** da packetização RTP, num Worker
dedicado. O SFU reencaminha bytes que não consegue abrir.

### Formato do frame

```
[ header em claro | ciphertext + tag GCM (16B) | IV (12B) ]
                   └── AES-256-GCM, AAD = header ──┘
```

O tamanho do header depende do tipo de frame — são os bytes que os
packetizers e depacketizers precisam de ler:

| Tipo | Header em claro |
|---|---|
| Vídeo, keyframe | 10 bytes |
| Vídeo, delta | 3 bytes |
| Áudio (Opus) | 1 byte |

O header fica **legível** de propósito, mas entra como *additional
authenticated data*: é legível, não é falsificável. Um intermediário que lhe
troque um bit faz a autenticação falhar e o frame é descartado. Há testes que
o fixam (`o_header_vai_autenticado_nao_so_em_claro`).

O **IV é aleatório por frame** (96 bits, `crypto.getRandomValues`) e viaja no
fim do frame, em claro — como é normal, já que a tag o autentica.

### Derivação da chave

```
chave = PBKDF2-SHA256(
    password   = frase-chave partilhada fora da plataforma,
    salt       = "delonix-meet:e2ee:" + código-da-sala,
    iterations = 250 000,
    dkLen      = 256 bits
)
```

A chave **nunca é enviada ao servidor** — com uma excepção explícita, a
gravação (§4).

### Duas implementações do mesmo formato

O cifrador é JavaScript, o decifrador da gravação é Rust (`decrypt_e2ee`). São
duas implementações independentes, sem nada que as obrigue a concordar. Se
divergirem, as gravações de salas E2EE saem em **ruído** e ninguém dá por isso:
o writer descarta o que não autentica e o ficheiro sai vazio, sem erro nenhum.
Seis testes em `recorder.rs` reconstroem em Rust, byte a byte, o que o worker
produz — é essa a rede.

---

## 2. Do que isto protege

**Do servidor e de quem o operar.** É a propriedade central e é real: o SFU
reencaminha, grava e observa bytes cifrados. Um administrador da plataforma,
alguém que comprometa o nó, ou quem tenha acesso ao armazenamento, não obtém
áudio nem vídeo.

**De quem estiver no caminho de rede.** O DTLS/SRTP já o fazia entre par e SFU;
o E2EE estende-o *através* do SFU.

**De adulteração do conteúdo.** GCM autentica payload e header. Um frame
alterado é descartado, não descodificado.

---

## 3. Do que isto NÃO protege — a metade que costuma faltar

### 3.1 Não há autenticação por remetente

**Todos os participantes partilham a MESMA chave simétrica**, e é com ela que
cifram e decifram. Consequência directa: **qualquer participante pode forjar
frames que se fazem passar por outro participante**. A criptografia prova que o
frame veio de *alguém que tem a chave da sala* — não de quem diz ter vindo.

Isto protege contra o servidor. Não protege participantes uns dos outros.

### 3.2 Não há forward secrecy, e a chave é eterna

A chave é uma **função pura** de (frase-chave, código da sala). O mesmo par dá
sempre a mesma chave — hoje, no mês passado, e daqui a um ano.

Duas consequências que valem por si:

1. **Quem descobrir a frase-chave decifra todo o passado.** Qualquer captura de
   tráfego guardada, qualquer gravação cifrada, qualquer cópia do armazenamento
   — tudo, retroactivamente.
2. **Uma reunião recorrente com frase-chave fixa usa UMA chave em todas as
   sessões.** Não há rotação. Um participante que saia da organização continua
   a poder decifrar as reuniões seguintes enquanto a frase não mudar.

### 3.3 Não há post-compromise security

Um dispositivo comprometido continua a decifrar tudo depois de o ataque
terminar. Não há como expulsar criptograficamente um membro: só mudando a
frase-chave e recomeçando.

### 3.4 A frase-chave é o elo fraco, e o servidor tem o ciphertext

Quem tiver o ciphertext — o operador do servidor, à cabeça — pode montar um
ataque de dicionário **offline** contra a frase-chave. As 250 000 iterações de
PBKDF2-SHA256 estão **abaixo** da recomendação corrente da OWASP (600 000), e
PBKDF2 não tem resistência a memória — um ataque com GPU é barato.

Uma frase-chave escolhida por uma pessoa (`reuniao2026`) não sobrevive a isto.
Uma frase de quatro ou cinco palavras aleatórias sobrevive com folga.

**Não se subiu o número de iterações nesta alteração**, e a razão tem de ser
dita: a derivação não é negociada. Clientes com contagens diferentes derivam
chaves diferentes e **deixam de se ouvir**, a meio de uma reunião em curso.
Subir exige que todos os participantes tenham a mesma versão da aplicação — o
que já é verdade para o formato do frame, mas aqui a falha é silenciosa (ninguém
ouve ninguém) em vez de evidente. É uma decisão do dono do produto, não uma
que se tome de passagem.

### 3.5 Metadados não são protegidos, e são muitos

Cifra-se o CONTEÚDO. Fica visível ao servidor: quem está na sala, quando entrou
e saiu, quem fala e quando (o selector de oradores usa energia de áudio),
tamanhos e ritmo dos frames, quem partilha ecrã, nomes, chat, legendas,
transcrições, quadro branco, sondagens.

**O chat e as legendas NÃO são E2EE.** Passam pelo servidor em claro. O
`mls.rs` descreve a camada que resolveria isso; não está implementada.

### 3.6 Limite de utilização da chave

Com IV aleatório de 96 bits, a NIST (SP 800-38D §8.3) limita a **2³²
invocações** por chave para manter a probabilidade de colisão abaixo de 2⁻³².
Uma colisão de IV com a mesma chave em GCM é séria: permite recuperar a chave
de autenticação.

Aqui o orçamento é **partilhado por todos os participantes**, porque a chave é
a mesma. Com ~80 frames por segundo por emissor (30 fps de vídeo + 50 de áudio)
e dez participantes, 2³² frames dão da ordem de **dois meses de emissão
contínua**. Uma reunião nunca lá chega — mas uma frase-chave reutilizada numa
sala recorrente **acumula**, porque a chave não muda. É outra razão para rodar
a frase.

---

## 4. A excepção: gravação no servidor

Uma gravação server-side de uma sala E2EE só é possível se o servidor puder
decifrar. O desenho assume-o em vez de o disfarçar:

- **É o anfitrião que cede a chave**, explicitamente, com uma confirmação na
  interface, ao ligar a gravação (`server-record`).
- A chave vai no WebSocket (WSS) e **fica só em memória** no servidor, dentro
  da sessão de gravação. Não é escrita na base de dados nem em ficheiro.
- Sem cedência, a gravação server-side de uma sala E2EE não acontece.

**O que isto significa, dito sem rodeios:** a partir do momento em que a
gravação é ligada, aquela reunião deixa de ser ponta-a-ponta. O servidor
decifra os frames para os compor. Os participantes vêem o indicador de
gravação; o que o indicador não diz é que a cedência da chave aconteceu.

### Duas perguntas que estavam por responder, e ficaram

**Pode a chave aparecer num log?** Não hoje — nenhum log imprime a mensagem
inteira, verificado. Mas o `ClientMsg` deriva `Debug` e a chave viajava lá
dentro como uma `String` normal: bastava um `tracing::debug!(?msg)` acrescentado
por boas razões para a chave AES-256 da sala ir parar ao ficheiro de log, em
base64, pronta a ler. O campo passou a ser um `Secret`, cujo `Debug` imprime
`[segredo redigido]` — e há teste que o fixa. A garantia deixa de depender de
alguém se lembrar.

**Sobrevive em memória?** A tabela de chaves do `Aes256Gcm` é limpa no `Drop`
(a crate `cipher` traz a feature `zeroize`), mas os **bytes crus** descodificados
do base64 ficavam numa `Vec<u8>` largada sem sobrescrever — chave AES-256 em
claro numa alocação libertada. Passaram a ser limpos à mão, tal como a `String`
base64 dentro do `Secret`. Não é uma garantia forte em Rust (a `String` pode ter
sido realocada antes disso), mas fecha a janela óbvia.

---

## 5. Onde isto está no espectro

| Propriedade | Delonix hoje | Com MLS (RFC 9420) |
|---|---|---|
| Conteúdo opaco ao servidor | ✅ | ✅ |
| Adulteração detectada | ✅ | ✅ |
| Autenticação por remetente | ❌ | ✅ |
| Forward secrecy | ❌ | ✅ |
| Post-compromise security | ❌ | ✅ |
| Expulsar um membro sem trocar a frase | ❌ | ✅ |
| Chat e legendas cifrados | ❌ | ✅ |
| Chave sem intervenção humana | ❌ (frase partilhada à mão) | ✅ |

O que está implementado é razoável contra o **servidor**, que é a ameaça que a
soberania de dados nomeia. Não é comparável a Signal nem ao MLS, e não deve ser
descrito como se fosse.

---

## 6. Como dizer isto a um cliente

**Pode dizer-se:** «o conteúdo de áudio e vídeo é cifrado no dispositivo e o
servidor não o consegue abrir; a chave nunca lhe chega, excepto se o anfitrião
a ceder de propósito para gravar.»

**Não se pode dizer:** «encriptação ponta-a-ponta como o Signal», «forward
secrecy», «só os participantes conseguem ler», «o chat é encriptado
ponta-a-ponta», nem apresentar o `/api/mls` como capacidade — as rotas foram
DESREGISTADAS a 2026-08-25 justamente por serem stubs sem autenticação que
respondiam `"status": "delivered"` a qualquer pessoa. Verificado depois da
alteração: as três devolvem **404**.

---

## 7. Por ordem de valor, o que falta

1. **Rotação de chave por sessão** — mata a §3.2 (chave eterna) com muito menos
   trabalho do que o MLS completo. É por onde se começa.
2. **Autenticação por remetente** — §3.1.
3. **MLS a sério** (RFC 9420) — resolve 3.1, 3.2, 3.3 e traz o chat.
4. **Frase-chave gerada, não escolhida** — mata a §3.4 sem tocar na cripto.
5. **Subir o PBKDF2 para ≥600 000** — com um plano para a incompatibilidade.
