# Delonix TTS — dobragem com voz sintética (Piper).
#
# ESTA IMAGEM É GPL-3.0-or-later, e existe sozinha por isso. O servidor do Meet
# é LGPL (decisão D3, `scripts/check-ffmpeg-licenca.sh`) e o que é copyleft vive
# confinado à sua própria imagem, com a sua própria oferta de código — foi
# assim com o ffmpeg do Channel Engine e é assim aqui. Ver `LICENCAS.md`.
FROM python:3.11-slim

# `curl` para trazer a fonte e a voz; sai da imagem final? Não: fica, porque a
# imagem não é distroless e arrancar um segundo estágio só para o tirar
# custaria mais do que vale. O que importa é não correr como root (abaixo).
RUN apt-get update && apt-get install -y --no-install-recommends \
      curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# ---------------------------------------------------------------------------
# A OFERTA DE CÓDIGO da GPL-3.0, cumprida por ACOMPANHAMENTO (§6): a fonte vem
# no mesmo artefacto que o binário. Não é uma promessa de a enviar durante três
# anos — é o tarball, aqui dentro, verificado pelo SHA-256 que o LICENCAS.md
# regista. Se o PyPI mudar o ficheiro debaixo de nós, o build PARA.
# ---------------------------------------------------------------------------
ARG PIPER_VERSION=1.8.0
ARG PIPER_SDIST_SHA256=830588aded347df579c91a32703e0fc2a3685d84f1e3533b14f2de69135d4904
RUN set -eu; \
    mkdir -p /usr/share/piper-src; \
    curl -fsSL -o /usr/share/piper-src/piper_tts-${PIPER_VERSION}.tar.gz \
      "https://files.pythonhosted.org/packages/source/p/piper-tts/piper_tts-${PIPER_VERSION}.tar.gz"; \
    echo "${PIPER_SDIST_SHA256}  /usr/share/piper-src/piper_tts-${PIPER_VERSION}.tar.gz" | sha256sum -c -
COPY LICENCAS.md /usr/share/piper-src/

# ---------------------------------------------------------------------------
# A VOZ, embutida. O cluster é OFFLINE (sem DNS nos pods — a mesma razão pela
# qual o whisper-server embute o modelo): descarregar no primeiro arranque
# falharia com «name resolution».
#
# Fixada por REVISÃO e por SHA-256. A revisão prende os dois ficheiros ao mesmo
# ponto do catálogo; o SHA-256 prende o modelo em si. Uma voz é um artefacto de
# terceiros, não configuração.
# ---------------------------------------------------------------------------
ARG VOICES_REV=c10ece1aade47bb51c153c893d14e5bf8e5b7117
ARG VOICE_SHA256=223a7aaca69a155c61897e8ada7c3b13bc306e16c72dbb9c2fed733e2b0927d4
ENV PIPER_VOICES=/vozes
RUN set -eu; \
    mkdir -p /vozes; \
    base="https://huggingface.co/rhasspy/piper-voices/resolve/${VOICES_REV}/pt/pt_PT/tug%C3%A3o/medium"; \
    curl -fsSL -o "/vozes/pt_PT-tugão-medium.onnx"      "${base}/pt_PT-tug%C3%A3o-medium.onnx"; \
    curl -fsSL -o "/vozes/pt_PT-tugão-medium.onnx.json" "${base}/pt_PT-tug%C3%A3o-medium.onnx.json"; \
    curl -fsSL -o "/vozes/pt_PT-tugão-medium.MODEL_CARD" "${base}/MODEL_CARD"; \
    echo "${VOICE_SHA256}  /vozes/pt_PT-tugão-medium.onnx" | sha256sum -c -

COPY app.py .

# Não-root, como as outras imagens deste repo. Uid NUMÉRICO e sem entrada no
# /etc/passwd — é o que o `runAsNonRoot` do Kubernetes exige. 65532, o mesmo do
# ai-worker e do whisper-server.
RUN chown -R 65532:65532 /vozes /app
USER 65532:65532

EXPOSE 8000
CMD ["uvicorn", "app:app", "--host", "0.0.0.0", "--port", "8000"]
