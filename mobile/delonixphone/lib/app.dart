import 'dart:async';

import 'package:flutter/material.dart';

import 'src/conta/controlador_conta.dart';
import 'src/telefonia/estado_chamada_celular.dart';
import 'src/ui/cartao_conta.dart';

/// Primeiro ecrã: só mostra o estado da chamada celular. Os textos têm de ir para o i18n
/// (pt, en, fr, es: RF-67) quando a app crescer; aqui ficam directos porque o ecrã é um andaime.
class DelonixPhoneApp extends StatelessWidget {
  const DelonixPhoneApp({
    super.key,
    required this.monitor,
    required this.controlador,
  });

  final MonitorChamadaCelular monitor;
  final ControladorConta controlador;

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'DelonixPhone',
    theme: ThemeData(
      colorSchemeSeed: const Color(0xFF0B5FFF),
      useMaterial3: true,
    ),
    home: EcraInicio(monitor: monitor, controlador: controlador),
  );
}

class EcraInicio extends StatefulWidget {
  const EcraInicio({
    super.key,
    required this.monitor,
    required this.controlador,
  });

  final MonitorChamadaCelular monitor;
  final ControladorConta controlador;

  @override
  State<EcraInicio> createState() => _EcraInicioState();
}

class _EcraInicioState extends State<EcraInicio> {
  @override
  void initState() {
    super.initState();
    widget.controlador.carregar();
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('DelonixPhone')),
    body: ListView(
      padding: const EdgeInsets.all(16),
      children: [
        CartaoConta(controlador: widget.controlador),
        const SizedBox(height: 16),
        EcraEstadoChamada(monitor: widget.monitor),
      ],
    ),
  );
}

class EcraEstadoChamada extends StatefulWidget {
  const EcraEstadoChamada({super.key, required this.monitor});

  final MonitorChamadaCelular monitor;

  @override
  State<EcraEstadoChamada> createState() => _EcraEstadoChamadaState();
}

class _EcraEstadoChamadaState extends State<EcraEstadoChamada> {
  StreamSubscription<EstadoChamadaCelular>? _subscricao;
  EstadoChamadaCelular? _estado;
  bool _semPermissao = true;

  @override
  void initState() {
    super.initState();
    widget.monitor.permissaoConcedida().then((ok) {
      if (ok && mounted) _ouvir();
    });
  }

  void _ouvir() {
    setState(() => _semPermissao = false);
    _subscricao?.cancel();
    _subscricao = widget.monitor.estados().listen(
      (e) => setState(() => _estado = e),
      onError: (Object e) {
        if (e is SemPermissaoTelefone) setState(() => _semPermissao = true);
      },
    );
  }

  Future<void> _pedir() async {
    if (await widget.monitor.pedirPermissao() && mounted) _ouvir();
  }

  @override
  void dispose() {
    _subscricao?.cancel();
    super.dispose();
  }

  static String _texto(EstadoChamadaCelular e) => switch (e) {
    EstadoChamadaCelular.repouso => 'repouso',
    EstadoChamadaCelular.aTocar => 'a tocar',
    EstadoChamadaCelular.emCurso => 'em curso',
  };

  @override
  Widget build(BuildContext context) {
    final texto = _semPermissao
        ? 'Chamada celular: sem permissão'
        : 'Chamada celular: ${_estado == null ? 'a ler…' : _texto(_estado!)}';
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              texto,
              key: const Key('estado-chamada-celular'),
              style: Theme.of(context).textTheme.titleMedium,
            ),
            if (_semPermissao) ...[
              const SizedBox(height: 12),
              FilledButton(
                key: const Key('pedir-permissao-telefone'),
                onPressed: _pedir,
                child: const Text('Permitir ler o estado das chamadas'),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
