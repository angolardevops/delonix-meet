# Servidor de autenticação de BRINQUEDO para o spike (não é código de produção).
import json, sys, time
from http.server import BaseHTTPRequestHandler, HTTPServer
KEYFILE = sys.argv[1]; LOG = sys.argv[2]
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get('Content-Length', 0))) or b'{}')
        key = open(KEYFILE).read().strip()
        # publicar exige a chave corrente; ler (hls/webrtc/rtmp) é livre neste spike
        ok = body.get('action') != 'publish' or body.get('password') == key
        # O corpo traz a credencial em `password`, `token` E `query`: nenhum deles vai para o log.
        red = {k: v for k, v in body.items() if k not in ('password', 'token', 'query')}
        red['has_credential'] = bool(body.get('password') or body.get('token'))
        with open(LOG, 'a') as f:
            f.write(json.dumps({'t': round(time.time(), 3), 'allow': ok, **red}) + '\n')
        self.send_response(200 if ok else 401); self.end_headers()
HTTPServer(('127.0.0.1', 9099), H).serve_forever()
