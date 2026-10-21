import 'package:delonixphone/src/conta/conta_sip.dart';
import 'package:delonixphone/src/conta/controlador_conta.dart';
import 'package:delonixphone/src/conta/provisionamento.dart';
import 'package:delonixphone/src/sip/registo_sip.dart';
import 'package:flutter_test/flutter_test.dart';

import 'conta_falsos.dart';

void main() {
  test('provisionar guarda a conta e fica pronta por registar', () async {
    final armazem = ArmazemMemoria();
    final c = controladorFalso(armazem: armazem);
    await c.provisionar('qualquer-coisa');
    expect(c.conta, contaDeTeste);
    expect(armazem.valor, contaDeTeste);
    expect(c.fase, FaseRegisto.parado);
  });

  test('um bilhete inválido não guarda nada e propaga a mensagem', () async {
    final armazem = ArmazemMemoria();
    final c = controladorFalso(
      armazem: armazem,
      provisionador: ProvisionadorFalso(
        erro: const ProvisionamentoInvalido('já usado'),
      ),
    );
    await expectLater(
      c.provisionar('x'),
      throwsA(isA<ProvisionamentoInvalido>()),
    );
    expect(armazem.valor, isNull);
    expect(c.fase, FaseRegisto.semConta);
  });

  test('registar: registado e falhou', () async {
    final registo = RegistoFalso();
    final c = controladorFalso(registo: registo);
    await c.provisionar('x');
    await c.registar();
    expect((c.fase, c.mensagem), (FaseRegisto.registado, 'Registado'));
    registo.resultado = const ResultadoRegisto.falhou(403, 'recusado');
    await c.registar();
    expect((c.fase, c.mensagem), (FaseRegisto.falhou, 'recusado'));
  });

  test('sem cifra: um build de release recusa uma conta UDP e não a guarda (RNF-20)', () async {
    final armazem = ArmazemMemoria();
    final c = controladorFalso(armazem: armazem, permitirSemCifra: false);
    await expectLater(
      c.provisionar('x'),
      throwsA(isA<ProvisionamentoInvalido>()),
    );
    expect(armazem.valor, isNull);
  });

  test('sem cifra: com TLS passa mesmo em release', () async {
    const tls = ContaSip(
      nomeExibicao: 'A',
      utilizador: 'u',
      palavraPasse: 'p',
      dominio: 'd',
      servidor: ServidorSip(
        anfitriao: 'h',
        porta: 5061,
        transporte: TransporteSip.tls,
      ),
    );
    final c = controladorFalso(permitirSemCifra: false);
    await c.definirManual(tls);
    expect(c.fase, FaseRegisto.parado);
  });

  test(
    'carregar recupera a conta guardada; remover desregista e apaga',
    () async {
      final armazem = ArmazemMemoria()..valor = contaDeTeste;
      final registo = RegistoFalso();
      final c = controladorFalso(armazem: armazem, registo: registo);
      await c.carregar();
      expect(c.fase, FaseRegisto.parado);
      await c.registar();
      await c.remover();
      expect(registo.chamadas, ['registar', 'desregistar']);
      expect(
        (armazem.valor, c.conta, c.fase),
        (null, null, FaseRegisto.semConta),
      );
    },
  );
}
