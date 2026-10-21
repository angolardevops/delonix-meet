#!/usr/bin/env python3
"""Prova do TLS no perfil dos ramais do FreeSWITCH (entrypoint, passo 9b).

Mede, contra um servidor que corre, o que o ADR-0009 pede: a sinalização cifrada, com um
certificado que o telefone consegue conferir, TLS 1.2 no mínimo, e um REGISTER com digest
que o FreeSWITCH aceita por esse canal. Cada linha é PASS ou FAIL; o código de saída é o
número de FAIL.

    SOFTPHONE_UTILIZADOR=… SOFTPHONE_PASSWORD=… SOFTPHONE_DOMINIO=… \\
      python3 -I scripts/ramais-tls-prova.py --servidor 10.3.31.15:5071 --ca ca.crt

A palavra-passe vem do AMBIENTE, nunca da linha de comandos, e nunca é impressa.

O que NÃO prova: SRTP (é de scripts/softphone-prova.sh), chamadas, nem o comportamento de um
telefone real (Linphone/iPhone); confere o certificado pelo IP/nome que lhe dás, não por DNS.
"""
import argparse
import hashlib
import warnings
import os
import re
import secrets
import socket
import ssl
import sys

falhas = 0
warnings.filterwarnings("ignore", category=DeprecationWarning)  # TLS 1.0/1.1 são deprecated: é de propósito


def linha(ok, texto, detalhe=""):
    global falhas
    falhas += 0 if ok else 1
    print(f"{'PASS' if ok else 'FAIL'}  {texto}" + (f"  [{detalhe}]" if detalhe else ""))


def contexto(ca, versao_min=None, versao_max=None):
    c = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    c.check_hostname = True
    c.verify_mode = ssl.CERT_REQUIRED
    if ca:
        c.load_verify_locations(ca)
    else:
        c.load_default_certs()
    if versao_min:
        c.minimum_version = versao_min
    if versao_max:
        c.maximum_version = versao_max
    return c


def ligar(host, porta, ctx):
    bruto = socket.create_connection((host, porta), timeout=8)
    return ctx.wrap_socket(bruto, server_hostname=host)


def md5(s):
    return hashlib.md5(s.encode()).hexdigest()


def resposta(sock):
    """Lê UMA resposta SIP (cabeçalhos até à linha vazia, mais o corpo por Content-Length)."""
    dados = b""
    while b"\r\n\r\n" not in dados:
        parte = sock.recv(4096)
        if not parte:
            break
        dados += parte
    cab, _, corpo = dados.partition(b"\r\n\r\n")
    m = re.search(rb"(?im)^content-length:\s*(\d+)", cab)
    falta = int(m.group(1)) - len(corpo) if m else 0
    while falta > 0:
        parte = sock.recv(4096)
        if not parte:
            break
        falta -= len(parte)
    return cab.decode(errors="replace")


def cabecalho(texto, nome):
    m = re.search(rf"(?im)^{nome}:\s*(.*)$", texto)
    return m.group(1).strip() if m else ""


def registar(host, porta, ctx, utilizador, senha, dominio):
    """REGISTER com digest sobre TLS. Devolve o código final (200, 403, …) e o 1.º (401)."""
    s = ligar(host, porta, ctx)
    ip, lp = s.getsockname()
    call_id, tag = secrets.token_hex(8) + "@prova-tls", secrets.token_hex(4)
    uri, ident = f"sip:{dominio}", f"sip:{utilizador}@{dominio}"

    def pedido(cseq, auth=None):
        return "\r\n".join([
            f"REGISTER {uri} SIP/2.0",
            f"Via: SIP/2.0/TLS {ip}:{lp};branch=z9hG4bK{secrets.token_hex(6)};rport",
            "Max-Forwards: 70", f'From: "prova" <{ident}>;tag={tag}', f"To: <{ident}>",
            f"Call-ID: {call_id}", f"CSeq: {cseq} REGISTER",
            f"Contact: <sip:{utilizador}@{ip}:{lp};transport=tls>", "Expires: 60",
            "User-Agent: ramais-tls-prova",
            *([f"Authorization: {auth}"] if auth else []), "Content-Length: 0", "", ""]).encode()

    try:
        s.sendall(pedido(1))
        r1 = resposta(s)
        c1 = int(re.match(r"SIP/2\.0 (\d{3})", r1).group(1))
        if c1 != 401:
            return c1, c1
        ch = cabecalho(r1, "www-authenticate")
        p = {k.lower(): a or b for k, a, b in re.findall(r'(\w+)\s*=\s*(?:"([^"]*)"|([^\s,]+))', ch)}
        qop = "auth" if "auth" in p.get("qop", "").split(",") or p.get("qop") == "auth" else None
        cn = secrets.token_hex(8)
        ha1, ha2 = md5(f"{utilizador}:{p['realm']}:{senha}"), md5(f"REGISTER:{uri}")
        resp = md5(f"{ha1}:{p['nonce']}:00000001:{cn}:auth:{ha2}") if qop else md5(f"{ha1}:{p['nonce']}:{ha2}")
        auth = (f'Digest username="{utilizador}", realm="{p["realm"]}", nonce="{p["nonce"]}", uri="{uri}", '
                f'response="{resp}", algorithm=MD5' + (f', qop=auth, nc=00000001, cnonce="{cn}"' if qop else ""))
        s.sendall(pedido(2, auth))
        r2 = resposta(s)
        # desregista: não deixar a prova registada
        if r2.startswith("SIP/2.0 200"):
            s.sendall(pedido(3, auth).replace(b"Expires: 60", b"Expires: 0"))
        return int(re.match(r"SIP/2\.0 (\d{3})", r2).group(1)), c1
    finally:
        s.close()


def main():
    a = argparse.ArgumentParser()
    a.add_argument("--servidor", required=True, help="host:porta TLS dos ramais")
    a.add_argument("--ca", help="raiz que assina o certificado (PEM); sem ela, só as do sistema")
    a.add_argument("--dominio", default=os.environ.get("SOFTPHONE_DOMINIO"))
    a = a.parse_args()
    host, _, porta = a.servidor.rpartition(":")
    porta = int(porta)
    user, senha = os.environ.get("SOFTPHONE_UTILIZADOR"), os.environ.get("SOFTPHONE_PASSWORD")
    if not (user and senha and a.dominio):
        sys.exit("faltam SOFTPHONE_UTILIZADOR, SOFTPHONE_PASSWORD e SOFTPHONE_DOMINIO")

    # 1. Certificado: conferido pela raiz, e pelo nome/IP a que o telefone se liga.
    base = False
    try:
        s = ligar(host, porta, contexto(a.ca, ssl.TLSVersion.TLSv1_2))
        linha(True, f"handshake com certificado conferido ({s.version()}, {s.cipher()[0]})")
        s.close()
        base = True
    except (ssl.SSLError, OSError) as e:
        linha(False, "handshake com certificado conferido", type(e).__name__)

    # 2. Sem a raiz de laboratório, o certificado NÃO é de confiança (controlo negativo).
    if a.ca:
        try:
            ligar(host, porta, contexto(None)).close()
            linha(False, "sem a raiz, o certificado é recusado", "foi aceite!")
        except ssl.SSLCertVerificationError:
            linha(True, "sem a raiz, o certificado é recusado")
        except (ssl.SSLError, OSError) as e:
            linha(False, "sem a raiz, o certificado é recusado", type(e).__name__)

    # 3. TLS abaixo de 1.2 recusado (RNF-20). Só vale se o handshake de base funciona: numa porta
    #    que nem fala TLS, «recusado» seria verdade por razões erradas.
    for nome, v in (("1.0", ssl.TLSVersion.TLSv1), ("1.1", ssl.TLSVersion.TLSv1_1)):
        try:
            c = contexto(a.ca)
            c.minimum_version, c.maximum_version = v, v
            c.set_ciphers("ALL:@SECLEVEL=0")
            ligar(host, porta, c).close()
            linha(False, f"TLS {nome} recusado", "foi aceite!")
        except (ssl.SSLError, OSError) as e:
            linha(base, f"TLS {nome} recusado", "" if base else f"sem handshake de base ({type(e).__name__})")

    # 4. Texto em claro na porta TLS não é SIP: a ligação é fechada, sem resposta SIP.
    try:
        b = socket.create_connection((host, porta), timeout=8)
        b.sendall(b"OPTIONS sip:x SIP/2.0\r\nCall-ID: claro\r\nCSeq: 1 OPTIONS\r\nContent-Length: 0\r\n\r\n")
        b.settimeout(4)
        try:
            d = b.recv(512)
        except (socket.timeout, ConnectionError):
            d = b""
        linha(base and not d.startswith(b"SIP/2.0"), "SIP em claro na porta TLS não é respondido",
              "" if base else "sem handshake de base")
        b.close()
    except OSError as e:
        linha(base, "SIP em claro na porta TLS não é respondido", type(e).__name__)

    # 5. REGISTER com digest por TLS: 401 → 200. E a palavra-passe errada → 403.
    ctx = contexto(a.ca, ssl.TLSVersion.TLSv1_2)
    try:
        final, primeiro = registar(host, porta, ctx, user, senha, a.dominio)
        linha(primeiro == 401 and final == 200, "REGISTER com digest por TLS (401 → 200)", f"{primeiro} → {final}")
        final, _ = registar(host, porta, ctx, user, senha + "x", a.dominio)
        linha(final in (401, 403), "palavra-passe errada é recusada por TLS", str(final))
    except (ssl.SSLError, OSError, AttributeError, KeyError) as e:
        linha(False, "REGISTER por TLS", type(e).__name__)

    print(f"\n{falhas} falha(s)")
    sys.exit(min(falhas, 255))


if __name__ == "__main__":
    main()
