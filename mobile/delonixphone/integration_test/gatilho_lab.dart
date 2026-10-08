import 'dart:convert';
import 'dart:io';

/// Pede ao anfitrião (ambiente/gatilho-gsm.py) um bilhete ou as credenciais do ramal de teste do
/// laboratório. Só o anfitrião tem a conta de administração; o aparelho recebe o que precisa.
class GatilhoLab {
  static const _base = 'http://10.0.2.2:8765/lab';

  /// Um URL de provisionamento fresco (o que a consola poria num QR).
  static Future<String> bilhete() async =>
      (await _obter('bilhete'))['url'] as String;

  /// Um URL que já foi resgatado: o servidor tem de o recusar.
  static Future<String> bilheteUsado() async =>
      (await _obter('bilhete-usado'))['url'] as String;

  /// Gasta um bilhete e devolve utilizador, palavraPasse, dominio e servidor (host:porta).
  static Future<Map<String, dynamic>> credenciais() => _obter('credenciais');

  static Future<Map<String, dynamic>> _obter(String rota) async {
    final cliente = HttpClient();
    try {
      final resposta = await (await cliente.getUrl(Uri.parse('$_base/$rota')))
          .close();
      final corpo = await resposta.transform(utf8.decoder).join();
      if (resposta.statusCode != 200) {
        throw StateError(
          'gatilho do laboratório "$rota": HTTP ${resposta.statusCode} — $corpo',
        );
      }
      return jsonDecode(corpo) as Map<String, dynamic>;
    } finally {
      cliente.close();
    }
  }
}
