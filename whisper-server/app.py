"""Delonix Whisper — ASR streaming soberano (faster-whisper).

Substitui o whisper-tiny WASM do browser por um Whisper maior no TEU servidor:
precisão muito superior sem depender da Google (Web Speech) nem da cloud. O
áudio nunca sai da tua infraestrutura.

Protocolo (WebSocket /asr?lang=pt):
  · cliente envia frames BINÁRIOS = PCM Int16 mono a 16 kHz (resample no browser);
  · servidor faz VAD por energia, transcreve a elocução em curso e responde
    JSON {"type":"interim"|"final","text":...}. `final` fecha a frase (silêncio).

Modelo/algoritmo controlados por env: WHISPER_MODEL (small|base|medium|large-v3),
WHISPER_DEVICE (cpu|cuda), WHISPER_COMPUTE (int8|float16). Em CPU usar `small`
int8; em GPU `large-v3` float16.
"""
import asyncio
import json
import logging
import os
import time

import jwt
import numpy as np
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from faster_whisper import WhisperModel

MODEL_NAME = os.environ.get("WHISPER_MODEL", "small")
DEVICE = os.environ.get("WHISPER_DEVICE", "cpu")
COMPUTE = os.environ.get("WHISPER_COMPUTE", "int8")
SR = 16000  # taxa de amostragem esperada do cliente

# O MESMO segredo do servidor Rust (server/src/auth.rs, HS256). O /asr fica
# no mesmo ingress público que o resto da app (deploy/k8s/04-ingress.yaml) —
# sem afinidade nem gateway de auth à frente — e até aqui aceitava QUALQUER
# ligação WS sem token nenhum: GPU de transcrição grátis para quem
# alcançasse o caminho. Sem JWT_SECRET, nada é aceite (fail-closed) — nunca
# degradar para "sem autenticação" por falta de configuração.
JWT_SECRET = os.environ.get("JWT_SECRET", "")
logger = logging.getLogger("delonix-whisper")


def _claims_or_none(token: str):
    """Valida o access token (mesmo formato do auth.rs: HS256, claim `typ`).
    None em qualquer falha — assinatura errada, expirado, tipo errado, ou
    JWT_SECRET por definir."""
    if not JWT_SECRET or not token:
        return None
    try:
        claims = jwt.decode(token, JWT_SECRET, algorithms=["HS256"])
    except jwt.PyJWTError:
        return None
    if claims.get("typ") != "access":
        return None
    return claims


# Um único modelo partilhado por todas as ligações (thread-safe em inferência).
model = WhisperModel(MODEL_NAME, device=DEVICE, compute_type=COMPUTE)

app = FastAPI()


@app.get("/health")
def health():
    return {"ok": True, "model": MODEL_NAME, "device": DEVICE}


def _transcribe(pcm: np.ndarray, lang: str) -> str:
    """Transcreve um bloco float32 [-1,1]. Roda em thread (inferência bloqueia)."""
    segments, _ = model.transcribe(
        pcm,
        language=lang or None,
        vad_filter=False,          # já fazemos a segmentação por elocução
        beam_size=1,               # rápido; sobe para 5 em GPU se quiseres qualidade
        condition_on_previous_text=False,
        no_speech_threshold=0.5,
    )
    return " ".join(s.text.strip() for s in segments).strip()


@app.websocket("/asr")
async def asr(ws: WebSocket):
    # Verificar ANTES de aceitar: uma ligação nunca aceite não gasta um
    # slot de transcrição nem entra no `while True` de baixo.
    if _claims_or_none(ws.query_params.get("token", "")) is None:
        await ws.close(code=1008)  # Policy Violation
        return
    await ws.accept()
    lang = ws.query_params.get("lang", "pt")[:5]
    # Aceita 'pt-PT' → 'pt' (faster-whisper usa códigos ISO curtos).
    lang = lang.split("-")[0]

    buf = np.zeros(0, dtype=np.float32)   # elocução em curso (float32)
    last_voice = time.monotonic()          # último instante com fala
    last_interim = 0.0                      # última vez que emitimos interim
    SILENCE_S = 0.7                         # gap que fecha a frase
    INTERIM_EVERY_S = 1.2                   # cadência dos parciais
    MAX_UTTER_S = 20.0                      # corta elocuções muito longas
    ENERGY_GATE = 0.0008                    # limiar de energia (RMS) p/ "há fala"

    async def emit(kind: str, text: str):
        if text:
            await ws.send_text(json.dumps({"type": kind, "text": text}))

    try:
        while True:
            msg = await ws.receive()
            if msg.get("type") == "websocket.disconnect":
                break
            data = msg.get("bytes")
            if data is None:
                continue
            # Int16 PCM → float32 [-1,1]
            chunk = np.frombuffer(data, dtype=np.int16).astype(np.float32) / 32768.0
            if chunk.size == 0:
                continue
            rms = float(np.sqrt(np.mean(chunk * chunk)))
            now = time.monotonic()
            if rms >= ENERGY_GATE:
                last_voice = now
                buf = np.concatenate([buf, chunk])
            elif buf.size:
                # silêncio: acumula um pouco de "cauda" para não cortar palavras
                buf = np.concatenate([buf, chunk])

            secs = buf.size / SR
            # Fim de frase: silêncio prolongado com áudio acumulado.
            if buf.size and (now - last_voice) >= SILENCE_S and secs >= 0.4:
                text = await asyncio.to_thread(_transcribe, buf, lang)
                await emit("final", text)
                buf = np.zeros(0, dtype=np.float32)
                last_interim = 0.0
                continue
            # Corte de segurança para elocuções muito longas.
            if secs >= MAX_UTTER_S:
                text = await asyncio.to_thread(_transcribe, buf, lang)
                await emit("final", text)
                buf = np.zeros(0, dtype=np.float32)
                last_interim = 0.0
                continue
            # Parcial (interim) a cada INTERIM_EVERY_S de elocução.
            if secs >= 0.8 and (now - last_interim) >= INTERIM_EVERY_S:
                last_interim = now
                text = await asyncio.to_thread(_transcribe, buf, lang)
                await emit("interim", text)
    except WebSocketDisconnect:
        pass
    except Exception:  # noqa: BLE001 — não derrubar o worker por uma ligação
        # O detalhe fica no LOG do servidor, nunca no cliente: a mensagem
        # crua de uma exceção Python (caminhos, nomes internos) ajudava
        # reconhecimento a quem a provocasse de propósito.
        logger.exception("erro na ligação /asr")
        try:
            await ws.send_text(json.dumps({"type": "error", "message": "erro interno"}))
        except Exception:
            pass
