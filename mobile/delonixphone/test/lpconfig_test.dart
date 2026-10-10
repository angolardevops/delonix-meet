import 'package:delonixphone/src/conta/conta_sip.dart';
import 'package:delonixphone/src/conta/provisionamento.dart';
import 'package:delonixphone/src/sip/lpconfig.dart';
import 'package:flutter_test/flutter_test.dart';

ContaSip _conta({
  String nome = 'Ana',
  String senha = 'segredo',
  TransporteSip t = TransporteSip.tls,
}) => ContaSip(
  nomeExibicao: nome,
  utilizador: 'ramal_x',
  palavraPasse: senha,
  dominio: 'org.ramais.delonix.meet',
  servidor: ServidorSip(anfitriao: '10.0.0.5', porta: 5071, transporte: t),
);

void main() {
  test('dá a volta: o que o motor carrega é a mesma conta que o QR dava', () {
    final c = _conta();
    final volta = contaDeLpconfig(lpconfigDeConta(c));
    expect(volta.utilizador, c.utilizador);
    expect(volta.palavraPasse, c.palavraPasse);
    expect(volta.dominio, c.dominio);
    expect(volta.servidor, c.servidor);
    expect(volta.nomeExibicao, 'Ana');
    expect(volta.srtpObrigatorio, isTrue);
  });

  test('uma palavra-passe com &, < e aspas não parte o XML', () {
    const senha = 'a&b<c>"d\'e';
    expect(
      contaDeLpconfig(lpconfigDeConta(_conta(senha: senha))).palavraPasse,
      senha,
    );
  });

  test('o nome não consegue sair das aspas do cabeçalho SIP', () {
    final xml = lpconfigDeConta(_conta(nome: 'Ana" <sip:x@y>\n'));
    final volta = contaDeLpconfig(xml);
    expect(volta.nomeExibicao, isNot(contains('"')));
    expect(volta.nomeExibicao, isNot(contains('<')));
    expect(volta.utilizador, 'ramal_x');
  });

  test('o transporte e a porta vão no proxy', () {
    final xml = lpconfigDeConta(_conta(t: TransporteSip.tls));
    expect(xml, contains('sip:10.0.0.5:5071;transport=tls'));
  });
}
