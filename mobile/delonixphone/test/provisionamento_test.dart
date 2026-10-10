import 'dart:io';

import 'package:delonixphone/src/conta/conta_sip.dart';
import 'package:delonixphone/src/conta/provisionamento.dart';
import 'package:flutter_test/flutter_test.dart';

final _bilhete = 'ab' * 32;

void main() {
  group('enderecoDeProvisionamento', () {
    test('aceita o URL que a consola do Meet põe no QR', () {
      final u = enderecoDeProvisionamento(
        '  https://10.3.31.15:8443/api/public/extension-provisioning/$_bilhete ',
      );
      expect(u.host, '10.3.31.15');
      expect(u.port, 8443);
    });

    for (final (nome, texto) in [
      (
        'http em claro (a configuração leva a palavra-passe)',
        'http://meet.exemplo.ao/api/public/extension-provisioning/${'ab' * 32}',
      ),
      ('outro caminho', 'https://meet.exemplo.ao/api/orgs/x'),
      (
        'bilhete curto',
        'https://meet.exemplo.ao/api/public/extension-provisioning/abc',
      ),
      (
        'bilhete em maiúsculas',
        'https://meet.exemplo.ao/api/public/extension-provisioning/${'AB' * 32}',
      ),
      (
        'com query',
        'https://meet.exemplo.ao/api/public/extension-provisioning/${'ab' * 32}?x=1',
      ),
      (
        'com credenciais no URL',
        'https://u:p@meet.exemplo.ao/api/public/extension-provisioning/${'ab' * 32}',
      ),
      (
        'prefixo no meio do caminho',
        'https://meet.exemplo.ao/x/api/public/extension-provisioning/${'ab' * 32}',
      ),
      ('texto qualquer', 'sip:101@exemplo'),
      ('vazio', ''),
    ]) {
      test(
        'recusa: $nome',
        () => expect(
          () => enderecoDeProvisionamento(texto),
          throwsA(isA<ProvisionamentoInvalido>()),
        ),
      );
    }
  });

  group('contaDeLpconfig (XML real gerado pelo servidor)', () {
    final xml = File('test/fixtures/lpconfig_real.xml').readAsStringSync();

    test('lê utilizador, palavra-passe (com entidade XML), domínio, servidor e nome', () {
      final c = contaDeLpconfig(xml);
      expect(c.utilizador, 'ramal_f23825e2c1b89a96');
      expect(c.palavraPasse, 'segredo-de-teste&1');
      expect(c.dominio, 'ngolacloud-ngolacloud-local.ramais.delonix.meet');
      expect(
        c.servidor,
        const ServidorSip(
          anfitriao: '10.3.31.15',
          porta: 5070,
          transporte: TransporteSip.udp,
        ),
      );
      expect(c.nomeExibicao, 'admin');
      expect(c.srtpObrigatorio, isTrue);
    });

    test('a palavra-passe nunca sai no toString', () {
      expect(contaDeLpconfig(xml).toString(), isNot(contains('segredo')));
    });

    test('sem nome na identidade, usa o utilizador', () {
      final c = contaDeLpconfig(xml.replaceAll(RegExp(r'"admin" '), ''));
      expect(c.nomeExibicao, c.utilizador);
    });

    test('falta a palavra-passe: erro claro, não conta vazia', () {
      final sem = xml.replaceAll(
        RegExp(r'<entry name="passwd"[^>]*>[^<]*</entry>'),
        '',
      );
      expect(
        () => contaDeLpconfig(sem),
        throwsA(isA<ProvisionamentoInvalido>()),
      );
    });

    test(
      'XML estragado',
      () => expect(
        () => contaDeLpconfig('<config><section'),
        throwsA(isA<ProvisionamentoInvalido>()),
      ),
    );
    test('não resolve entidades externas (XXE)', () {
      const mau =
          '<?xml version="1.0"?><!DOCTYPE c [<!ENTITY x SYSTEM "file:///etc/passwd">]><config><section name="auth_info_0"><entry name="username">&x;</entry></section></config>';
      expect(
        () => contaDeLpconfig(mau),
        throwsA(isA<ProvisionamentoInvalido>()),
      );
    });
  });

  group('ServidorSip.doUri', () {
    test('com e sem <>', () {
      expect(
        ServidorSip.doUri('<sip:a.b:5070;transport=tls>'),
        const ServidorSip(
          anfitriao: 'a.b',
          porta: 5070,
          transporte: TransporteSip.tls,
        ),
      );
      expect(ServidorSip.doUri('sip:a.b').porta, 5060);
      expect(ServidorSip.doUri('sips:a.b').porta, 5061);
    });
    test('recusa lixo e portas absurdas', () {
      expect(() => ServidorSip.doUri('a.b:5070'), throwsFormatException);
      expect(() => ServidorSip.doUri('sip:a.b:99999'), throwsFormatException);
    });
  });
}
