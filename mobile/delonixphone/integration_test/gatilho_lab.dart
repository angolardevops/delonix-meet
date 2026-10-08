import 'dart:async';
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

  /// O FreeSWITCH origina uma chamada para o ramal [utilizador]@[dominio] (sem esperar que atenda).
  static Future<void> tocar(String utilizador, String dominio) async {
    unawaited(_obter('tocar?u=$utilizador&d=$dominio'));
  }

  /// O anfitrião dá o microfone à app (`adb shell pm grant`): evita automatizar o diálogo do sistema.
  static Future<void> concederMicrofone() async {
    final cliente = HttpClient();
    try {
      final r = await (await cliente.getUrl(
        Uri.parse('http://10.0.2.2:8765/adb/microfone'),
      )).close();
      await r.drain<void>();
      if (r.statusCode != 200) {
        throw StateError('conceder microfone: HTTP ${r.statusCode}');
      }
    } finally {
      cliente.close();
    }
  }

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
