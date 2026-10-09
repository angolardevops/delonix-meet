import 'package:flutter/material.dart';

import '../chamadas/servico_chamadas.dart';
import '../sip/sip_engine.dart';

/// O motor SIP e a chamada em curso: estado do registo e os botões de atender, recusar e terminar.
class CartaoChamada extends StatelessWidget {
  const CartaoChamada({super.key, required this.servico});

  final ServicoChamadas servico;

  static String _registo(ServicoChamadas s) {
    if (s.motorIndisponivel) return 'Motor SIP: indisponível neste build';
    if (s.erro != null) return s.erro!;
    if (!s.iniciado) return 'Motor SIP: parado';
    return switch (s.registo) {
      EstadoRegistoMotor.registado => 'Motor SIP: registado',
      EstadoRegistoMotor.aRegistar ||
      EstadoRegistoMotor.aRenovar => 'Motor SIP: a registar…',
      EstadoRegistoMotor.falhou => 'Motor SIP: o registo falhou',
      _ => 'Motor SIP: sem registo',
    };
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: servico,
    builder: (context, _) => Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(_registo(servico), key: const Key('motor-registo')),
            if (servico.fase == FaseChamada.aEntrar) ...[
              const SizedBox(height: 12),
              Text(
                'Chamada a entrar',
                key: const Key('chamada-a-entrar'),
                style: Theme.of(context).textTheme.titleLarge,
              ),
              const SizedBox(height: 8),
              Wrap(
                spacing: 8,
                children: [
                  FilledButton(
                    key: const Key('atender'),
                    onPressed: servico.atender,
                    child: const Text('Atender'),
                  ),
                  OutlinedButton(
                    key: const Key('recusar'),
                    onPressed: servico.recusar,
                    child: const Text('Recusar'),
                  ),
                ],
              ),
            ],
            if (servico.fase == FaseChamada.emCurso ||
                servico.fase == FaseChamada.aSair) ...[
              const SizedBox(height: 12),
              Text(
                servico.fase == FaseChamada.emCurso ? 'Em chamada' : 'A ligar…',
                key: const Key('chamada-em-curso'),
                style: Theme.of(context).textTheme.titleLarge,
              ),
              const SizedBox(height: 8),
              FilledButton(
                key: const Key('terminar'),
                onPressed: servico.terminar,
                child: const Text('Terminar'),
              ),
            ],
          ],
        ),
      ),
    ),
  );
}
