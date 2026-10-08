#!/usr/bin/env python3
"""Prova do S-02 (ADR-0023): o FreeSWITCH segura uma chamada para um ramal sem registo, avisa o control
plane (o «wake») e liga-a quando o aparelho se regista — ou desiste ao fim do limite.

Corre contra o LABORATÓRIO em modo LAN (`make compose-up LAN_IP=…`), a partir do worktree do laboratório:

    python3 -I scripts/ramais-push-espera-prova.py --lab <worktree do laboratório>

Mede (cada linha é PASS ou FAIL; a saída é o número de FAIL):
  1. com `delonix_push_wake_url` a dizer `awaiting:true` e o destino a registar-se 5 s depois, o INVITE
     chega ao aparelho DEPOIS do registo, e o servidor recebeu UM pedido de wake com o ramal certo;
  2. controlo negativo: com a espera desligada (0), a chamada falha de imediato (USER_NOT_REGISTERED);
  3. controlo negativo: com `awaiting:false`, falha de imediato — não fica presa à espera de nada;
  4. se o aparelho nunca acorda, a chamada acaba ao fim do limite com NO_USER_RESPONSE, não antes nem depois.

O «wake» é um servidor de papel nesta máquina: o endpoint real e os fornecedores de push (FCM, APNs) ainda
não existem. Isto prova o lado do FreeSWITCH. Não prova push, nem iPhone, nem o toque no aparelho.

A palavra-passe do ramal de prova nunca é impressa. Cria (se faltarem) os ramais da empresa 1901 e 1902.
"""
import argparse, hashlib, json, os, re, secrets, socket, ssl, subprocess, sys, threading, time, urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

falhas = 0
def linha(ok, texto, det=""):
    global falhas
    falhas += 0 if ok else 1
    print(f"{'PASS' if ok else 'FAIL'}  {texto}" + (f"  [{det}]" if det else ""), flush=True)

class Lab:
    def __init__(self, d):
        self.d = Path(d)
        env = dict(l.split("=", 1) for l in (self.d / ".env").read_text().splitlines() if "=" in l and not l.startswith("#"))
        self.senha, self.esl = env["MEET_ADMIN_PASSWORD"], env["TELEPHONY_ESL_PASSWORD"]
        lan = (self.d / "deploy/compose/generated/lan.yaml").read_text()
        self.base = re.search(r"CORS_ORIGINS:\s*(https://[^,\s]+)", lan).group(1)
        self.ip = re.search(r"https://([\d.]+):", self.base).group(1)
        self.ca = str(self.d / "deploy/compose/generated/lan-tls/ca.crt")
        self.ctx = ssl.create_default_context(cafile=self.ca)
        self.tok = None
    def api(self, m, p, b=None, bruto=False):
        r = urllib.request.Request(self.base + p, method=m, data=json.dumps(b).encode() if b is not None else None,
            headers={"content-type": "application/json", **({"authorization": "Bearer " + self.tok} if self.tok else {})})
        d = urllib.request.urlopen(r, context=self.ctx, timeout=15).read().decode()
        return d if bruto else json.loads(d or "null")
    def entrar(self):
        self.tok = self.api("POST", "/api/auth/login", {"email": "admin@ngolacloud.local", "password": self.senha})["access_token"]
        self.org = next(o for o in self.api("GET", "/api/orgs") if o["name"] == "ngolacloud")
        self.dominio = self.org["slug"] + ".ramais.delonix.meet"
    def ramal(self, num, etiqueta):
        for e in self.api("GET", f"/api/orgs/{self.org['id']}/extensions"):
            if e["extension"] == num:
                return e
        return self.api("POST", f"/api/orgs/{self.org['id']}/extensions", {"extension": num, "label": etiqueta})
    def credencial(self, ext):
        """Gasta um bilhete (troca a palavra-passe do ramal de PROVA) e devolve utilizador e palavra-passe."""
        url = self.api("POST", f"/api/orgs/{self.org['id']}/extensions/{ext['id']}/provisioning-ticket", {})["provisioning_url"]
        xml = self.api("GET", url[len(self.base):], bruto=True)
        g = lambda n: re.search(rf'name="{n}"[^>]*>([^<]*)<', xml).group(1)
        return g("username"), g("passwd")
    def fs(self, cmd, timeout=90):
        return subprocess.run(["delonix", "container", "exec", "delonix-freeswitch", "fs_cli", "-H", "127.0.0.1", "-p", self.esl, "-x", cmd],
                              capture_output=True, text=True, timeout=timeout).stdout.strip()

class Wake(BaseHTTPRequestHandler):
    pedidos, awaiting = [], True
    def do_POST(self):
        n = int(self.headers.get("content-length", 0))
        try: corpo = json.loads(self.rfile.read(n) or b"{}")
        except ValueError: corpo = {}
        Wake.pedidos.append({"t": time.time(), "tem_segredo": "x-voice-secret" in {k.lower() for k in self.headers}, **corpo})
        r = json.dumps({"awaiting": Wake.awaiting, "devices": 1}).encode()
        self.send_response(200); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(r))); self.end_headers(); self.wfile.write(r)
    def log_message(self, *_): pass

def md5(s): return hashlib.md5(s.encode()).hexdigest()

class Aparelho:
    """Um telefone mínimo por TLS: regista-se e fica à escuta do INVITE (responde 486)."""
    def __init__(self, lab, user, senha, dominio):
        self.lab, self.user, self.senha, self.dom = lab, user, senha, dominio
        self.t_invite = None; self.t_registo = None; self.sock = None
    def _ler(self, s):
        d = b""
        while b"\r\n\r\n" not in d:
            p = s.recv(4096)
            if not p: break
            d += p
        cab, _, resto = d.partition(b"\r\n\r\n")
        m = re.search(rb"(?im)^content-length:\s*(\d+)", cab); falta = (int(m.group(1)) if m else 0) - len(resto)
        while falta > 0:
            p = s.recv(4096)
            if not p: break
            falta -= len(p); resto += p
        return cab.decode(errors="replace")
    def registar(self):
        ctx = ssl.create_default_context(cafile=self.lab.ca)
        s = ctx.wrap_socket(socket.create_connection((self.lab.ip, 5071), timeout=10), server_hostname=self.lab.ip)
        self.sock = s; ip, lp = s.getsockname()
        cid, tag, uri, ident = secrets.token_hex(8) + "@prova", secrets.token_hex(4), f"sip:{self.dom}", f"sip:{self.user}@{self.dom}"
        def pedido(cs, auth=None):
            return "\r\n".join([f"REGISTER {uri} SIP/2.0", f"Via: SIP/2.0/TLS {ip}:{lp};branch=z9hG4bK{secrets.token_hex(6)};rport", "Max-Forwards: 70",
                f'From: "prova" <{ident}>;tag={tag}', f"To: <{ident}>", f"Call-ID: {cid}", f"CSeq: {cs} REGISTER",
                f"Contact: <sip:{self.user}@{ip}:{lp};transport=tls>", "Expires: 120", *([f"Authorization: {auth}"] if auth else []), "Content-Length: 0", "", ""]).encode()
        s.sendall(pedido(1)); r1 = self._ler(s)
        p = {k.lower(): a or b for k, a, b in re.findall(r'(\w+)\s*=\s*(?:"([^"]*)"|([^\s,]+))', re.search(r"(?im)^www-authenticate:\s*(.*)$", r1).group(1))}
        cn = secrets.token_hex(8)
        resp = md5(f"{md5(f'{self.user}:{p['realm']}:{self.senha}')}:{p['nonce']}:00000001:{cn}:auth:{md5(f'REGISTER:{uri}')}")
        s.sendall(pedido(2, f'Digest username="{self.user}", realm="{p["realm"]}", nonce="{p["nonce"]}", uri="{uri}", response="{resp}", algorithm=MD5, qop=auth, nc=00000001, cnonce="{cn}"'))
        r2 = self._ler(s)
        self.t_registo = time.time()
        threading.Thread(target=self._escutar, daemon=True).start()
        return r2.startswith("SIP/2.0 200")
    def _escutar(self):
        try:
            while True:
                m = self._ler(self.sock)
                if m.startswith("INVITE") and self.t_invite is None:
                    self.t_invite = time.time()
                    h = lambda n: re.search(rf"(?im)^{n}:\s*(.*)$", m).group(1).strip()
                    via = "\r\n".join(re.findall(r"(?im)^Via:.*$", m))
                    self.sock.sendall("\r\n".join(["SIP/2.0 486 Busy Here", via, f"From: {h('From')}", f"To: {h('To')};tag={secrets.token_hex(4)}",
                        f"Call-ID: {h('Call-ID')}", f"CSeq: {h('CSeq')}", "Content-Length: 0", "", ""]).encode())
                if not m: return
        except (OSError, AttributeError):
            return
    def fechar(self):
        try: self.sock.close()
        except OSError: pass

def originar(lab, chamador_user, dominio, destino, uuid):
    """Chamada ao dialplan dos ramais como se o chamador a marcasse (loopback). Devolve (resposta, segundos)."""
    t0 = time.time()
    r = lab.fs(f"originate {{origination_uuid={uuid},sip_auth_username={chamador_user},sip_auth_realm={dominio},domain_name={dominio},sip_from_host={dominio}}}loopback/{destino}/delonix_ramais &park", timeout=120)
    return r, time.time() - t0

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--lab", required=True); a = ap.parse_args()
    lab = Lab(a.lab); lab.entrar()
    alvo, chamador = lab.ramal("1901", "Prova push (destino)"), lab.ramal("1902", "Prova push (chamador)")
    user_alvo, senha_alvo = lab.credencial(alvo)
    user_cham, _ = lab.credencial(chamador)
    srv = ThreadingHTTPServer((lab.ip, 0), Wake); porta = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    lab.fs(f"global_setvar delonix_push_wake_url http://{lab.ip}:{porta}/wake")
    desregistado = lambda: user_alvo not in lab.fs("sofia status profile internal reg")
    try:
        assert desregistado(), "o ramal de prova já está registado"
        # 2. controlo negativo: espera desligada
        lab.fs("global_setvar delonix_push_wait_secs 0"); Wake.pedidos.clear(); Wake.awaiting = True
        r, dt = originar(lab, user_cham, lab.dominio, "1901", secrets.token_hex(4).rjust(8, "0") + "-0000-0000-0000-000000000002")
        linha("USER_NOT_REGISTERED" in r and dt < 3 and not Wake.pedidos, "espera desligada (0): falha de imediato e não pede wake", f"{r} em {dt:.2f}s, wakes={len(Wake.pedidos)}")
        # 3. controlo negativo: o servidor diz que não há aparelhos acordáveis
        lab.fs("global_setvar delonix_push_wait_secs 15"); Wake.pedidos.clear(); Wake.awaiting = False
        r, dt = originar(lab, user_cham, lab.dominio, "1901", secrets.token_hex(4).rjust(8, "0") + "-0000-0000-0000-000000000003")
        linha("USER_NOT_REGISTERED" in r and dt < 3 and len(Wake.pedidos) == 1, "awaiting:false: falha de imediato, mas pediu o wake uma vez", f"{r} em {dt:.2f}s, wakes={len(Wake.pedidos)}")
        # 4. o aparelho nunca acorda: acaba ao fim do limite
        lab.fs("global_setvar delonix_push_wait_secs 4"); Wake.pedidos.clear(); Wake.awaiting = True
        r, dt = originar(lab, user_cham, lab.dominio, "1901", secrets.token_hex(4).rjust(8, "0") + "-0000-0000-0000-000000000004")
        linha("NO_USER_RESPONSE" in r and 3.5 <= dt <= 7, "sem registo: acaba ao fim do limite (4 s) com NO_USER_RESPONSE", f"{r} em {dt:.2f}s")
        # 1. o aparelho acorda tarde: o INVITE chega depois do registo
        lab.fs("global_setvar delonix_push_wait_secs 20"); Wake.pedidos.clear(); Wake.awaiting = True
        ap_ = Aparelho(lab, user_alvo, senha_alvo, lab.dominio); res = {}
        def chamada():
            res["r"], res["dt"] = originar(lab, user_cham, lab.dominio, "1901", secrets.token_hex(4).rjust(8, "0") + "-0000-0000-0000-000000000001")
            res["t_fim"] = time.time()
        t0 = time.time(); th = threading.Thread(target=chamada); th.start()
        time.sleep(5)
        ok_reg = ap_.registar()
        th.join(timeout=60)
        espera = (ap_.t_registo - t0) if ap_.t_registo else None
        linha(ok_reg and len(Wake.pedidos) == 1 and Wake.pedidos[0].get("sip_username") == user_alvo and Wake.pedidos[0]["tem_segredo"],
              "o servidor recebeu UM wake, com o ramal certo e o segredo de voz", f"wakes={len(Wake.pedidos)}")
        linha(ap_.t_invite is not None and ap_.t_invite > ap_.t_registo and (ap_.t_invite - ap_.t_registo) < 5,
              "o INVITE chegou ao aparelho DEPOIS de ele se registar", "" if ap_.t_invite is None else f"registo aos {espera:.1f}s, INVITE {1000*(ap_.t_invite-ap_.t_registo):.0f} ms depois")
        linha("USER_BUSY" in res.get("r", ""), "a chamada seguiu para o aparelho (que recusou com 486: a prova não atende)", res.get("r", "sem resposta"))
        ap_.fechar()
    finally:
        lab.fs("global_setvar delonix_push_wait_secs 0"); lab.fs("global_setvar delonix_push_wake_url ")
        srv.shutdown()
    print(f"\n{falhas} falha(s)"); sys.exit(min(falhas, 255))

if __name__ == "__main__":
    main()
