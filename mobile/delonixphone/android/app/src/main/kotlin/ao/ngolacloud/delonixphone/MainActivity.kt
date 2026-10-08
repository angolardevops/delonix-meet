package ao.ngolacloud.delonixphone

import android.Manifest
import android.content.pm.PackageManager
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    // Um pedido de permissão pendente por código: o resultado volta ao Dart que o pediu.
    private val pedidos = mutableMapOf<Int, MethodChannel.Result>()

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val mensageiro = flutterEngine.dartExecutor.binaryMessenger
        EventChannel(mensageiro, EstadoChamadaCelular.CANAL_ESTADOS)
            .setStreamHandler(EstadoChamadaCelular(this))
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
        private const val PEDIDO_TELEFONE = 4401
        private const val PEDIDO_MICROFONE = 4402
    }
}
