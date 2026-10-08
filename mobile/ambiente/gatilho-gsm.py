#!/usr/bin/env python3 -I
"""Gatilho de GSM para testes no emulador.

Um teste a correr DENTRO do aparelho não consegue gerar uma chamada celular. Este servidor
corre no anfitrião e traduz três pedidos HTTP em `adb emu gsm …`; o emulador chega-lhe em
10.0.2.2. Só escuta em 127.0.0.1, só aceita as três rotas, e o número é fixo (nada do pedido
chega à linha de comandos).
"""
import subprocess
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

NUMERO = "244912345678"
ROTAS = {"/gsm/call": "call", "/gsm/accept": "accept", "/gsm/cancel": "cancel"}


class Gatilho(BaseHTTPRequestHandler):
    def do_GET(self):
        accao = ROTAS.get(self.path)
        if accao is None:
            self.send_error(404)
            return
        r = subprocess.run(["adb", "emu", "gsm", accao, NUMERO], capture_output=True, text=True, timeout=15)
        ok = r.returncode == 0 and "OK" in r.stdout
        self.send_response(200 if ok else 502)
        self.end_headers()
        self.wfile.write((r.stdout if ok else r.stderr or r.stdout).encode())

    def log_message(self, *_):
        pass


if __name__ == "__main__":
    porta = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    HTTPServer(("127.0.0.1", porta), Gatilho).serve_forever()
