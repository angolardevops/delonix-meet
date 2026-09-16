"""De onde vêm os trabalhos de transcrição, e para onde vai o resultado.

Duas implementações do mesmo contrato (`JobSource`):

- `GrpcJobSource` — o caminho suportado (ADR-0005 §3). O servidor é o dono do
  estado: reserva com prazo (lease), no máximo 5 tentativas, DLP aplicado ANTES
  de gravar, auditoria. O worker não toca na base.
- `LegacyDbJobSource` — o worker antigo a escrever directamente no Postgres.
  CONTORNA o DLP e as reservas; fica só para instalações que ainda não abriram
  o gRPC interno, e sai quando elas o abrirem.

O ciclo (`worker.py`) só conhece o contrato; nenhum dos dois sabe transcrever.
"""
from dataclasses import dataclass
from typing import Optional, Protocol


@dataclass(frozen=True)
class Job:
    recording_id: str
    media_file: str
    room_code: str = ""
    lease_token: str = ""
    lease_expires_unix: int = 0
    attempt: int = 1


class LeaseLost(Exception):
    """A reserva expirou ou passou para outro worker: o resultado não conta."""


class JobSource(Protocol):
    def claim(self) -> Optional[Job]: ...

    def complete(self, job: Job, transcript: str, minutes: str) -> None: ...

    def fail(self, job: Job, reason: str, retryable: bool) -> None: ...

    def close(self) -> None: ...


class ConfigError(Exception):
    """Configuração inválida — o processo sai com uma mensagem clara."""


# ---------------------------------------------------------------- gRPC

def _read(path: str) -> bytes:
    try:
        with open(path, "rb") as f:
            return f.read()
    except OSError as e:
        raise ConfigError(f"não consigo ler {path}: {e}") from e


def open_channel(addr: str, client_cert: Optional[str], client_key: Optional[str],
                 ca: Optional[str], insecure: bool):
    """Canal para o gRPC interno. mTLS com os três ficheiros; texto claro só com
    GRPC_INSECURE=1 (o servidor, por sua vez, só o aceita com
    DELONIX_ALLOW_INSECURE=1)."""
    import grpc

    given = [v for v in (client_cert, client_key, ca) if v]
    if len(given) == 3:
        creds = grpc.ssl_channel_credentials(
            root_certificates=_read(ca),
            private_key=_read(client_key),
            certificate_chain=_read(client_cert),
        )
        return grpc.secure_channel(addr, creds)
    if given:
        raise ConfigError(
            "mTLS do gRPC incompleto: são precisos os três GRPC_CLIENT_CERT, "
            "GRPC_CLIENT_KEY e GRPC_CA")
    if insecure:
        return grpc.insecure_channel(addr)
    raise ConfigError(
        f"DELONIX_GRPC_ADDR={addr} sem GRPC_CLIENT_CERT/GRPC_CLIENT_KEY/GRPC_CA — o gRPC "
        "interno exige mTLS (texto claro só com GRPC_INSECURE=1, e só em desenvolvimento)")


class GrpcJobSource:
    """Cliente do `delonix.meet.transcription.v1.TranscriptionService`.

    `stub` e `messages` são injectados (o stub gerado e o módulo `_pb2`) para o
    ciclo poder ser testado com dobras, sem servidor."""

    def __init__(self, stub, messages, worker_id: str, lease_seconds: int,
                 timeout_seconds: float = 30.0, channel=None):
        self._stub = stub
        self._m = messages
        self._worker_id = worker_id
        self._lease_seconds = lease_seconds
        self._timeout = timeout_seconds
        self._channel = channel

    @classmethod
    def connect(cls, channel, worker_id: str, lease_seconds: int) -> "GrpcJobSource":
        # Stubs gerados de server/proto por gen_protos.sh (nunca versionados).
        from delonix.meet.transcription.v1 import transcription_pb2, transcription_pb2_grpc

        stub = transcription_pb2_grpc.TranscriptionServiceStub(channel)
        return cls(stub, transcription_pb2, worker_id, lease_seconds, channel=channel)

    @staticmethod
    def _is_lease_lost(err: Exception) -> bool:
        code = getattr(err, "code", None)
        return callable(code) and getattr(code(), "name", "") == "FAILED_PRECONDITION"

    def claim(self) -> Optional[Job]:
        resp = self._stub.ClaimJob(
            self._m.ClaimJobRequest(worker_id=self._worker_id,
                                    lease_seconds=self._lease_seconds),
            timeout=self._timeout)
        if not resp.HasField("job"):
            return None
        j = resp.job
        return Job(recording_id=j.recording_id, media_file=j.media_file,
                   room_code=j.room_code, lease_token=j.lease_token,
                   lease_expires_unix=j.lease_expires_unix, attempt=j.attempt)

    def complete(self, job: Job, transcript: str, minutes: str) -> None:
        try:
            self._stub.CompleteJob(
                self._m.CompleteJobRequest(recording_id=job.recording_id,
                                           lease_token=job.lease_token,
                                           transcript=transcript, minutes=minutes),
                timeout=self._timeout)
        except Exception as e:
            if self._is_lease_lost(e):
                raise LeaseLost(str(e)) from e
            raise

    def fail(self, job: Job, reason: str, retryable: bool) -> None:
        try:
            self._stub.FailJob(
                self._m.FailJobRequest(recording_id=job.recording_id,
                                       lease_token=job.lease_token,
                                       reason=reason, retryable=retryable),
                timeout=self._timeout)
        except Exception as e:
            if self._is_lease_lost(e):
                raise LeaseLost(str(e)) from e
            raise

    def close(self) -> None:
        if self._channel is not None:
            self._channel.close()


# ---------------------------------------------------------------- legado

class LegacyDbJobSource:
    """DEPRECADO: escreve no Postgres por baixo do servidor — sem DLP, sem
    reserva, sem limite de tentativas (ADR-0005 §3). Comportamento igual ao do
    worker antigo, para não mudar nada a quem ainda depende dele."""

    def __init__(self, database_url: str):
        import psycopg2  # opcional: só este modo o usa

        self._conn = psycopg2.connect(database_url)
        self._conn.autocommit = False

    def claim(self) -> Optional[Job]:
        try:
            with self._conn.cursor() as cur:
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
            self._conn.commit()
        except Exception:
            self._conn.rollback()
            raise
        if not row:
            return None
        rec_id, room_code = row
        return Job(recording_id=str(rec_id), media_file=f"{rec_id}.webm",
                   room_code=room_code or "")

    def complete(self, job: Job, transcript: str, minutes: str) -> None:
        try:
            self._mark_done(job.recording_id, transcript, minutes)
            # A acta da reunião ligada (é o que o leitor mostra), se ainda vazia.
            if job.room_code:
                with self._conn.cursor() as cur:
                    cur.execute(
                        "UPDATE meetings SET transcript = %s, minutes = %s "
                        "WHERE room_code = %s AND transcript = ''",
                        (transcript, minutes, job.room_code),
                    )
            self._conn.commit()
        except Exception:
            self._conn.rollback()
            raise

    def fail(self, job: Job, reason: str, retryable: bool) -> None:
        # O worker antigo não tinha fila de falhas: um erro não-retryable (o
        # ficheiro em falta) marcava a gravação como processada para não a
        # repetir; um retryable deixava-a na fila.
        if retryable:
            return
        try:
            self._mark_done(job.recording_id, "", "")
            self._conn.commit()
        except Exception:
            self._conn.rollback()
            raise

    def _mark_done(self, rec_id: str, transcript: str, minutes: str) -> None:
        with self._conn.cursor() as cur:
            cur.execute(
                "UPDATE recordings SET transcript = %s, minutes = %s, "
                "transcribed_at = now() WHERE id = %s",
                (transcript, minutes, rec_id),
            )

    def close(self) -> None:
        self._conn.close()
