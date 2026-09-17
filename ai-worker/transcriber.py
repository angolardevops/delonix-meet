"""Transcrição: o modelo, separado de onde vêm os trabalhos.

`WhisperTranscriber` é o de produção (faster-whisper, GPU quando há). O
`FakeTranscriber` só existe para os testes (TRANSCRIBER=fake): devolve um texto
fixo sem carregar modelo nenhum, para o circuito gRPC poder ser provado numa
máquina sem GPU nem faster-whisper instalado.
"""
from typing import Callable, Protocol


class Transcriber(Protocol):
    def transcribe(self, path: str) -> str: ...


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

    def transcribe(self, path: str) -> str:
        # vad_filter corta silêncios; language=None deixa o modelo detetar (PT/EN/…).
        segments, _info = self._model.transcribe(path, vad_filter=True, beam_size=5)
        return " ".join(seg.text.strip() for seg in segments).strip()


class FakeTranscriber:
    """Só para testes: não lê o áudio, devolve `text`."""

    def __init__(self, text: str):
        self._text = text

    def transcribe(self, path: str) -> str:
        return self._text
