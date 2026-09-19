"""Testes do ciclo do worker — sem GPU, sem servidor, sem base.

Correr (da raiz do repo):
    python3 -m unittest discover -s ai-worker/tests

O `GrpcJobSource` é exercitado com um stub falso e mensagens falsas: prova-se
que o ciclo chama CompleteJob/FailJob com os argumentos certos e que um
FAILED_PRECONDITION vira `LeaseLost`. Que os nomes batem com o .proto real
prova-o o `tests/it_grpc.sh`, contra o servidor.
"""
import os
import sys
import tempfile
import threading
import types
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import transcribe_worker  # noqa: E402
import worker  # noqa: E402
from job_source import ConfigError, GrpcJobSource, Job, LeaseLost, open_channel  # noqa: E402
from minutes import build_mom  # noqa: E402
from transcriber import FakeTranscriber  # noqa: E402


# ------------------------------------------------------------------ dobras

class _Msg(types.SimpleNamespace):
    def HasField(self, name):
        return getattr(self, name, None) is not None


def _messages():
    return types.SimpleNamespace(
        ClaimJobRequest=lambda **kw: _Msg(kind="claim", **kw),
        CompleteJobRequest=lambda **kw: _Msg(kind="complete", **kw),
        FailJobRequest=lambda **kw: _Msg(kind="fail", **kw),
    )


class _RpcError(Exception):
    def __init__(self, code_name):
        super().__init__(code_name)
        self._code = types.SimpleNamespace(name=code_name)

    def code(self):
        return self._code


class FakeStub:
    def __init__(self, jobs=(), complete_error=None, fail_error=None, claim_error=None):
        self.jobs = list(jobs)
        self.calls = []
        self.complete_error = complete_error
        self.fail_error = fail_error
        self.claim_error = claim_error

    def ClaimJob(self, req, timeout=None):
        self.calls.append(req)
        if self.claim_error:
            raise self.claim_error
        return _Msg(job=self.jobs.pop(0) if self.jobs else None)

    def CompleteJob(self, req, timeout=None):
        self.calls.append(req)
        if self.complete_error:
            raise self.complete_error
        return _Msg()

    def FailJob(self, req, timeout=None):
        self.calls.append(req)
        if self.fail_error:
            raise self.fail_error
        return _Msg()


class BoomTranscriber:
    def transcribe(self, path):
        raise RuntimeError("CUDA out of memory")


def _job(rec="11111111-1111-1111-1111-111111111111", media=None, expires=0):
    return _Msg(recording_id=rec, lease_token="tok-1", media_file=media or f"{rec}.webm",
                room_code="abc-def", lease_expires_unix=expires, attempt=2)


def _nolog(_msg):
    pass


class WorkerLoopTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name

    def tearDown(self):
        self.tmp.cleanup()

    def _source(self, stub):
        return GrpcJobSource(stub, _messages(), worker_id="gpu-test", lease_seconds=900)

    def _touch(self, name):
        with open(os.path.join(self.dir, name), "wb") as f:
            f.write(b"\x1a\x45\xdf\xa3")

    def test_idle_when_no_job(self):
        stub = FakeStub()
        out = worker.process_one(self._source(stub), FakeTranscriber("x"), self.dir, _nolog)
        self.assertEqual(out, worker.Outcome.IDLE)
        self.assertEqual([c.kind for c in stub.calls], ["claim"])
        self.assertEqual(stub.calls[0].worker_id, "gpu-test")
        self.assertEqual(stub.calls[0].lease_seconds, 900)

    def test_completes_with_transcript_minutes_and_lease_token(self):
        job = _job()
        self._touch(job.media_file)
        stub = FakeStub([job])
        text = "Bom dia. Ficou decidido que o prazo é sexta."
        out = worker.process_one(self._source(stub), FakeTranscriber(text), self.dir, _nolog)
        self.assertEqual(out, worker.Outcome.COMPLETED)
        complete = stub.calls[-1]
        self.assertEqual(complete.kind, "complete")
        self.assertEqual(complete.recording_id, job.recording_id)
        self.assertEqual(complete.lease_token, "tok-1")
        self.assertEqual(complete.transcript, text)
        self.assertEqual(complete.minutes, build_mom(text))
        self.assertIn("## Ações / decisões", complete.minutes)

    def test_missing_file_fails_for_good(self):
        stub = FakeStub([_job()])
        out = worker.process_one(self._source(stub), FakeTranscriber("x"), self.dir, _nolog)
        self.assertEqual(out, worker.Outcome.FAILED)
        fail = stub.calls[-1]
        self.assertEqual(fail.kind, "fail")
        self.assertFalse(fail.retryable)
        self.assertEqual(fail.lease_token, "tok-1")
        self.assertIn("ficheiro em falta", fail.reason)

    def test_path_escaping_the_volume_is_refused_without_touching_disk(self):
        stub = FakeStub([_job(media="../../etc/passwd")])
        out = worker.process_one(self._source(stub), FakeTranscriber("x"), self.dir, _nolog)
        self.assertEqual(out, worker.Outcome.FAILED)
        self.assertFalse(stub.calls[-1].retryable)
        self.assertIn("media_file inválido", stub.calls[-1].reason)

    def test_transcription_error_is_retryable(self):
        job = _job()
        self._touch(job.media_file)
        stub = FakeStub([job])
        out = worker.process_one(self._source(stub), BoomTranscriber(), self.dir, _nolog)
        self.assertEqual(out, worker.Outcome.FAILED)
        fail = stub.calls[-1]
        self.assertEqual(fail.kind, "fail")
        self.assertTrue(fail.retryable)
        self.assertIn("CUDA out of memory", fail.reason)

    def test_lease_lost_on_complete_is_logged_and_skipped(self):
        job = _job(expires=1)
        self._touch(job.media_file)
        stub = FakeStub([job], complete_error=_RpcError("FAILED_PRECONDITION"))
        logs = []
        out = worker.process_one(self._source(stub), FakeTranscriber("x"), self.dir,
                                 logs.append, clock=lambda: 100.0)
        self.assertEqual(out, worker.Outcome.LEASE_LOST)
        self.assertTrue(any("reserva expirou" in m for m in logs), logs)
        self.assertTrue(any("reserva perdida" in m for m in logs), logs)

    def test_other_rpc_error_on_complete_does_not_raise(self):
        job = _job()
        self._touch(job.media_file)
        stub = FakeStub([job], complete_error=_RpcError("UNAVAILABLE"))
        out = worker.process_one(self._source(stub), FakeTranscriber("x"), self.dir, _nolog)
        self.assertEqual(out, worker.Outcome.ERROR)

    def test_claim_error_does_not_raise(self):
        stub = FakeStub(claim_error=_RpcError("UNAVAILABLE"))
        out = worker.process_one(self._source(stub), FakeTranscriber("x"), self.dir, _nolog)
        self.assertEqual(out, worker.Outcome.ERROR)

    def test_lease_lost_maps_only_failed_precondition(self):
        src = self._source(FakeStub(fail_error=_RpcError("FAILED_PRECONDITION")))
        with self.assertRaises(LeaseLost):
            src.fail(Job("r", "r.webm", lease_token="t"), "x", True)
        src = self._source(FakeStub(fail_error=_RpcError("INTERNAL")))
        with self.assertRaises(_RpcError):
            src.fail(Job("r", "r.webm", lease_token="t"), "x", True)

    def test_run_stops_promptly_on_stop_event(self):
        stub = FakeStub()
        stop = threading.Event()
        t = threading.Thread(target=worker.run,
                             args=(self._source(stub), FakeTranscriber("x"), self.dir,
                                   3600, stop, _nolog))
        t.start()
        stop.set()
        t.join(timeout=2)
        self.assertFalse(t.is_alive(), "o SIGTERM tem de interromper a espera")


class ConfigTest(unittest.TestCase):
    def test_grpc_without_certs_and_without_insecure_is_refused(self):
        with self.assertRaises(ConfigError) as e:
            open_channel("localhost:9180", None, None, None, insecure=False)
        self.assertIn("mTLS", str(e.exception))

    def test_partial_mtls_is_refused(self):
        with self.assertRaises(ConfigError):
            open_channel("localhost:9180", "/c.pem", None, "/ca.pem", insecure=True)

    def test_main_exits_2_on_config_error(self):
        self.assertEqual(transcribe_worker.main([], env={"DELONIX_GRPC_ADDR": "x:1"}), 2)
        self.assertEqual(transcribe_worker.main([], env={}), 2)

    def test_unknown_transcriber_is_refused(self):
        with self.assertRaises(ConfigError):
            transcribe_worker.build_transcriber({"TRANSCRIBER": "gpt"})


if __name__ == "__main__":
    unittest.main()
