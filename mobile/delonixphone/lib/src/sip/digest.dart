import 'package:crypto/crypto.dart';

String _md5(String s) => md5.convert(s.codeUnits).toString();

/// A resposta do SIP Digest (RFC 2617/3261, MD5). Com `qop=auth`, entram `nc` e `cnonce`.
String respostaDigest({
  required String utilizador,
  required String realm,
  required String palavraPasse,
  required String metodo,
  required String uri,
  required String nonce,
  String? qop,
  String nc = '00000001',
  String cnonce = '',
}) {
  final ha1 = _md5('$utilizador:$realm:$palavraPasse');
  final ha2 = _md5('$metodo:$uri');
  return qop == null
      ? _md5('$ha1:$nonce:$ha2')
      : _md5('$ha1:$nonce:$nc:$cnonce:$qop:$ha2');
}

/// O desafio de um `WWW-Authenticate: Digest …`.
class DesafioDigest {
  const DesafioDigest({
    required this.realm,
    required this.nonce,
    this.opaque,
    this.qop,
    this.algoritmo = 'MD5',
  });

  final String realm;
  final String nonce;
  final String? opaque;
  final String? qop;
  final String algoritmo;

  /// `null` se não é um desafio Digest com `realm` e `nonce`.
  static DesafioDigest? ler(String cabecalho) {
    if (!cabecalho.trimLeft().toLowerCase().startsWith('digest')) return null;
    final p = <String, String>{};
    for (final m in RegExp(
      r'(\w+)\s*=\s*(?:"([^"]*)"|([^\s,]+))',
    ).allMatches(cabecalho)) {
      p[m.group(1)!.toLowerCase()] = m.group(2) ?? m.group(3)!;
    }
    final realm = p['realm'], nonce = p['nonce'];
    if (realm == null || nonce == null) return null;
    // `qop="auth,auth-int"`: só usamos `auth`.
    final qop =
        p['qop']?.split(',').map((e) => e.trim()).contains('auth') == true
        ? 'auth'
        : null;
    return DesafioDigest(
      realm: realm,
      nonce: nonce,
      opaque: p['opaque'],
      qop: qop,
      algoritmo: (p['algorithm'] ?? 'MD5').toUpperCase(),
    );
  }
}
