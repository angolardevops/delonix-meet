#!/usr/bin/env python3
"""
Delonix Meet — worker de transcrição em GPU.

Consome as gravações produzidas pelo servidor (recorder.rs → RECORDINGS_DIR/
<id>.webm), transcreve-as com faster-whisper (GPU quando disponível) e preenche
a transcrição + a ATA (MoM) na base de dados. É idempotente: só processa
gravações com `transcribed_at IS NULL`.

Env:
  DATABASE_URL     ligação Postgres (obrigatório)
  RECORDINGS_DIR   pasta das gravações (default: /recordings)
  WHISPER_MODEL    modelo faster-whisper (default: large-v3)
  WHISPER_DEVICE   cuda|cpu (default: cuda)
  WHISPER_COMPUTE  float16|int8_float16|int8 (default: float16)
  POLL_SECONDS     intervalo de sondagem quando não há trabalho (default: 20)
"""
import os
import sys
import time
import signal

import psycopg2
from faster_whisper import WhisperModel

DATABASE_URL = os.environ["DATABASE_URL"]
RECORDINGS_DIR = os.environ.get("RECORDINGS_DIR", "/recordings")
MODEL_NAME = os.environ.get("WHISPER_MODEL", "large-v3")
DEVICE = os.environ.get("WHISPER_DEVICE", "cuda")
COMPUTE = os.environ.get("WHISPER_COMPUTE", "float16")
POLL_SECONDS = int(os.environ.get("POLL_SECONDS", "20"))

_running = True


def _stop(*_):
    global _running
    _running = False


signal.signal(signal.SIGTERM, _stop)
signal.signal(signal.SIGINT, _stop)


def log(msg: str):
    print(f"[ai-worker] {msg}", flush=True)


def build_mom(transcript: str) -> str:
    """Ata (MoM) simples e extractiva a partir da transcrição — sem LLM.
    Resumo por tópicos: primeiras frases + linhas com marcadores de ação."""
    text = " ".join(transcript.split())
    if not text:
        return ""
    # Divide em frases de forma tosca mas robusta.
    import re
    sentences = [s.strip() for s in re.split(r"(?<=[.!?])\s+", text) if s.strip()]
    action_kw = ("decid", "ficou", "vamos", "próximo", "proximo", "ação", "acao",
                 "tarefa", "responsáv", "responsav", "prazo", "até", "ate ", "todo")
    actions = [s for s in sentences if any(k in s.lower() for k in action_kw)]
    lines = ["# Ata (gerada automaticamente)", "", "## Resumo"]
    lines += [f"- {s}" for s in sentences[:5]]
    if actions:
        lines += ["", "## Ações / decisões"]
        lines += [f"- {s}" for s in actions[:8]]
    return "\n".join(lines)


def transcribe(model: WhisperModel, path: str) -> str:
    # vad_filter corta silêncios; language=None deixa o modelo detetar (PT/EN/…).
    segments, _info = model.transcribe(path, vad_filter=True, beam_size=5)
    return " ".join(seg.text.strip() for seg in segments).strip()


def process_one(conn, model: WhisperModel) -> bool:
    """Processa uma gravação pendente. Devolve True se havia trabalho."""
    with conn.cursor() as cur:
        cur.execute(
            """
            SELECT r.id, rm.code
            FROM recordings r
            LEFT JOIN rooms rm ON rm.id = r.room_id
            WHERE r.transcribed_at IS NULL
            ORDER BY r.id ASC
            LIMIT 1
            """
        )
        row = cur.fetchone()
    if not row:
        return False

    rec_id, room_code = row
    path = os.path.join(RECORDINGS_DIR, f"{rec_id}.webm")
    if not os.path.exists(path):
        log(f"ficheiro em falta {path} — a marcar como processado para não repetir")
        _mark_done(conn, rec_id, "", "")
        return True

    log(f"a transcrever gravação {rec_id} ({path})…")
    t0 = time.time()
    transcript = transcribe(model, path)
    mom = build_mom(transcript)
    log(f"gravação {rec_id} transcrita em {time.time() - t0:.1f}s ({len(transcript)} chars)")

    _mark_done(conn, rec_id, transcript, mom)

    # Preenche também a ATA da reunião ligada (é o que o leitor mostra), se
    # existir uma reunião com este room_code e ainda sem transcrição.
    if room_code:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE meetings SET transcript = %s, minutes = %s "
                "WHERE room_code = %s AND transcript = ''",
                (transcript, mom, room_code),
            )
        conn.commit()
    return True


def _mark_done(conn, rec_id, transcript: str, mom: str):
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE recordings SET transcript = %s, minutes = %s, transcribed_at = now() WHERE id = %s",
            (transcript, mom, rec_id),
        )
    conn.commit()


def main():
    log(f"a carregar modelo {MODEL_NAME} em {DEVICE}/{COMPUTE}…")
    try:
        model = WhisperModel(MODEL_NAME, device=DEVICE, compute_type=COMPUTE)
    except Exception as e:  # GPU indisponível → cai para CPU (mais lento)
        log(f"falha a carregar em {DEVICE} ({e}); a tentar CPU/int8")
        model = WhisperModel(MODEL_NAME, device="cpu", compute_type="int8")
    log("modelo pronto — a sondar gravações")

    conn = psycopg2.connect(DATABASE_URL)
    conn.autocommit = False
    while _running:
        try:
            worked = process_one(conn, model)
        except Exception as e:  # não deixar o worker morrer por uma gravação má
            log(f"erro a processar: {e}")
            conn.rollback()
            worked = False
        if not worked:
            for _ in range(POLL_SECONDS):
                if not _running:
                    break
                time.sleep(1)
    conn.close()
    log("terminado")


if __name__ == "__main__":
    sys.exit(main())
