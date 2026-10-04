# B3 — ffmpeg LGPL na imagem do servidor (2026-10-04)

Fecha o achado **B3** da [auditoria](auditoria-2026-10-03.md): a imagem do servidor era distroless e não tinha ffmpeg,
por isso o directo e a gravação do servidor falhavam com `ffmpeg-ausente`. Decisão: **LGPL** na imagem do servidor
(o servidor só usa `-c copy`, `aac` nativo, `libvpx-vp9` e `libopus`); a GPL com libx264 fica para o Channel Engine
(D3, [ADR-0011](../adr/0011-channel-engine-ingest-e-distribuicao-de-tv.md)).

## O que mudou

- `Dockerfile.server`: novo estágio `ffmpeg` (Debian bookworm) que compila o **ffmpeg 9.0.2** com `--disable-gpl --disable-nonfree --disable-autodetect --enable-shared --disable-static --enable-libvpx --enable-libopus --enable-zlib`; versão e **SHA-256 fixados**; a imagem final recebe `/opt/ffmpeg` e `FFMPEG_BIN`/`FFPROBE_BIN`.
- Não se usa o `apt install ffmpeg` do Debian: é compilado com `--enable-gpl`.
- `server/src/recorder.rs`: passa a usar `FFMPEG_BIN` (usava `Command::new("ffmpeg")` fixo e ignorava a configuração).
- `scripts/check-ffmpeg-licenca.sh` (no `make fitness` e no CI): recusa `--enable-gpl|nonfree|version3`, libx264/x265/fdk fora de comentários, um SHA-256 em falta ou de zeros.

## O que foi verificado

| Verificação | Resultado |
|---|---|
| Versão mais recente | 9.0.2 de 2026-09-18, pela listagem `ffmpeg.org/releases/` (12 040 788 bytes) |
| **Assinatura PGP** do tarball | «Good signature» da chave `FCF9 86EA 15E6 E293 A564 4F10 B432 2F04 D676 58D8` («FFmpeg release signing key»); a mesma impressão digital está em `ffmpeg.org/download.html`. A confiança na chave é «unknown» no gpg (não há rede de confiança): a verificação assenta na coincidência com o site |
| SHA-256 | `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e`, verificado pelo próprio Dockerfile (`sha256sum -c`: OK) |
| O estágio compila | sim (debian:bookworm-slim, gcc 12) |
| Em **distroless/cc-debian12:nonroot**, sem shell | `ffmpeg -L` diz «GNU Lesser General Public License»; `-version` 9.0.2 sem `enable-gpl/nonfree/version3`; zero encoders libx264/x265/fdk; correm `ffmpeg` e `ffprobe` sem `LD_LIBRARY_PATH` (rpath) |
| Encoders e muxers de que o servidor depende | `libvpx`, `libvpx-vp9`, `libopus`, `aac`, `mjpeg`; `webm`, `flv`, `ivf`, `ogg`, `matroska` |
| **Composição da gravação** (como `recorder.rs`: `scale/pad/tpad/xstack/adelay/amix`, `VP9_ARGS`, Opus) a partir de 2 IVF VP8 + 2 OGG Opus | ok; 1280×360 VP9 + Opus |
| `ffprobe` do ficheiro composto (validação pré-`ready`, B5) | ok |
| Miniatura (`mjpeg`) | ok |
| **Directo** (`-c:v copy -c:a aac -f flv`) com uma fonte H.264 | ok: sai `h264` 1280×720 + `aac` |
| Imagem de apoio | `/opt/ffmpeg` com 28 MB; avisos de licença de libvpx, libopus e zlib em `/opt/ffmpeg/licenses/` |

A fonte H.264 do último teste foi criada com o ffmpeg **do host** (com libx264), apenas como entrada de teste; o ffmpeg LGPL só a passou por cópia.

## O que NÃO foi verificado

- **O `Dockerfile.server` completo nunca foi construído**: só o estágio `ffmpeg` e uma imagem distroless de teste com `/opt/ffmpeg` copiado. O estágio do Rust (mais de 1 GB, não autorizado) e o `COPY --from=ffmpeg` / `ENV` da imagem final não foram exercitados.
- **Nenhuma gravação nem directo reais** correram com esta imagem a ser servidor: os testes acima são do ffmpeg isolado, com os mesmos argumentos que o código usa.
- Sem Kubernetes, sem medição de CPU/RAM sob carga, sem o tempo exacto de construção (minutos; não registado).
- **Obrigações de redistribuição (a validar com o jurídico, ver ADR-0011):** `ffmpeg.org/legal.html` pede, para redistribuir binários LGPL, o código-fonte **correspondente alojado no mesmo servidor** que o binário. A imagem aponta para `ffmpeg.org`; se a imagem for entregue a terceiros, será preciso alojar também o tarball (e o código do libvpx e do libopus). **Não está feito.**
- A construção da imagem depende de `ffmpeg.org`, do Docker Hub e dos espelhos Debian: durante este trabalho o DNS do `ffmpeg.org` e do Docker Hub falhou de forma transitória e houve de repetir.
