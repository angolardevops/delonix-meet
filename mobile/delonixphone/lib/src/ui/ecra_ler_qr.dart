import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

/// Lê um QR com a câmara e devolve o texto (`Navigator.pop`). Não interpreta nada: quem chama
/// valida (`enderecoDeProvisionamento`).
class EcraLerQr extends StatefulWidget {
  const EcraLerQr({super.key});

  @override
  State<EcraLerQr> createState() => _EcraLerQrState();
}

class _EcraLerQrState extends State<EcraLerQr> {
  bool _lido = false;

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('Ler QR de provisionamento')),
    body: MobileScanner(
      onDetect: (captura) {
        if (_lido) return;
        for (final c in captura.barcodes) {
          final v = c.rawValue;
          if (v != null && v.isNotEmpty) {
            _lido = true;
            Navigator.of(context).pop(v);
            return;
          }
        }
      },
      errorBuilder: (context, erro) => Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Text(
            erro.errorCode == MobileScannerErrorCode.permissionDenied
                ? 'Sem permissão para a câmara. Cole o endereço ou use os parâmetros manuais.'
                : 'A câmara não está disponível. Cole o endereço ou use os parâmetros manuais.',
            key: const Key('qr-sem-camara'),
            textAlign: TextAlign.center,
          ),
        ),
      ),
    ),
  );
}
