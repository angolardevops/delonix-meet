#!/usr/bin/env python3
"""
Delonix Meet — worker de transcrição em GPU.

Consome as gravações produzidas pelo servidor (recorder.rs → RECORDINGS_DIR/
<id>.webm), transcreve-as com faster-whisper (GPU quando disponível) e preenche
a transcrição + a ATA (MoM) na base de dados. É idempotente: só processa
gravações com `transcribed_at IS NULL`.

Guarda os SEGMENTOS (início, fim, texto, confiança) e a língua detectada
(migração 0041) — são eles que alimentam a legenda no leitor, os capítulos
automáticos e o `GET /api/recordings/{id}/transcript`. Enquanto trabalha, a
gravação fica em `status = 'transcribing'` com `progress_pct`; vários workers
não pegam na mesma (`FOR UPDATE SKIP LOCKED`), e uma gravação cujo worker
morreu (sem sinal de vida há `STALE_MINUTES`) volta a ser apanhada.

Env:
  DATABASE_URL     ligação Postgres (obrigatório)
  RECORDINGS_DIR   pasta das gravações (default: /recordings)
  WHISPER_MODEL    modelo faster-whisper (default: large-v3)
  WHISPER_DEVICE   cuda|cpu (default: cuda)
  WHISPER_COMPUTE  float16|int8_float16|int8 (default: float16)
  POLL_SECONDS     intervalo de sondagem quando não há trabalho (default: 20)
  STALE_MINUTES    sem progresso há mais do que isto = worker morto (default: 30)
"""
import json
import math
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
STALE_MINUTES = int(os.environ.get("STALE_MINUTES", "30"))

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


def segment_row(start: float, end: float, text: str, avg_logprob) -> dict | None:
    """Um segmento do faster-whisper na forma guardada em `transcript_segments`.

    A confiança é exp(avg_logprob), presa a [0, 1]. Texto vazio não é segmento."""
    text = (text or "").strip()
    if not text:
        return None
    conf = None
    if avg_logprob is not None and not math.isnan(avg_logprob):
        conf = round(max(0.0, min(1.0, math.exp(avg_logprob))), 3)
    start_ms = max(0, int(round(start * 1000)))
    end_ms = max(start_ms, int(round(end * 1000)))
    return {"start_ms": start_ms, "end_ms": end_ms, "text": text, "confidence": conf}


def transcribe(model: WhisperModel, path: str, on_progress=None):
    """Devolve (texto, segmentos, língua, confiança média)."""
    # vad_filter corta silêncios; language=None deixa o modelo detetar (PT/EN/…).
    segments, info = model.transcribe(path, vad_filter=True, beam_size=5)
    total = float(getattr(info, "duration", 0) or 0)
    rows = []
    for seg in segments:
        row = segment_row(seg.start, seg.end, seg.text, getattr(seg, "avg_logprob", None))
        if row:
            rows.append(row)
        if on_progress and total > 0:
            on_progress(min(99, int(seg.end * 100 / total)))
    text = " ".join(r["text"] for r in rows).strip()
    confs = [r["confidence"] for r in rows if r["confidence"] is not None]
    avg = round(sum(confs) / len(confs), 3) if confs else None
    return text, rows, getattr(info, "language", None), avg


def _claim(conn):
    """Reserva uma gravação para transcrever. Devolve (id, room_code) ou None."""
    with conn.cursor() as cur:
        cur.execute(
            """
            UPDATE recordings SET status = 'transcribing', progress_pct = 0, progress_at = now()
            WHERE id = (
                SELECT id FROM recordings
                WHERE transcribed_at IS NULL
                  AND (status = 'ready'
                       OR (status = 'transcribing'
                           AND progress_at < now() - make_interval(mins => %s)))
                ORDER BY created_at ASC
                LIMIT 1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING id, (SELECT code FROM rooms WHERE rooms.id = recordings.room_id)
            """,
            (STALE_MINUTES,),
        )
        row = cur.fetchone()
    conn.commit()
    return row


def process_one(conn, model: WhisperModel) -> bool:
    """Processa uma gravação pendente. Devolve True se havia trabalho."""
    row = _claim(conn)
    if not row:
        return False

    rec_id, room_code = row
    path = os.path.join(RECORDINGS_DIR, f"{rec_id}.webm")
    if not os.path.exists(path):
        log(f"ficheiro em falta {path} — a marcar como processado para não repetir")
        _mark_failed(conn, rec_id, "O ficheiro da gravação não foi encontrado pelo serviço de transcrição.")
        return True

    last = {"pct": -1, "at": 0.0}

    def on_progress(pct: int):
        now = time.time()
        if pct >= last["pct"] + 5 or now - last["at"] > 30:
            last.update(pct=pct, at=now)
            with conn.cursor() as cur:
                cur.execute(
                    "UPDATE recordings SET progress_pct = %s, progress_at = now() "
                    "WHERE id = %s AND status = 'transcribing'",
                    (pct, rec_id),
                )
            conn.commit()

    log(f"a transcrever gravação {rec_id} ({path})…")
    t0 = time.time()
    try:
        transcript, rows, language, confidence = transcribe(model, path, on_progress)
    except Exception as e:  # ficheiro ilegível, modelo sem memória, …
        conn.rollback()
        log(f"gravação {rec_id}: transcrição falhou: {e}")
        _mark_failed(conn, rec_id, "A transcrição falhou. A equipa de operação tem o detalhe no registo.")
        return True
    mom = build_mom(transcript)
    log(f"gravação {rec_id} transcrita em {time.time() - t0:.1f}s "
        f"({len(transcript)} chars, {len(rows)} segmentos, língua {language})")

    _mark_done(conn, rec_id, transcript, mom, rows, language, confidence)

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


def _mark_done(conn, rec_id, transcript: str, mom: str, rows, language, confidence):
    with conn.cursor() as cur:
        cur.execute(
            """
            UPDATE recordings SET transcript = %s, minutes = %s, transcribed_at = now(),
                   transcript_segments = %s::jsonb, transcript_language = %s,
                   transcript_confidence = %s, transcript_error = NULL,
                   status = CASE WHEN status = 'transcribing' THEN 'ready' ELSE status END,
                   progress_pct = NULL, progress_at = NULL
            WHERE id = %s
            """,
            (transcript, mom, json.dumps(rows, ensure_ascii=False), language, confidence, rec_id),
        )
    conn.commit()


def _mark_failed(conn, rec_id, reason: str):
    """Marca como processada COM erro: `transcribed_at` fica preenchido para
    não repetir em ciclo, e `transcript_error` diz porquê."""
    with conn.cursor() as cur:
        cur.execute(
            """
            UPDATE recordings SET transcribed_at = now(), transcript_error = %s,
                   status = CASE WHEN status = 'transcribing' THEN 'ready' ELSE status END,
                   progress_pct = NULL, progress_at = NULL
            WHERE id = %s
            """,
            (reason, rec_id),
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
