package ao.ngolacloud.delonixphone

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    // Um pedido de permissão pendente por código: o resultado volta ao Dart que o pediu.
    private val pedidos = mutableMapOf<Int, MethodChannel.Result>()

    // Os extras `dlx_*` que um intent traz (acordar a app, configuração de laboratório). Guardam-se até o
    // Dart os pedir, ou entregam-se logo se já escuta. Só cadeias, e só com este prefixo: nada mais passa.
    // QUALQUER app pode mandar um intent a esta actividade: o Dart só actua sobre a configuração em
    // builds de debug, e o resto só arranca o motor com a conta que a app já tem.
    private val extrasPendentes = mutableMapOf<String, String>()
    private var intencaoSink: EventChannel.EventSink? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        capturar(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        capturar(intent)
    }

    private fun capturar(i: Intent?) {
        val extras = i?.extras ?: return
        for (chave in extras.keySet()) {
            if (!chave.startsWith("dlx_")) continue
            i.getStringExtra(chave)?.take(2048)?.let { extrasPendentes[chave] = it }
        }
        val sink = intencaoSink
        if (sink != null && extrasPendentes.isNotEmpty()) {
            sink.success(HashMap(extrasPendentes))
            extrasPendentes.clear()
        }
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val mensageiro = flutterEngine.dartExecutor.binaryMessenger
        EventChannel(mensageiro, EstadoChamadaCelular.CANAL_ESTADOS)
            .setStreamHandler(EstadoChamadaCelular(this))
        MethodChannel(mensageiro, CANAL_INTENCAO).setMethodCallHandler { chamada, resultado ->
            if (chamada.method == "extras") {
                resultado.success(HashMap(extrasPendentes))
                extrasPendentes.clear()
            } else {
                resultado.notImplemented()
            }
        }
        EventChannel(mensageiro, CANAL_INTENCAO_EVENTOS).setStreamHandler(
            object : EventChannel.StreamHandler {
                override fun onListen(argumentos: Any?, eventos: EventChannel.EventSink) {
                    intencaoSink = eventos
                }

                override fun onCancel(argumentos: Any?) {
                    intencaoSink = null
                }
            },
        )
        val motor = MotorFabrica.criar(this)
        MethodChannel(mensageiro, MotorSip.CANAL).setMethodCallHandler(motor)
        EventChannel(mensageiro, MotorSip.CANAL_EVENTOS).setStreamHandler(motor)
        MethodChannel(mensageiro, CANAL_PERMISSOES).setMethodCallHandler { chamada, resultado ->
            when (chamada.method) {
                "telefoneConcedida" -> resultado.success(concedida(Manifest.permission.READ_PHONE_STATE))
                "pedirTelefone" -> pedir(Manifest.permission.READ_PHONE_STATE, PEDIDO_TELEFONE, resultado)
                "microfoneConcedida" -> resultado.success(concedida(Manifest.permission.RECORD_AUDIO))
                "pedirMicrofone" -> pedir(Manifest.permission.RECORD_AUDIO, PEDIDO_MICROFONE, resultado)
                else -> resultado.notImplemented()
            }
        }
    }

    private fun concedida(permissao: String) =
        checkSelfPermission(permissao) == PackageManager.PERMISSION_GRANTED

    private fun pedir(permissao: String, codigo: Int, resultado: MethodChannel.Result) {
        if (concedida(permissao)) {
            resultado.success(true)
        } else {
            pedidos[codigo] = resultado
            requestPermissions(arrayOf(permissao), codigo)
        }
    }

    override fun onRequestPermissionsResult(codigo: Int, permissoes: Array<out String>, resultados: IntArray) {
        super.onRequestPermissionsResult(codigo, permissoes, resultados)
        pedidos.remove(codigo)?.success(resultados.isNotEmpty() && resultados[0] == PackageManager.PERMISSION_GRANTED)
    }

    companion object {
        const val CANAL_PERMISSOES = "ao.ngolacloud.delonixphone/permissoes"
        const val CANAL_INTENCAO = "ao.ngolacloud.delonixphone/intencao"
        const val CANAL_INTENCAO_EVENTOS = "ao.ngolacloud.delonixphone/intencao_eventos"
        private const val PEDIDO_TELEFONE = 4401
        private const val PEDIDO_MICROFONE = 4402
    }
}
