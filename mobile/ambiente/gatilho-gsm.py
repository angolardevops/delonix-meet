#!/usr/bin/env python3 -I
"""Gatilho de GSM para testes no emulador.

Um teste a correr DENTRO do aparelho não consegue gerar uma chamada celular. Este servidor
corre no anfitrião e traduz três pedidos HTTP em `adb emu gsm …`; o emulador chega-lhe em
10.0.2.2. Só escuta em 127.0.0.1, só aceita as três rotas, e o número é fixo (nada do pedido
chega à linha de comandos).
"""
import json
import re
import ssl
import subprocess
import sys
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

NUMERO = "244912345678"
ROTAS = {"/gsm/call": "call", "/gsm/accept": "accept", "/gsm/cancel": "cancel"}

# ---- Laboratório do Meet (compose em modo LAN, `make compose-up LAN_IP=…`) ----------------
# O teste Patrol precisa de um bilhete de provisionamento FRESCO e só o administrador o emite.
# O anfitrião tem as credenciais de laboratório (.env do worktree do laboratório); o aparelho
# nunca as vê, só o bilhete ou as credenciais do ramal DE TESTE. Nunca se escreve segredo no log.
import os

# DP_LAB_DIR muda de sítio; por omissão o worktree do laboratório criado sobre a develop
LAB = Path(os.environ.get("DP_LAB_DIR") or Path(__file__).resolve().parents[3] / "laboratorio-develop")
RAMAL_TESTE = ("1900", "DelonixPhone (emulador)")  # ramal da empresa, só para estes testes


class Laboratorio:
    def __init__(self):
        env = dict(l.split("=", 1) for l in (LAB / ".env").read_text().splitlines() if "=" in l and not l.startswith("#"))
        self.senha = env["MEET_ADMIN_PASSWORD"]
        # a 1.ª origem de CORS é a que o telemóvel alcança (a que o servidor põe no QR)
        origem = re.search(r"CORS_ORIGINS:\s*(https://[^,\s]+)", (LAB / "deploy/compose/generated/lan.yaml").read_text())
        self.base = origem.group(1)
        self.ctx = ssl.create_default_context(cafile=str(LAB / "deploy/compose/generated/lan-tls/ca.crt"))
        self.token = None

    def chamar(self, metodo, caminho, corpo=None, bruto=False):
        pedido = urllib.request.Request(
            self.base + caminho, method=metodo, data=json.dumps(corpo).encode() if corpo is not None else None,
            headers={"content-type": "application/json", **({"authorization": "Bearer " + self.token} if self.token else {})})
        with urllib.request.urlopen(pedido, context=self.ctx, timeout=15) as r:
            dados = r.read().decode()
            return dados if bruto else json.loads(dados or "null")

    def entrar(self):
        if self.token is None:
            r = self.chamar("POST", "/api/auth/login", {"email": "admin@ngolacloud.local", "password": self.senha})
            self.token = r["access_token"]

    def ramal(self):
        self.entrar()
        org = next(o for o in self.chamar("GET", "/api/orgs") if o["name"] == "ngolacloud")["id"]
        num, etiqueta = RAMAL_TESTE
        for e in self.chamar("GET", f"/api/orgs/{org}/extensions"):
            if e["extension"] == num:
                return org, e["id"]
        e = self.chamar("POST", f"/api/orgs/{org}/extensions", {"extension": num, "label": etiqueta})
        return org, e["id"]

    def bilhete(self):
        org, ext = self.ramal()
        return self.chamar("POST", f"/api/orgs/{org}/extensions/{ext}/provisioning-ticket", {})["provisioning_url"]

    def resgatar(self, url):
        return self.chamar("GET", url[len(self.base):], bruto=True)

    def tocar(self, q):
        """O FreeSWITCH origina uma chamada para o ramal [u]@[d] e, ao atender, toca um tom de 440 Hz sem parar (`endless_playback`: o `playback` de um tom acaba sozinho e o servidor desligava a chamada ao fim de 1 s).
        Só o anfitrião fala com o Event Socket (a password vem do .env do laboratório)."""
        u, d = q.get("u", ""), q.get("d", "")
        if not (SEGURO.match(u) and SEGURO.match(d)):
            raise KeyError("u/d")
        env = dict(l.split("=", 1) for l in (LAB / ".env").read_text().splitlines() if "=" in l and not l.startswith("#"))
        r = subprocess.run(["delonix", "container", "exec", "delonix-freeswitch", "fs_cli", "-H", "127.0.0.1", "-p",
                            env["TELEPHONY_ESL_PASSWORD"], "-x",
                            f"originate {{ignore_early_media=true,originate_timeout=25}}user/{u}@{d} &endless_playback(tone_stream://%(1000,0,440))"],
                           capture_output=True, text=True, timeout=40)
        return {"ok": "+OK" in r.stdout}

    def credenciais(self):
        """Gasta um bilhete e devolve a conta do ramal de teste (para o caminho manual)."""
        raiz = ET.fromstring(self.resgatar(self.bilhete()))
        # `iter("{*}x")` não aceita o curinga do espaço de nomes (só `find`): filtra-se à mão
        cfg = {(s.get("name"), e.get("name")): (e.text or "")
               for s in raiz.iter() if s.tag.endswith("}section") for e in s if e.tag.endswith("}entry")}
        proxy = re.search(r"sip:([^:;>]+):(\d+)(?:;transport=(\w+))?", cfg[("proxy_0", "reg_proxy")])
        return {"utilizador": cfg[("auth_info_0", "username")], "palavraPasse": cfg[("auth_info_0", "passwd")],
                "dominio": cfg[("auth_info_0", "domain")], "servidor": f"{proxy.group(1)}:{proxy.group(2)}",
                "transporte": (proxy.group(3) or "udp").lower()}


LAB_ROTAS = {"/lab/bilhete", "/lab/credenciais", "/lab/bilhete-usado", "/lab/tocar"}
SEGURO = re.compile(r"^[A-Za-z0-9_.-]{1,80}$")  # o que entra na linha do fs_cli: nada de espaços nem aspas
_lab = None


class Gatilho(BaseHTTPRequestHandler):
    def do_GET(self):
        rota, _, consulta = self.path.partition("?")
        if rota == "/adb/microfone":
            # Comando FIXO, sem nada do pedido: dá o microfone à app de testes. Os testes de chamada não
            # provam o diálogo do sistema (isso é do teste da permissão do telefone): a automação do
            # diálogo falhava com a máquina carregada.
            r = subprocess.run(["adb", "shell", "pm", "grant", "ao.ngolacloud.delonixphone", "android.permission.RECORD_AUDIO"],
                               capture_output=True, text=True, timeout=15)
            self.send_response(200 if r.returncode == 0 else 502)
            self.end_headers()
            self.wfile.write(b"ok" if r.returncode == 0 else r.stderr.encode())
            return
        if rota in LAB_ROTAS:
            self._lab(rota, consulta)
            return
        accao = ROTAS.get(self.path)
        if accao is None:
            self.send_error(404)
            return
        r = subprocess.run(["adb", "emu", "gsm", accao, NUMERO], capture_output=True, text=True, timeout=15)
        ok = r.returncode == 0 and "OK" in r.stdout
        self.send_response(200 if ok else 502)
        self.end_headers()
        self.wfile.write((r.stdout if ok else r.stderr or r.stdout).encode())

    def _lab(self, rota, consulta):
        global _lab
        try:
            _lab = _lab or Laboratorio()
            if rota == "/lab/bilhete":
                corpo = {"url": _lab.bilhete()}
            elif rota == "/lab/credenciais":
                corpo = _lab.credenciais()
            elif rota == "/lab/tocar":
                corpo = _lab.tocar(dict(p.split("=", 1) for p in consulta.split("&") if "=" in p))
            else:  # um URL que já foi resgatado: para provar que o 2.º uso é recusado
                url = _lab.bilhete()
                _lab.resgatar(url)
                corpo = {"url": url}
            codigo, dados = 200, json.dumps(corpo)
        except (urllib.error.URLError, OSError, KeyError, StopIteration, ET.ParseError) as e:
            codigo, dados = 502, f"laboratório indisponível: {type(e).__name__}"  # nunca o conteúdo: pode ter segredos
        self.send_response(codigo)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(dados.encode())

    def log_message(self, *_):
        pass


if __name__ == "__main__":
    porta = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    HTTPServer(("127.0.0.1", porta), Gatilho).serve_forever()
