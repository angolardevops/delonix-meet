# syntax=docker/dockerfile:1
# ============================================================
#  Delonix Meet — worker de transcrição em GPU (faster-whisper)
#  Base CUDA+cuDNN (CTranslate2 usa cuDNN para GPU). Corre ROOTLESS.
#  Build a partir da RAIZ do repo:  docker build -f ai-worker/Dockerfile .
#  (a raiz é o contexto porque os stubs gRPC saem de server/proto)
# ============================================================

# ---- stubs gRPC: gerados do .proto no build, nunca versionados ----
# O código gerado não depende da versão do Python, só da do grpcio-tools, que
# acompanha o grpcio de runtime (requirements.txt).
FROM python:3.10-slim AS protos
WORKDIR /src
COPY ai-worker/requirements-build.txt ai-worker/gen_protos.sh ai-worker/
RUN pip install --no-cache-dir -r ai-worker/requirements-build.txt
COPY server/proto server/proto
RUN bash ai-worker/gen_protos.sh server/proto /out

FROM nvidia/cuda:12.2.2-cudnn8-runtime-ubuntu22.04

RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 python3-pip ffmpeg ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY ai-worker/requirements.txt .
RUN pip3 install --no-cache-dir -r requirements.txt
COPY --from=protos /out /app/gen
COPY ai-worker/transcribe_worker.py ai-worker/worker.py ai-worker/job_source.py \
     ai-worker/transcriber.py ai-worker/minutes.py ./

# O modelo (~3GB para large-v3) é descarregado no 1º arranque para HF_HOME.
# Montar um volume em /models para persistir e não voltar a descarregar.
ENV HF_HOME=/models \
    WHISPER_MODEL=large-v3 \
    WHISPER_DEVICE=cuda \
    WHISPER_COMPUTE=float16 \
    RECORDINGS_DIR=/recordings \
    LEASE_SECONDS=1800

# Rootless.
RUN useradd -u 65532 -r -m -d /home/worker -s /usr/sbin/nologin worker \
    && mkdir -p /models && chown 65532 /models
USER 65532:65532
ENTRYPOINT ["python3", "/app/transcribe_worker.py"]
