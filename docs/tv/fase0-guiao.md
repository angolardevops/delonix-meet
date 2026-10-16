# Guião de medição da Fase 0 (RFC-0002)

**Data:** 2026-10-14 · **Script:** [`scripts/tv-fase0-medir.sh`](../../scripts/tv-fase0-medir.sh) ·
**Compras:** [fase0-compras.md](fase0-compras.md) ·
**Decisão que isto fecha:** [ADR-0031](../adr/0031-a-captura-de-tv-entra-por-um-agente-na-caixa.md),
que está **Proposto** e só passa a Aceite com o relatório desta fase.

A Fase 0 responde a cinco perguntas. Nenhuma se responde por desenho, e três delas bloqueiam
decisões de implementação (encode onde, entradas por caixa, caminho de sintonia):

1. Marca e modelo do descodificador — e dá imagem ou **preto por HDCP**?
2. **Responde à sintonia**, e por que caminho (CEC ou IR)?
3. Que **modos reais** o capturador entrega (resolução, fps, formato)?
4. O **áudio** daquele capturador emparelha com o vídeo dele?
5. **Quantas entradas** em simultâneo antes de perder frames?

## 1. Antes de ligar nada

```bash
sudo apt install v4l-utils alsa-utils ffmpeg   # v4l2-ctl e arecord
sudo apt install cec-utils                     # só se o caminho for CEC
pip install broadlink                          # só se o caminho for IR
```

Medido a 2026-10-14 na máquina de desenvolvimento: **falta o `v4l2-ctl` e o `cec-client`**. O script
corre sem eles — cai para a enumeração do `ffmpeg` e marca a sintonia como «não medido» — mas a
grelha de fps por resolução só sai com o `v4l-utils`.

Ligar, por esta ordem: descodificador → cabo HDMI → capturador → porta USB **directa** (não o hub,
ainda). Ligar o descodificador num canal com imagem em movimento; um canal de ecrã fixo mente sobre
o fps.

## 2. Correr o guião

```bash
bash scripts/tv-fase0-medir.sh                       # tudo, amostras de 20 s
SEGUNDOS=60 bash scripts/tv-fase0-medir.sh           # amostra longa (recomendado para o tecto)
bash scripts/tv-fase0-medir.sh inventario sinal      # só um par de passos
```

O relatório sai em `~/.cache/delonix-tv-fase0/relatorio.md` e as amostras de vídeo ao lado — **fora
do repositório, de propósito**.

| Passo | Responde | Critério |
|---|---|---|
| `ambiente` | em que máquina e com que carga se mediu | a carga do host entra no relatório: esta máquina é partilhada |
| `inventario` | identidade estável e par vídeo+áudio | cada entrada tem caminho físico; coluna de áudio preenchida |
| `modos` | resolução/fps que o aparelho declara | 1080p a 30 ou 60 presentes na lista |
| `sinal` | imagem, áudio e fps entregue | YAVG ≥ 16 (há imagem) e fps entregue ≈ fps declarado |
| `carga` | o tecto do barramento | queda < 5 % no fps de cada entrada com todas a gravar |
| `sintonia` | CEC ou IR | o descodificador aparece na varredura, ou o IR reproduz o código |
| `relatorio` | a tabela das cinco respostas | preenchida à mão, sem deixar linha por responder |

**O veredicto do `sinal` é o que importa mais**, e o script di-lo pelos números em vez de adivinhar:

- **imagem presente** (YAVG alto) → a entrada serve.
- **preto COM áudio** → é a assinatura do **HDCP**. O estado no modelo é `protected` e **não se
  resolve em software** (RFC-0002 §12.1).
- **preto e sem áudio** → cabo, entrada errada no descodificador, ou aparelho desligado.

## 3. Os três passos que o script não pode dar

1. **Olhar a amostra.** Um YAVG baixo pode ser uma cena escura em vez de HDCP. Abrir o `.mp4` fecha
   a dúvida em cinco segundos.
2. **Trocar os capturadores de porta** e correr o `inventario` outra vez. É a prova da decisão
   central do ADR-0031: a chave acompanhou a **porta**, não o aparelho. É também onde se vê o
   problema a sério — a webcam desta máquina, medida hoje, reporta série `SN0001`, que é uma
   constante do fabricante e não identifica unidade nenhuma.
3. **Filmar o zapping.** A prova de que a sintonia funciona é o canal mudar **no aparelho**; o
   telemóvel basta. O script envia o comando e regista o que enviou — não sabe o que o
   descodificador fez, e a RFC é explícita em não confundir «enviado» com «mudado».

## 4. Controlo negativo corrido hoje (2026-10-14, sem capturador)

Corri `ambiente inventario modos sinal` nesta máquina, com a webcam interna a fazer de entrada. Não
prova nada sobre descodificadores — prova que o guião mede, e já apanhou três coisas:

```
== Ambiente        em falta: v4l2-ctl cec-client · carga: 26.75 23.85 20.78
== Inventário      /dev/video0 → pci-0000:6b:00.0-usb-0:1:1.0  (áudio: nenhum)
== Sinal           /dev/video0: YAVG=127.5 rms=n/a fps=5.0
```

1. **O filtro de nós funciona.** `/dev/video1` (nó de metadados, `caps=:`) e os `/dev/video10/11`
   (`video_output`, o `v4l2loopback` do `dxcam`) ficaram de fora; só o nó de captura entrou.
   **Limite a saber:** o inventário lista apenas nós com `:capture:` nas propriedades do udev, logo
   um dispositivo virtual mal declarado não aparece.
2. **O par de áudio vazio é visível**, e é o estado certo: esta webcam não expõe cartão ALSA no
   mesmo caminho físico. Num capturador HDMI que o exponha, a coluna enche-se.
3. **O host partilhado falseia o fps.** YAVG=127,5 diz «há imagem», mas 5,0 fps com carga 26 não é o
   tecto da webcam — é a máquina ocupada. É exactamente por isso que o `ambiente` grava a carga, e a
   medida do tecto (`carga`) se repete com a máquina em repouso.

## 5. Como se fecha a fase

1. Correr tudo com `SEGUNDOS=60`, com a máquina o mais em repouso possível.
2. Preencher a tabela das cinco respostas no fim do relatório.
3. Copiar para `docs/tv/fase0-resultado-AAAA-MM-DD.md` e abrir PR.
4. No mesmo PR, propor o estado do ADR-0031: **Aceite** se o sinal chega e a identidade é estável, ou
   **alterado** se a medida contrariar o desenho — por exemplo HDCP em todos os canais úteis, ou
   nenhum caminho de sintonia a funcionar. Uma medida que derruba o desenho é um bom resultado da
   Fase 0, não um problema dela.
