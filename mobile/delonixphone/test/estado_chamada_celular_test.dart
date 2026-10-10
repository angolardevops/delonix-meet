import 'dart:async';

import 'package:delonixphone/app.dart';

import 'conta_falsos.dart';

import 'package:delonixphone/src/telefonia/estado_chamada_celular.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

class _Falso implements MonitorChamadaCelular {
  _Falso({this.concedida = false});
  bool concedida;
  final controlo = StreamController<EstadoChamadaCelular>.broadcast();

  @override
  Future<bool> permissaoConcedida() async => concedida;
  @override
  Future<bool> pedirPermissao() async => concedida = true;
  @override
  Stream<EstadoChamadaCelular> estados() => controlo.stream;
}

String _estado(WidgetTester t) =>
    t.widget<Text>(find.byKey(const Key('estado-chamada-celular'))).data!;

/// O evento do `StreamController` chega num microtask: sem o 2.º `pump` a UI ainda não o viu.
Future<void> _emitir(WidgetTester t, _Falso m, EstadoChamadaCelular e) async {
  m.controlo.add(e);
  await t.pump();
  await t.pump();
}

void main() {
  group('EstadoChamadaCelular.doNome', () {
    test('traduz os três estados', () {
      expect(
        EstadoChamadaCelular.doNome('repouso'),
        EstadoChamadaCelular.repouso,
      );
      expect(
        EstadoChamadaCelular.doNome('a_tocar'),
        EstadoChamadaCelular.aTocar,
      );
      expect(
        EstadoChamadaCelular.doNome('em_curso'),
        EstadoChamadaCelular.emCurso,
      );
    });

    test('um estado desconhecido falha em vez de virar «repouso»', () {
      expect(
        () => EstadoChamadaCelular.doNome('conferencia'),
        throwsFormatException,
      );
    });
  });

  group('EcraEstadoChamada', () {
    testWidgets('sem permissão pede-a e depois mostra o estado', (t) async {
      final monitor = _Falso();
      await t.pumpWidget(
        DelonixPhoneApp(monitor: monitor, controlador: controladorFalso()),
      );
      await t.pump();
      expect(_estado(t), 'Chamada celular: sem permissão');

      await t.tap(find.byKey(const Key('pedir-permissao-telefone')));
      await t.pump();
      await _emitir(t, monitor, EstadoChamadaCelular.aTocar);
      expect(_estado(t), 'Chamada celular: a tocar');
      expect(find.byKey(const Key('pedir-permissao-telefone')), findsNothing);
    });

    testWidgets('com permissão já dada segue as mudanças', (t) async {
      final monitor = _Falso(concedida: true);
      await t.pumpWidget(
        DelonixPhoneApp(monitor: monitor, controlador: controladorFalso()),
      );
      await t.pump();
      await _emitir(t, monitor, EstadoChamadaCelular.emCurso);
      expect(_estado(t), 'Chamada celular: em curso');
      await _emitir(t, monitor, EstadoChamadaCelular.repouso);
      expect(_estado(t), 'Chamada celular: repouso');
    });
  });
}
