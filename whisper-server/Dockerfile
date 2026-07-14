# Delonix Whisper — ASR streaming soberano (faster-whisper).
# CPU por defeito (CTranslate2 int8). Para GPU: base numa imagem CUDA e
# WHISPER_DEVICE=cuda / WHISPER_COMPUTE=float16 (ver 09-whisper.yaml).
FROM python:3.11-slim

# ffmpeg/libgomp são precisos ao CTranslate2/faster-whisper.
RUN apt-get update && apt-get install -y --no-install-recommends \
      libgomp1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY app.py .

ENV HF_HOME=/models \
    WHISPER_MODEL=small \
    WHISPER_DEVICE=cpu \
    WHISPER_COMPUTE=int8

# Embute o modelo na imagem (o cluster kind é OFFLINE — sem DNS/internet nos
# pods, o download no 1º arranque falha com "name resolution"). O build corre
# no host (com internet) e o modelo fica cacheado em /models dentro da imagem.
# Trocar WHISPER_MODEL exige rebuild (--build-arg MODEL=...).
ARG MODEL=small
ENV WHISPER_MODEL=${MODEL}
RUN python -c "from faster_whisper import WhisperModel; WhisperModel('${MODEL}', device='cpu', compute_type='int8')"

EXPOSE 8000
CMD ["uvicorn", "app:app", "--host", "0.0.0.0", "--port", "8000", "--ws-max-size", "4000000"]
