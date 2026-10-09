import 'package:flutter/material.dart';

import '../conta/controlador_conta.dart';
import 'ecra_configurar_conta.dart';

/// O cartão da conta no ecrã inicial: o ramal, o estado do registo e as acções.
class CartaoConta extends StatelessWidget {
  const CartaoConta({super.key, required this.controlador});

  final ControladorConta controlador;

  static String _texto(ControladorConta c) => switch (c.fase) {
    FaseRegisto.semConta => 'Sem conta configurada',
    FaseRegisto.parado => 'Conta pronta, por registar',
    FaseRegisto.aRegistar => 'A registar…',
    FaseRegisto.registado => 'Registado',
    FaseRegisto.falhou => c.mensagem ?? 'O registo falhou',
  };

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controlador,
    builder: (context, _) {
      final conta = controlador.conta;
      return Card(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (conta != null) ...[
                Text(
                  conta.nomeExibicao,
                  key: const Key('conta-nome'),
                  style: Theme.of(context).textTheme.titleLarge,
                ),
                Text(
                  '${conta.utilizador} · ${conta.servidor.anfitriao}:${conta.servidor.porta} ${conta.servidor.transporte.name.toUpperCase()}',
                  key: const Key('conta-servidor'),
                ),
                const SizedBox(height: 8),
              ],
              Text(
                _texto(controlador),
                key: const Key('conta-estado'),
                style: Theme.of(context).textTheme.titleMedium,
              ),
              if (conta != null && !conta.servidor.transporte.cifrado)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Text(
                    'Sinalização sem cifra (só laboratório)',
                    key: const Key('conta-aviso-sem-cifra'),
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                ),
              const SizedBox(height: 12),
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  if (conta == null)
                    FilledButton(
                      key: const Key('configurar-conta'),
                      onPressed: () => Navigator.of(context).push(
                        MaterialPageRoute<void>(
                          builder: (_) =>
                              EcraConfigurarConta(controlador: controlador),
                        ),
                      ),
                      child: const Text('Configurar conta'),
                    )
                  else ...[
                    FilledButton(
                      key: const Key('registar'),
                      onPressed: controlador.fase == FaseRegisto.aRegistar
                          ? null
                          : controlador.registar,
                      child: const Text('Registar'),
                    ),
                    OutlinedButton(
                      key: const Key('remover-conta'),
                      onPressed: controlador.remover,
                      child: const Text('Remover conta'),
                    ),
                  ],
                ],
              ),
            ],
          ),
        ),
      );
    },
  );
}
