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

# O modelo é descarregado no 1º arranque para /models (PVC) e reutilizado.
ENV HF_HOME=/models \
    WHISPER_MODEL=small \
    WHISPER_DEVICE=cpu \
    WHISPER_COMPUTE=int8

EXPOSE 8000
CMD ["uvicorn", "app:app", "--host", "0.0.0.0", "--port", "8000", "--ws-max-size", "4000000"]
