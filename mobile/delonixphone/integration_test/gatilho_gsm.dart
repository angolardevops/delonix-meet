import 'dart:io';

/// Pede ao anfitrião (ambiente/gatilho-gsm.py) uma chamada celular ao emulador.
/// 10.0.2.2 é o anfitrião visto de dentro do emulador.
class GatilhoGsm {
  static const _base = 'http://10.0.2.2:8765/gsm';

  static Future<void> ligar() => _pedir('call');
  static Future<void> atender() => _pedir('accept');
  static Future<void> cancelar() => _pedir('cancel');

  static Future<void> _pedir(String accao) async {
    final cliente = HttpClient();
    try {
      final resposta = await (await cliente.getUrl(Uri.parse('$_base/$accao')))
          .close();
      if (resposta.statusCode != 200) {
        throw StateError(
          'gatilho GSM "$accao" falhou: HTTP ${resposta.statusCode}',
        );
      }
    } finally {
      cliente.close();
    }
  }
}
