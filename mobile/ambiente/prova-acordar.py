#!/usr/bin/env python3
"""Prova (ADR-0023): o PUSH de laboratório acorda a APP MORTA no emulador, que se regista e toca.

Mede a cadeia completa, com o servidor, o FreeSWITCH e a app REAIS (só o fornecedor de push é «de papel»):

  chamada ao ramal sem registo → FreeSWITCH segura-a e pede o wake → servidor real manda o push `lab` ao
  receptor desta máquina → o receptor abre a app (intent) → a app (morta até aí) arranca o motor com a conta
  guardada, regista-se por TLS → o INVITE que estava à espera chega → «Chamada a entrar» → atende-se → em curso.

Pré-requisitos (ver mobile/ambiente/prova-acordar.sh, que os trata): emulador a correr, APK de debug instalado,
laboratório levantado com `make compose-up LAN_IP=<ip> PUSH_LAB_URL=http://<ip>:18890/push`, e a branch do
servidor com o S-01 (PR #279). O que NÃO prova: FCM/APNs reais, nem um telemóvel físico, nem a app a ser morta
pelo sistema (usa-se `am force-stop`), nem áudio (o emulador corre sem som: mede-se o estado, não a voz).

Cria uma pessoa de prova no laboratório (com ramal e aparelho) e deixa-a lá. As credenciais dela são
descartáveis, geradas aqui, e só vão ao emulador por um intent de debug; nunca são impressas.
"""
import argparse, json, os, re, secrets, ssl, subprocess, sys, threading, time, urllib.request, xml.etree.ElementTree as ET
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

PACOTE = "ao.ngolacloud.delonixphone"
falhas = 0

def linha(ok, texto, det=""):
    global falhas
    falhas += 0 if ok else 1
    print(f"{'PASS' if ok else 'FAIL'}  {texto}" + (f"  [{det}]" if det else ""), flush=True)

def adb(*args, timeout=60):
    return subprocess.run(["adb", *args], capture_output=True, text=True, timeout=timeout).stdout.strip()

def a_correr():
    return bool(adb("shell", "pidof", PACOTE))

def ecra():
    # Apaga antes: se o dump falhar (ecrã desligado, sistema ocupado) lê-se vazio, e não o ecrã de uma corrida antiga.
    adb("shell", "rm", "-f", "/sdcard/u.xml")
    adb("shell", "uiautomator", "dump", "/sdcard/u.xml")
    return adb("shell", "cat", "/sdcard/u.xml")

def ha_texto(xml, texto):
    return bool(re.search(rf'(?:text|content-desc)="[^"]*{re.escape(texto)}[^"]*"', xml))

def tocar(texto):
    """Toca no controlo cujo texto (ou content-desc) contém `texto`, lido do ecrã real."""
    xml = ecra()
    for no in re.finditer(r"<node [^>]*>", xml):
        n = no.group(0)
        if re.search(rf'(?:text|content-desc)="[^"]*{re.escape(texto)}[^"]*"', n):
            b = list(map(int, re.search(r'bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"', n).groups()))
            adb("shell", "input", "tap", str((b[0] + b[2]) // 2), str((b[1] + b[3]) // 2))
            return True
    return False

def esperar(cond, segundos, passo=1.0):
    fim = time.time() + segundos
    while time.time() < fim:
        v = cond()
        if v:
            return v
        time.sleep(passo)
    return None

class Lab:
    def __init__(self, d):
        self.d = Path(d)
        env = dict(l.split("=", 1) for l in (self.d / ".env").read_text().splitlines() if "=" in l and not l.startswith("#"))
        self.senha, self.esl = env["MEET_ADMIN_PASSWORD"], env["TELEPHONY_ESL_PASSWORD"]
        lan = (self.d / "deploy/compose/generated/lan.yaml").read_text()
        self.base = re.search(r"CORS_ORIGINS:\s*(https://[^,\s]+)", lan).group(1)
        self.ip = re.search(r"https://([\d.]+):", self.base).group(1)
        self.ctx = ssl.create_default_context(cafile=str(self.d / "deploy/compose/generated/lan-tls/ca.crt"))
        self.tok = None
    def api(self, m, p, b=None, bruto=False, token=None):
        t = token or self.tok
        r = urllib.request.Request(self.base + p, method=m, data=json.dumps(b).encode() if b is not None else None,
            headers={"content-type": "application/json", **({"authorization": "Bearer " + t} if t else {})})
        try:
            with urllib.request.urlopen(r, context=self.ctx, timeout=20) as x:
                d = x.read().decode()
                return d if bruto else json.loads(d or "null")
        except urllib.error.HTTPError as e:
            return {"_http": e.code}
    def entrar(self):
        self.tok = self.api("POST", "/api/auth/login", {"email": "admin@ngolacloud.local", "password": self.senha})["access_token"]
        self.org = next(o for o in self.api("GET", "/api/orgs") if o["name"] == "ngolacloud")
        self.dominio = self.org["slug"] + ".ramais.delonix.meet"
    def fs(self, cmd, timeout=120):
        return subprocess.run(["delonix", "container", "exec", "delonix-freeswitch", "fs_cli", "-H", "127.0.0.1", "-p", self.esl, "-x", cmd],
                              capture_output=True, text=True, timeout=timeout).stdout.strip()

class Receptor(BaseHTTPRequestHandler):
    """O fornecedor `lab`: recebe o pedido do servidor REAL e abre a app com um intent (o «push»)."""
    pedidos = []
    def do_POST(self):
        n = int(self.headers.get("content-length", 0))
        try: corpo = json.loads(self.rfile.read(n) or b"{}")
        except ValueError: corpo = {}
        Receptor.pedidos.append({"t": time.time(), **corpo})
        self.send_response(200); self.send_header("content-length", "0"); self.end_headers()
        call, caller = str(corpo.get("call_uuid", "")), str(corpo.get("caller", ""))
        # O que vai à linha de comandos do aparelho é validado: o `adb shell` junta os argumentos num comando.
        if re.fullmatch(r"[0-9a-f-]{1,64}", call) and re.fullmatch(r"[A-Za-z0-9_.+-]{0,40}", caller):
            Receptor.t_push = time.time()
            adb("shell", "am", "start", "-n", f"{PACOTE}/.MainActivity", "--es", "dlx_acordar", call, "--es", "dlx_chamador", caller or "-")
    def log_message(self, *_): pass

def _md5(t):
    import hashlib
    return hashlib.md5(t.encode()).hexdigest()

class Chamador:
    """Um telefone SIP de papel, por TLS, com SRTP (SDES) e PCMU: o que um telefone real mandaria.
    Liga a um ramal e regista os estados que vê (180, 200, ...), com a hora de cada um. Não é o
    `originate` com perna de loopback: essa perna só fala L16, e o FreeSWITCH oferecia só L16 à app."""
    def __init__(self, lab, user, senha, dominio):
        self.lab, self.user, self.senha, self.dom = lab, user, senha, dominio
        ctx = ssl.create_default_context(cafile=str(lab.d / "deploy/compose/generated/lan-tls/ca.crt"))
        self.s = ctx.wrap_socket(__import__("socket").create_connection((lab.ip, 5071), timeout=10), server_hostname=lab.ip)
        self.ip, self.lp = self.s.getsockname()
        self.call_id, self.tag = secrets.token_hex(8) + "@prova", secrets.token_hex(4)
        self.estados = []   # [(codigo, hora)]
        self.cseq = 0
        self.a_to = ""
    def _ler(self):
        d = b""
        while b"\r\n\r\n" not in d:
            p = self.s.recv(4096)
            if not p: return None
            d += p
        cab, _, resto = d.partition(b"\r\n\r\n")
        m = re.search(rb"(?im)^content-length:\s*(\d+)", cab); falta = (int(m.group(1)) if m else 0) - len(resto)
        while falta > 0:
            p = self.s.recv(4096)
            if not p: break
            falta -= len(p)
        return cab.decode(errors="replace")
    def ligar(self, numero, timeout=60):
        uri, ident = f"sip:{numero}@{self.dom}", f"sip:{self.user}@{self.dom}"
        sdp = "\r\n".join(["v=0", f"o=prova 1 1 IN IP4 {self.ip}", "s=prova", f"c=IN IP4 {self.ip}", "t=0 0",
            "m=audio 40000 RTP/SAVP 0 101", "a=rtpmap:0 PCMU/8000", "a=rtpmap:101 telephone-event/8000", "a=fmtp:101 0-15",
            "a=crypto:1 AES_CM_128_HMAC_SHA1_80 inline:" + __import__("base64").b64encode(secrets.token_bytes(30)).decode(), "a=sendrecv", ""])
        def invite(cseq, auth=None, cab_auth="Authorization"):
            cab = [f"INVITE {uri} SIP/2.0", f"Via: SIP/2.0/TLS {self.ip}:{self.lp};branch=z9hG4bK{secrets.token_hex(6)};rport", "Max-Forwards: 70",
                   f'From: "prova" <{ident}>;tag={self.tag}', f"To: <{uri}>", f"Call-ID: {self.call_id}", f"CSeq: {cseq} INVITE",
                   f"Contact: <sip:{self.user}@{self.ip}:{self.lp};transport=tls>", "Content-Type: application/sdp"]
            if auth: cab.append(f"{cab_auth}: {auth}")
            cab += [f"Content-Length: {len(sdp)}", "", sdp]
            return "\r\n".join(cab).encode()
        def ack(cseq, to):
            return "\r\n".join([f"ACK {uri} SIP/2.0", f"Via: SIP/2.0/TLS {self.ip}:{self.lp};branch=z9hG4bK{secrets.token_hex(6)};rport", "Max-Forwards: 70",
                   f'From: "prova" <{ident}>;tag={self.tag}', f"To: {to}", f"Call-ID: {self.call_id}", f"CSeq: {cseq} ACK", "Content-Length: 0", "", ""]).encode()
        self.s.sendall(invite(1)); cseq = 1
        fim = time.time() + timeout; self.s.settimeout(5)
        while time.time() < fim:
            try: m = self._ler()
            except OSError: continue
            if m is None: return
            codigo = int(re.match(r"SIP/2\.0 (\d{3})", m).group(1)) if m.startswith("SIP/2.0") else 0
            to = re.search(r"(?im)^To:\s*(.*)$", m).group(1).strip() if re.search(r"(?im)^To:", m) else ""
            if codigo in (401, 407) and cseq == 1:
                self.s.sendall(ack(1, to))
                # 401 desafia com WWW-Authenticate e responde-se com Authorization; 407, com Proxy-*.
                desafio, resposta = ("www-authenticate", "Authorization") if codigo == 401 else ("proxy-authenticate", "Proxy-Authorization")
                p = {k.lower(): a or b for k, a, b in re.findall(r'(\w+)\s*=\s*(?:"([^"]*)"|([^\s,]+))', re.search(rf"(?im)^{desafio}:\s*(.*)$", m).group(1))}
                cn = secrets.token_hex(8)
                resp = _md5(f"{_md5(f'{self.user}:{p['realm']}:{self.senha}')}:{p['nonce']}:00000001:{cn}:auth:{_md5(f'INVITE:{uri}')}")
                cseq = 2
                self.s.sendall(invite(2, f'Digest username="{self.user}", realm="{p["realm"]}", nonce="{p["nonce"]}", uri="{uri}", response="{resp}", algorithm=MD5, qop=auth, nc=00000001, cnonce="{cn}"', resposta))
                continue
            self.estados.append((codigo, time.time()))
            if 200 <= codigo < 300:
                self.a_to = to; self.cseq = cseq
                self.s.sendall(ack(cseq, to)); return
            if codigo >= 300:
                self.s.sendall(ack(cseq, to)); return
    def desligar(self):
        try:
            uri = f"sip:{self.user}@{self.dom}"
            self.s.sendall("\r\n".join([f"BYE sip:mod_sofia@{self.lab.ip}:5071;transport=tls SIP/2.0", f"Via: SIP/2.0/TLS {self.ip}:{self.lp};branch=z9hG4bK{secrets.token_hex(6)};rport",
                "Max-Forwards: 70", f'From: "prova" <sip:{self.user}@{self.dom}>;tag={self.tag}', f"To: {self.a_to}", f"Call-ID: {self.call_id}",
                f"CSeq: {self.cseq + 1} BYE", "Content-Length: 0", "", ""]).encode())
        except OSError: pass
        try: self.s.close()
        except OSError: pass

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--lab", required=True); ap.add_argument("--porta", type=int, default=18890)
    a = ap.parse_args()
    lab = Lab(a.lab); lab.entrar()
    suf = secrets.token_hex(3); email, senha = f"app-prova-{suf}@ngolacloud.local", secrets.token_urlsafe(18) + "Aa1!"
    lab.api("POST", f"/api/orgs/{lab.org['id']}/members", {"email": email, "username": f"app-prova-{suf}", "password": senha, "role": "member", "title": "Prova da app"})
    uid = lab.api("POST", "/api/auth/login", {"email": email, "password": senha})["user"]["id"]
    usados = {e["extension"] for e in lab.api("GET", f"/api/orgs/{lab.org['id']}/extensions")}
    numero = next(str(n) for n in range(1960, 2000) if str(n) not in usados)
    ext = lab.api("POST", f"/api/orgs/{lab.org['id']}/extensions", {"extension": numero, "member_id": uid})
    ticket = lab.api("POST", f"/api/orgs/{lab.org['id']}/extensions/{ext['id']}/provisioning-ticket", {})["provisioning_url"]
    # um segundo ramal só para ligar (a perna chamadora do loopback)
    cham = lab.api("POST", f"/api/orgs/{lab.org['id']}/extensions", {"extension": str(int(numero) + 1), "label": "Prova app (chamador)"})
    url_c = lab.api("POST", f"/api/orgs/{lab.org['id']}/extensions/{cham['id']}/provisioning-ticket", {})["provisioning_url"]
    xml_c = lab.api("GET", url_c[len(lab.base):], bruto=True)
    chamador = re.search(r'name="username"[^>]*>([^<]*)<', xml_c).group(1)
    senha_chamador = re.search(r'name="passwd"[^>]*>([^<]*)<', xml_c).group(1)
    sip_user = ext.get("sip_username")
    aor = f"{sip_user}@{lab.dominio}"
    srv = None
    try:
        # 1. a app configura-se por intent de debug: provisiona, entra no Meet, regista o aparelho lab, arranca o motor
        adb("shell", "am", "force-stop", PACOTE); adb("shell", "pm", "clear", PACOTE)
        adb("shell", "pm", "grant", PACOTE, "android.permission.RECORD_AUDIO")
        adb("shell", "am", "start", "-n", f"{PACOTE}/.MainActivity", "--es", "dlx_configurar_url", ticket, "--es", "dlx_email", email, "--es", "dlx_senha", senha)
        registou = esperar(lambda: aor in lab.fs("sofia status profile internal reg"), 90)
        aparelhos = lab.api("GET", f"/api/orgs/{lab.org['id']}/extensions/{ext['id']}/devices")
        linha(bool(registou) and isinstance(aparelhos, list) and len(aparelhos) == 1,
              "a app configura-se por intent: provisiona, entra no Meet, regista o aparelho e o motor regista-se no FreeSWITCH",
              f"registo={'sim' if registou else 'não'}, aparelhos={len(aparelhos) if isinstance(aparelhos, list) else aparelhos}")
        if not registou or not isinstance(aparelhos, list) or not aparelhos:
            return
        dev = aparelhos[0]["id"]
        # 2. a app «morre» e o registo caduca
        adb("shell", "am", "force-stop", PACOTE); lab.fs(f"sofia profile internal flush_inbound_reg {aor}")
        linha(not a_correr() and aor not in lab.fs("sofia status profile internal reg"), "a app está morta e o ramal sem registo")
        # 3. a chamada, feita por um telefone SIP de papel (TLS, SRTP, PCMU), não por um loopback
        srv = ThreadingHTTPServer((lab.ip, a.porta), Receptor); threading.Thread(target=srv.serve_forever, daemon=True).start()
        lab.fs("global_setvar delonix_push_wake_url="); lab.fs("global_setvar delonix_push_wait_secs=40")
        Receptor.pedidos.clear(); Receptor.t_push = None
        ch = Chamador(lab, chamador, senha_chamador, lab.dominio)
        t0 = time.time()
        th = threading.Thread(target=lambda: ch.ligar(numero, 70)); th.start()
        a_tocar = esperar(lambda: ha_texto(ecra(), "Chamada a entrar"), 60, 1.5)
        t_toca = time.time() - t0
        linha(len(Receptor.pedidos) == 1 and Receptor.pedidos[0].get("device_id") == dev,
              "o servidor real pediu ao fornecedor para acordar o aparelho certo", f"pedidos={len(Receptor.pedidos)}")
        linha(a_correr() and bool(a_tocar), "a app acordou, registou-se e mostra «Chamada a entrar»", f"{t_toca:.1f}s" if a_tocar else "o ecrã nunca mostrou a chamada")
        if not a_tocar:
            textos = re.findall(r'(?:text|content-desc)="([^"]+)"', ecra())
            print("   ecrã:", [t for t in textos if t][:10])
            print("   estados vistos pelo chamador:", [c for c, _ in ch.estados])
            ch.desligar(); th.join(timeout=5); return
        linha(180 in [c for c, _ in ch.estados], "o chamador ouve «180 Ringing» enquanto a app toca", str([c for c, _ in ch.estados]))
        # 4. atende-se no ecrã
        atendeu = tocar("Atender")
        th.join(timeout=40)
        codigos = [c for c, _ in ch.estados]
        linha(atendeu and 200 in codigos, "atender no ecrã: o chamador recebe «200 OK» (chamada estabelecida)", str(codigos))
        em_curso = esperar(lambda: ha_texto(ecra(), "Em chamada"), 20, 1.0)
        linha(bool(em_curso), "a app mostra «Em chamada»")
        tocar("Terminar"); ch.desligar()
        # 5. controlo negativo: aparelho revogado -> falha já, e a app NÃO é acordada
        time.sleep(1); adb("shell", "am", "force-stop", PACOTE); lab.fs(f"sofia profile internal flush_inbound_reg {aor}")
        lab.api("DELETE", f"/api/orgs/{lab.org['id']}/extensions/{ext['id']}/devices/{dev}")
        Receptor.pedidos.clear()
        ch2 = Chamador(lab, chamador, senha_chamador, lab.dominio)
        t1 = time.time(); ch2.ligar(numero, 15); dt = time.time() - t1
        final = ch2.estados[-1][0] if ch2.estados else None
        time.sleep(2)
        linha(final is not None and final >= 400 and dt < 4 and not Receptor.pedidos and not a_correr(),
              "aparelho revogado: o chamador recebe um erro já, ninguém é acordado e a app continua morta", f"final={final} em {dt:.2f}s")
        ch2.desligar()
    finally:
        lab.fs("global_setvar delonix_push_wait_secs=0")
        lab.fs(f"sofia profile internal flush_inbound_reg {aor}")
        adb("shell", "am", "force-stop", PACOTE)
        if srv: srv.shutdown()
        print(f"\n{falhas} falha(s)  (pessoa de prova: app-prova-{suf}, ramal {numero}; ficam no laboratório)")
    sys.exit(min(falhas, 255))

if __name__ == "__main__":
    main()
