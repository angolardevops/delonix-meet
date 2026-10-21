import 'package:flutter/material.dart';

import '../conta/conta_sip.dart';
import '../conta/controlador_conta.dart';
import '../conta/provisionamento.dart';
import 'ecra_ler_qr.dart';

/// Três caminhos para a mesma conta, como no Linphone: ler o QR da consola do Meet, colar o
/// endereço do QR, ou parâmetros à mão (recurso). Os dois primeiros trazem a palavra-passe SIP
/// gerada pelo servidor: ninguém a digita.
class EcraConfigurarConta extends StatefulWidget {
  const EcraConfigurarConta({super.key, required this.controlador});

  final ControladorConta controlador;

  @override
  State<EcraConfigurarConta> createState() => _EcraConfigurarContaState();
}

class _EcraConfigurarContaState extends State<EcraConfigurarConta> {
  final _endereco = TextEditingController();
  final _nome = TextEditingController();
  final _utilizador = TextEditingController();
  final _palavraPasse = TextEditingController();
  final _dominio = TextEditingController();
  final _servidor = TextEditingController();
  final _formulario = GlobalKey<FormState>();
  TransporteSip _transporte = TransporteSip.tls;
  String? _erro;
  bool _ocupado = false;

  @override
  void dispose() {
    for (final c in [
      _endereco,
      _nome,
      _utilizador,
      _palavraPasse,
      _dominio,
      _servidor,
    ]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _executar(Future<void> Function() accao) async {
    setState(() {
      _erro = null;
      _ocupado = true;
    });
    try {
      await accao();
      if (mounted) Navigator.of(context).pop();
    } on ProvisionamentoInvalido catch (e) {
      if (mounted) setState(() => _erro = e.mensagem);
    } finally {
      if (mounted) setState(() => _ocupado = false);
    }
  }

  Future<void> _lerQr() async {
    final lido = await Navigator.of(context)
        .push<String>(MaterialPageRoute(builder: (_) => const EcraLerQr()));
    if (lido != null && mounted) {
      await _executar(() => widget.controlador.provisionar(lido));
    }
  }

  Future<void> _guardarManual() async {
    if (!_formulario.currentState!.validate()) return;
    final ServidorSip servidor;
    try {
      final h = _servidor.text.trim();
      final i = h.lastIndexOf(':');
      servidor = i > 0 && int.tryParse(h.substring(i + 1)) != null
          ? ServidorSip(
              anfitriao: h.substring(0, i),
              porta: int.parse(h.substring(i + 1)),
              transporte: _transporte,
            )
          : ServidorSip(
              anfitriao: h,
              porta: _transporte == TransporteSip.tls ? 5061 : 5060,
              transporte: _transporte,
            );
    } on FormatException {
      setState(() => _erro = 'Servidor inválido.');
      return;
    }
    await _executar(
      () => widget.controlador.definirManual(
        ContaSip(
          nomeExibicao: _nome.text.trim().isEmpty
              ? _utilizador.text.trim()
              : _nome.text.trim(),
          utilizador: _utilizador.text.trim(),
          palavraPasse: _palavraPasse.text,
          dominio: _dominio.text.trim(),
          servidor: servidor,
        ),
      ),
    );
  }

  String? _obrigatorio(String? v) =>
      (v == null || v.trim().isEmpty) ? 'Obrigatório' : null;

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('Configurar conta')),
    body: ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text(
          'Peça na consola do Delonix Meet o QR do seu ramal. Ler o QR troca a palavra-passe SIP do ramal: '
          'o aparelho que estava registado deixa de registar.',
        ),
        const SizedBox(height: 12),
        FilledButton.icon(
          key: const Key('ler-qr'),
          onPressed: _ocupado ? null : _lerQr,
          icon: const Icon(Icons.qr_code_scanner),
          label: const Text('Ler QR'),
        ),
        const SizedBox(height: 16),
        TextField(
          key: const Key('campo-endereco'),
          controller: _endereco,
          decoration: const InputDecoration(
            labelText: 'Ou cole o endereço do QR',
            border: OutlineInputBorder(),
          ),
          keyboardType: TextInputType.url,
          autocorrect: false,
          enableSuggestions: false,
        ),
        const SizedBox(height: 8),
        OutlinedButton(
          key: const Key('usar-endereco'),
          onPressed: _ocupado
              ? null
              : () => _executar(
                  () => widget.controlador.provisionar(_endereco.text),
                ),
          child: const Text('Usar endereço'),
        ),
        const SizedBox(height: 8),
        ExpansionTile(
          key: const Key('parametros-manuais'),
          title: const Text('Parâmetros manuais'),
          children: [
            Form(
              key: _formulario,
              child: Column(
                children: [
                  TextFormField(
                    key: const Key('m-nome'),
                    controller: _nome,
                    decoration: const InputDecoration(
                      labelText: 'Nome (opcional)',
                    ),
                  ),
                  TextFormField(
                    key: const Key('m-utilizador'),
                    controller: _utilizador,
                    validator: _obrigatorio,
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: const InputDecoration(
                      labelText: 'Utilizador SIP',
                    ),
                  ),
                  TextFormField(
                    key: const Key('m-palavra-passe'),
                    controller: _palavraPasse,
                    validator: _obrigatorio,
                    obscureText: true,
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: const InputDecoration(
                      labelText: 'Palavra-passe',
                    ),
                  ),
                  TextFormField(
                    key: const Key('m-dominio'),
                    controller: _dominio,
                    validator: _obrigatorio,
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: const InputDecoration(
                      labelText: 'Domínio (realm)',
                    ),
                  ),
                  TextFormField(
                    key: const Key('m-servidor'),
                    controller: _servidor,
                    validator: _obrigatorio,
                    autocorrect: false,
                    enableSuggestions: false,
                    keyboardType: TextInputType.url,
                    decoration: const InputDecoration(
                      labelText: 'Servidor (host:porta)',
                    ),
                  ),
                  DropdownButtonFormField<TransporteSip>(
                    key: const Key('m-transporte'),
                    initialValue: _transporte,
                    decoration: const InputDecoration(labelText: 'Transporte'),
                    items: [
                      for (final t in TransporteSip.values)
                        DropdownMenuItem(
                          value: t,
                          child: Text(t.name.toUpperCase()),
                        ),
                    ],
                    onChanged: (t) =>
                        setState(() => _transporte = t ?? _transporte),
                  ),
                  const SizedBox(height: 8),
                  const Align(
                    alignment: Alignment.centerLeft,
                    child: Text('SRTP obrigatório (SDES)'),
                  ),
                  const SizedBox(height: 8),
                  FilledButton(
                    key: const Key('m-guardar'),
                    onPressed: _ocupado ? null : _guardarManual,
                    child: const Text('Guardar'),
                  ),
                ],
              ),
            ),
          ],
        ),
        if (_erro != null)
          Padding(
            padding: const EdgeInsets.only(top: 16),
            child: Text(
              _erro!,
              key: const Key('erro-configuracao'),
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          ),
      ],
    ),
  );
}
