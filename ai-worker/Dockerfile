# syntax=docker/dockerfile:1
# ============================================================
#  Delonix Meet — worker de transcrição em GPU (faster-whisper)
#  Base CUDA+cuDNN (CTranslate2 usa cuDNN para GPU). Corre ROOTLESS.
#  Build a partir da RAIZ do repo:  docker build -f ai-worker/Dockerfile .
# ============================================================
FROM nvidia/cuda:12.2.2-cudnn8-runtime-ubuntu22.04

RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 python3-pip ffmpeg ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY ai-worker/requirements.txt .
RUN pip3 install --no-cache-dir -r requirements.txt
COPY ai-worker/transcribe_worker.py .

# O modelo (~3GB para large-v3) é descarregado no 1º arranque para HF_HOME.
# Montar um volume em /models para persistir e não voltar a descarregar.
ENV HF_HOME=/models \
    WHISPER_MODEL=large-v3 \
    WHISPER_DEVICE=cuda \
    WHISPER_COMPUTE=float16 \
    RECORDINGS_DIR=/recordings

# Rootless.
RUN useradd -u 65532 -r -m -d /home/worker -s /usr/sbin/nologin worker \
    && mkdir -p /models && chown 65532 /models
USER 65532:65532
ENTRYPOINT ["python3", "/app/transcribe_worker.py"]
