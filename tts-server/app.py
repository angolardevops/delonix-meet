"""Delonix TTS — dobragem com voz sintética, soberana (Piper).

Um pedido, uma frase, uma onda PCM. Não guarda nada, não sai da infraestrutura.

Protocolo (HTTP):
  · `POST /synthesize` {"text": "...", "lang": "pt-PT"} → `audio/wav`
    (PCM 16-bit mono a **48 kHz**);
  · `GET /health` → a voz carregada, a versão do motor e a licença.

**Esta imagem é GPL-3.0-or-later** e vive sozinha por isso — ver `LICENCAS.md`,
que também fixa a voz por revisão e por SHA-256.
"""
import io
import os
import wave

import numpy as np
import soxr

import jwt
from fastapi import FastAPI, Header, HTTPException
from pydantic import BaseModel, Field

# ---------------------------------------------------------------------------
# A credencial. Fail-closed, como no whisper-server: sem JWT_SECRET nada é
# aceite. Um serviço de inferência alcançável sem token é GPU grátis para quem
# chegar ao caminho — e, aqui, uma forma de pôr a nossa voz a dizer o que
# quiserem.
# ---------------------------------------------------------------------------
JWT_SECRET = os.environ.get("JWT_SECRET", "")

# O catálogo, que é o MESMO do domínio (`conferencing/dubbing.rs`). Está nos
# dois sítios de propósito: o Rust decide se uma língua PODE ser pedida, este
# serviço decide se a SABE dizer. Divergirem é um defeito, e o
# `check-tts-licenca.sh` compara-os.
VOZES = {
    "pt-PT": "pt_PT-tugão-medium",
}

# Uma frase, não um livro. O caminho é ASR→LLM→TTS frase a frase (ADR-0030);
# um pedido gigante só pode vir de um erro ou de um abuso, e em qualquer dos
# casos segura a fila de toda a gente.
MAX_CHARS = 1_000

# A taxa de SAÍDA é 48 kHz, e não a nativa da voz (22 050 Hz), porque o Opus só
# aceita 8/12/16/24/48 kHz — 22 050 não é uma delas. A reamostragem fica AQUI, e
# não no Rust, por duas razões: é onde o áudio já está em memória, e o `soxr`
# faz um trabalho melhor do que uma interpolação linear escrita à mão (que em
# fala se ouve como um sibilo nos «s»).
TAXA_SAIDA = 48_000

app = FastAPI(title="Delonix TTS", docs_url=None, redoc_url=None)
_vozes_carregadas = {}


def _voz(nome: str):
    """Carrega a voz uma vez e reutiliza-a. 63 MB não se lêem por pedido."""
    if nome not in _vozes_carregadas:
        from piper import PiperVoice

        modelo = os.path.join(os.environ.get("PIPER_VOICES", "/vozes"), f"{nome}.onnx")
        _vozes_carregadas[nome] = PiperVoice.load(modelo)
    return _vozes_carregadas[nome]


def _autorizado(authorization: str | None) -> bool:
    if not JWT_SECRET or not authorization:
        return False
    if not authorization.startswith("Bearer "):
        return False
    try:
        jwt.decode(authorization[7:], JWT_SECRET, algorithms=["HS256"])
    except jwt.PyJWTError:
        return False
    return True


class Pedido(BaseModel):
    text: str = Field(min_length=1)
    lang: str = "pt-PT"


@app.get("/health")
def health():
    import piper

    return {
        "status": "ok",
        "engine": "piper",
        "engine_version": getattr(piper, "__version__", "desconhecida"),
        "license": "GPL-3.0-or-later",
        "voices": VOZES,
    }


@app.post("/synthesize")
def synthesize(p: Pedido, authorization: str | None = Header(default=None)):
    if not _autorizado(authorization):
        raise HTTPException(status_code=401, detail="tts.unauthorized")
    if p.lang not in VOZES:
        # A mesma recusa do domínio, com o mesmo código: não há voz para essa
        # língua, e dizê-lo é melhor do que escolher outra parecida.
        raise HTTPException(status_code=400, detail="dubbing.no_voice")
    texto = p.text.strip()
    if not texto:
        raise HTTPException(status_code=400, detail="tts.empty")
    if len(texto) > MAX_CHARS:
        raise HTTPException(status_code=413, detail="tts.too_long")

    voz = _voz(VOZES[p.lang])
    bruto = io.BytesIO()
    with wave.open(bruto, "wb") as w:
        voz.synthesize_wav(texto, w)

    bruto.seek(0)
    with wave.open(bruto) as w:
        taxa = w.getframerate()
        amostras = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16)

    if taxa != TAXA_SAIDA:
        amostras = soxr.resample(amostras, taxa, TAXA_SAIDA, quality="HQ").astype(np.int16)

    saida = io.BytesIO()
    with wave.open(saida, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(TAXA_SAIDA)
        w.writeframes(amostras.tobytes())

    from fastapi.responses import Response

    return Response(content=saida.getvalue(), media_type="audio/wav")
