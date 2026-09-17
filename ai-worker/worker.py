"""O ciclo do worker: reservar → transcrever → acta → entregar (ou desistir).

Não sabe de onde vêm os trabalhos (`JobSource`) nem como se transcreve
(`Transcriber`) — os dois são injectados, e é isso que o deixa testar sem GPU,
sem servidor e sem base.
"""
import os
import threading
import time
from enum import Enum
from typing import Callable

from job_source import Job, JobSource, LeaseLost
from minutes import build_mom
from transcriber import Transcriber

Log = Callable[[str], None]


class Outcome(Enum):
    IDLE = "idle"              # não havia trabalho
    COMPLETED = "completed"    # entregue e aceite
    FAILED = "failed"          # entregue como falha (FailJob)
    LEASE_LOST = "lease_lost"  # o servidor recusou: a reserva já não era nossa
    ERROR = "error"            # a fonte falhou (rede, servidor em baixo…)


def _media_path(recordings_dir: str, media_file: str):
    """Caminho do ficheiro, recusando o que sairia do volume de gravações."""
    if not media_file or os.path.isabs(media_file) or ".." in media_file.split("/"):
        return None
    return os.path.join(recordings_dir, media_file)


def _give_up(source: JobSource, job: Job, reason: str, retryable: bool, log: Log) -> Outcome:
    try:
        source.fail(job, reason, retryable)
    except LeaseLost as e:
        log(f"gravação {job.recording_id}: reserva perdida ao desistir ({e})")
        return Outcome.LEASE_LOST
    except Exception as e:
        # A reserva expira sozinha e o trabalho volta à fila: não é fatal.
        log(f"gravação {job.recording_id}: FailJob falhou ({e}); a reserva expira sozinha")
        return Outcome.ERROR
    return Outcome.FAILED


def process_one(source: JobSource, transcriber: Transcriber, recordings_dir: str,
                log: Log, clock: Callable[[], float] = time.time) -> Outcome:
    """Trata no máximo um trabalho. Nunca deixa escapar uma excepção da
    transcrição: uma gravação má não pode matar o worker."""
    try:
        job = source.claim()
    except Exception as e:
        log(f"falha a reservar trabalho: {e}")
        return Outcome.ERROR
    if job is None:
        return Outcome.IDLE

    path = _media_path(recordings_dir, job.media_file)
    if path is None:
        return _give_up(source, job, f"media_file inválido: {job.media_file!r}", False, log)
    if not os.path.exists(path):
        log(f"gravação {job.recording_id}: ficheiro em falta {path} — falha definitiva")
        return _give_up(source, job, f"ficheiro em falta: {job.media_file}", False, log)

    log(f"a transcrever gravação {job.recording_id} (tentativa {job.attempt}, {path})…")
    t0 = clock()
    try:
        transcript = transcriber.transcribe(path)
        minutes = build_mom(transcript)
    except Exception as e:
        log(f"gravação {job.recording_id}: transcrição falhou ({type(e).__name__}: {e})")
        return _give_up(source, job, f"{type(e).__name__}: {e}", True, log)
    now = clock()
    log(f"gravação {job.recording_id} transcrita em {now - t0:.1f}s ({len(transcript)} chars)")

    # Não há renovação da reserva no contrato: se a transcrição demorou mais do
    # que ela, o servidor vai recusar a entrega (e outro worker pode já a ter).
    if job.lease_expires_unix and now > job.lease_expires_unix:
        log(f"gravação {job.recording_id}: a reserva expirou há "
            f"{now - job.lease_expires_unix:.0f}s — subir LEASE_SECONDS (máx. 7200)")

    try:
        source.complete(job, transcript, minutes)
    except LeaseLost as e:
        log(f"gravação {job.recording_id}: entrega recusada, reserva perdida ({e}) — a seguir")
        return Outcome.LEASE_LOST
    except Exception as e:
        log(f"gravação {job.recording_id}: CompleteJob falhou ({e}); a reserva expira sozinha")
        return Outcome.ERROR
    log(f"gravação {job.recording_id} entregue")
    return Outcome.COMPLETED


def run(source: JobSource, transcriber: Transcriber, recordings_dir: str,
        poll_seconds: float, stop: threading.Event, log: Log) -> None:
    """Ciclo até `stop`. Sem trabalho (ou com a fonte em baixo) espera
    `poll_seconds`, acordando logo que chegue um SIGTERM."""
    while not stop.is_set():
        outcome = process_one(source, transcriber, recordings_dir, log)
        if outcome in (Outcome.IDLE, Outcome.ERROR):
            stop.wait(poll_seconds)
