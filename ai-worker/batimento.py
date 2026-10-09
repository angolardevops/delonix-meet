"""O batimento do worker — o único sinal de vida que ele tem.

PORQUE EXISTE. O worker não serve HTTP e não tem Service: não há nada que uma
sonda possa chamar. Um worker pendurado (CUDA travado, gRPC a bloquear sem
timeout) fica `Running` para sempre, com a reserva da gravação na mão até ela
expirar — e nada acusa. O Deployment está `Available`, com um pod, a não fazer
nada.

PULSADO POR PROGRESSO, NÃO POR RELÓGIO. Esta é a decisão que faz a sonda valer
algo. Um batimento por relógio (um thread a tocar o ficheiro a cada 30 s)
continuaria a tocá-lo com o trabalho pendurado — e seria uma sonda que não prova
nada, como um ServiceMonitor sem alvos. Por isso o pulso parte de quem avança:
uma volta do ciclo, e cada segmento que o transcritor produz.

O FICHEIRO SÓ NASCE DEPOIS DO MODELO. É o que deixa o `startupProbe` distinguir
«ainda a descarregar o large-v3» (~3 GB no primeiro arranque) de «pendurado»: o
startup espera que o ficheiro APAREÇA, com folga larga; só depois a liveness
começa a olhar para a IDADE dele.
"""
import os
import time
from typing import Optional


class Batimento:
    """Toca um ficheiro a cada sinal de progresso. `caminho=None` desliga-o."""

    def __init__(self, caminho: Optional[str], log=lambda _m: None):
        self._caminho = caminho or None
        self._log = log
        self._queixou = False

    @property
    def caminho(self) -> Optional[str]:
        return self._caminho

    def pulso(self) -> None:
        """Marca progresso. NUNCA levanta: um batimento que falha não pode
        derrubar a transcrição que estava a correr bem. Queixa-se UMA vez — a
        partir daí é o `startupProbe` que não vê o ficheiro aparecer e o
        Kubernetes que decide, que é o sítio certo para essa decisão."""
        if not self._caminho:
            return
        try:
            agora = time.time()
            with open(self._caminho, "a"):
                pass
            os.utime(self._caminho, (agora, agora))
        except OSError as e:
            if not self._queixou:
                self._queixou = True
                self._log(f"batimento: não consigo escrever {self._caminho} ({e}) — "
                          "as sondas vão tratar este pod como morto")

    def idade(self) -> Optional[float]:
        """Segundos desde o último pulso, ou None se o ficheiro ainda não
        existe. Só serve aos testes: no cluster é a sonda `exec` que mede."""
        if not self._caminho:
            return None
        try:
            return time.time() - os.stat(self._caminho).st_mtime
        except OSError:
            return None
