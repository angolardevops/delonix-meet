#!/usr/bin/env python3
"""
Delonix Meet — worker de transcrição em GPU.

Reserva gravações no servidor pelo gRPC interno
(`delonix.meet.transcription.v1.TranscriptionService`, ADR-0006 §3), transcreve-as
com faster-whisper (GPU quando disponível), gera a ATA (MoM) e entrega. O servidor
é o dono do estado: aplica o DLP antes de gravar, controla a reserva (lease) e as
tentativas (máx. 5). O worker não toca na base.

Env — modo gRPC (o suportado):
  DELONIX_GRPC_ADDR  host:porta do gRPC interno (ex.: delonix-server-internal:9180)
  GRPC_CLIENT_CERT   certificado de cliente (PEM)   ┐
  GRPC_CLIENT_KEY    chave do certificado (PEM)     ├ mTLS: os três, ou nenhum
  GRPC_CA            CA que assina o servidor (PEM) ┘
  GRPC_INSECURE      =1 aceita texto claro sem certificados (só desenvolvimento;
                     o servidor tem de ter DELONIX_ALLOW_INSECURE=1)
  WORKER_ID          identificador para logs/auditoria (default: hostname)
  LEASE_SECONDS      duração pedida da reserva (default: 1800; o servidor limita a
                     60..7200). Não há renovação: tem de cobrir a gravação mais longa.

Env — modo legado (DEPRECADO, escreve no Postgres SEM DLP nem reservas):
  DATABASE_URL       só é usado quando DELONIX_GRPC_ADDR NÃO está definido

Env — comuns:
  RECORDINGS_DIR     pasta das gravações partilhada com o servidor (default: /recordings)
  WHISPER_MODEL      modelo faster-whisper (default: large-v3)
  WHISPER_DEVICE     cuda|cpu (default: cuda)
  WHISPER_COMPUTE    float16|int8_float16|int8 (default: float16)
  POLL_SECONDS       espera quando não há trabalho (default: 20)
  TRANSCRIBER        whisper (default) | fake — o fake só serve os testes e devolve
                     FAKE_TRANSCRIPT sem carregar modelo nenhum

Opções:
  --once   trata no máximo um trabalho e sai. Código de saída: 0 entregue (ou
           falha registada), 3 sem trabalho, 4 reserva perdida, 1 erro, 2 configuração.

Stubs gRPC: gerados de server/proto por `ai-worker/gen_protos.sh` para
`ai-worker/gen/` (no build da imagem; nunca versionados).
"""
import argparse
import os
import signal
import socket
import sys
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.environ.get("DELONIX_PROTO_GEN_DIR", os.path.join(HERE, "gen")))

from job_source import ConfigError, GrpcJobSource, LegacyDbJobSource, open_channel  # noqa: E402
from transcriber import FakeTranscriber, WhisperTranscriber  # noqa: E402
import worker  # noqa: E402

EXIT_CODES = {
    worker.Outcome.COMPLETED: 0,
    worker.Outcome.FAILED: 0,
    worker.Outcome.IDLE: 3,
    worker.Outcome.LEASE_LOST: 4,
    worker.Outcome.ERROR: 1,
}


def log(msg: str):
    print(f"[ai-worker] {msg}", flush=True)


def _int_env(env, name: str, default: int) -> int:
    raw = env.get(name, "").strip()
    if not raw:
        return default
    try:
        return int(raw)
    except ValueError as e:
        raise ConfigError(f"{name}={raw!r} não é um inteiro") from e


def build_source(env):
    grpc_addr = env.get("DELONIX_GRPC_ADDR", "").strip()
    if grpc_addr:
        channel = open_channel(
            grpc_addr,
            env.get("GRPC_CLIENT_CERT") or None,
            env.get("GRPC_CLIENT_KEY") or None,
            env.get("GRPC_CA") or None,
            insecure=env.get("GRPC_INSECURE") == "1",
        )
        worker_id = env.get("WORKER_ID") or socket.gethostname()
        lease = _int_env(env, "LEASE_SECONDS", 1800)
        mode = "mTLS" if env.get("GRPC_CA") else "TEXTO CLARO (GRPC_INSECURE=1)"
        log(f"modo gRPC: {grpc_addr} ({mode}), worker {worker_id}, reserva {lease}s")
        return GrpcJobSource.connect(channel, worker_id, lease)
    if env.get("DATABASE_URL"):
        log("AVISO: modo legado DATABASE_URL está DEPRECADO — escreve directamente no "
            "Postgres e CONTORNA o DLP, as reservas e o limite de tentativas do servidor "
            "(ADR-0006 §3). Definir DELONIX_GRPC_ADDR e os certificados mTLS.")
        return LegacyDbJobSource(env["DATABASE_URL"])
    raise ConfigError("falta DELONIX_GRPC_ADDR (ou, deprecado, DATABASE_URL)")


def build_transcriber(env):
    kind = env.get("TRANSCRIBER", "whisper")
    if kind == "fake":
        log("AVISO: TRANSCRIBER=fake — não há modelo; só para testes")
        return FakeTranscriber(env.get("FAKE_TRANSCRIPT", "transcrição de teste."))
    if kind != "whisper":
        raise ConfigError(f"TRANSCRIBER={kind!r} desconhecido (whisper|fake)")
    return WhisperTranscriber(env.get("WHISPER_MODEL", "large-v3"),
                              env.get("WHISPER_DEVICE", "cuda"),
                              env.get("WHISPER_COMPUTE", "float16"), log)


def main(argv=None, env=None) -> int:
    env = os.environ if env is None else env
    parser = argparse.ArgumentParser(description="Delonix Meet — worker de transcrição")
    parser.add_argument("--once", action="store_true",
                        help="trata no máximo um trabalho e sai")
    opts = parser.parse_args(argv)

    source = None
    try:
        recordings_dir = env.get("RECORDINGS_DIR", "/recordings")
        poll_seconds = _int_env(env, "POLL_SECONDS", 20)
        # A fonte primeiro: um erro de configuração não deve esperar 3GB de modelo.
        source = build_source(env)
        transcriber = build_transcriber(env)
    except ConfigError as e:
        log(f"ERRO de configuração: {e}")
        if source is not None:
            source.close()
        return 2

    try:
        if opts.once:
            return EXIT_CODES[worker.process_one(source, transcriber, recordings_dir, log)]
        stop = threading.Event()
        signal.signal(signal.SIGTERM, lambda *_: stop.set())
        signal.signal(signal.SIGINT, lambda *_: stop.set())
        log("a sondar trabalhos")
        worker.run(source, transcriber, recordings_dir, poll_seconds, stop, log)
        log("terminado")
        return 0
    finally:
        source.close()


if __name__ == "__main__":
    sys.exit(main())
