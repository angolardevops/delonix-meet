"""Transcrição: o modelo, separado de onde vêm os trabalhos.

`WhisperTranscriber` é o de produção (faster-whisper, GPU quando há). O
`FakeTranscriber` só existe para os testes (TRANSCRIBER=fake): devolve um texto
fixo sem carregar modelo nenhum, para o circuito gRPC poder ser provado numa
máquina sem GPU nem faster-whisper instalado.
"""
import math
from dataclasses import dataclass
from typing import Callable, Optional, Protocol, Tuple


@dataclass(frozen=True)
class Segment:
    """Um segmento com tempos, na forma do `TranscriptSegment` do contrato."""
    start_ms: int
    end_ms: int
    text: str
    confidence: Optional[float] = None


@dataclass(frozen=True)
class Transcription:
    """O texto inteiro, os segmentos com tempos e a língua detectada. Os
    segmentos são o que dá legendas, capítulos e «em que minuto se disse isto»
    (R183); o servidor aplica-lhes o DLP."""
    text: str
    segments: Tuple[Segment, ...] = ()
    language: str = ""


def confidence_from_logprob(avg_logprob) -> Optional[float]:
    """`exp(avg_logprob)` preso a 0..1; `None` se o modelo não a deu."""
    try:
        v = math.exp(float(avg_logprob))
    except (TypeError, ValueError, OverflowError):
        return None
    if not math.isfinite(v):
        return None
    return min(max(v, 0.0), 1.0)


class Transcriber(Protocol):
    def transcribe(self, path: str) -> Transcription: ...


class WhisperTranscriber:
    def __init__(self, model_name: str, device: str, compute: str,
                 log: Callable[[str], None]):
        # Import tardio: os testes e o modo fake não precisam do faster-whisper.
        from faster_whisper import WhisperModel

        log(f"a carregar modelo {model_name} em {device}/{compute}…")
        try:
            self._model = WhisperModel(model_name, device=device, compute_type=compute)
        except Exception as e:  # GPU indisponível → cai para CPU (mais lento)
            log(f"falha a carregar em {device} ({e}); a tentar CPU/int8")
            self._model = WhisperModel(model_name, device="cpu", compute_type="int8")
        log("modelo pronto")

    def transcribe(self, path: str) -> Transcription:
        # vad_filter corta silêncios; language=None deixa o modelo detetar (PT/EN/…).
        raw, info = self._model.transcribe(path, vad_filter=True, beam_size=5)
        segments = tuple(
            Segment(
                start_ms=int(round(seg.start * 1000)),
                end_ms=int(round(seg.end * 1000)),
                text=seg.text.strip(),
                confidence=confidence_from_logprob(getattr(seg, "avg_logprob", None)),
            )
            for seg in raw  # gerador: a transcrição corre aqui
            if seg.text.strip()
        )
        return Transcription(
            text=" ".join(s.text for s in segments).strip(),
            segments=segments,
            language=getattr(info, "language", "") or "",
        )


class FakeTranscriber:
    """Só para testes: não lê o áudio, devolve `text`."""

    def __init__(self, text: str, segments: Tuple[Segment, ...] = (), language: str = ""):
        self._result = Transcription(text=text, segments=tuple(segments), language=language)

    def transcribe(self, path: str) -> Transcription:
        return self._result
