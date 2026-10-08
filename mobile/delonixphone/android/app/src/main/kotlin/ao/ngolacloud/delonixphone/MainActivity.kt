package ao.ngolacloud.delonixphone

import android.Manifest
import android.content.pm.PackageManager
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    private var pedidoPermissao: MethodChannel.Result? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val mensageiro = flutterEngine.dartExecutor.binaryMessenger
        EventChannel(mensageiro, EstadoChamadaCelular.CANAL_ESTADOS)
            .setStreamHandler(EstadoChamadaCelular(this))
        MethodChannel(mensageiro, CANAL_PERMISSOES).setMethodCallHandler { chamada, resultado ->
            when (chamada.method) {
                "telefoneConcedida" -> resultado.success(telefoneConcedida())
                "pedirTelefone" -> {
                    if (telefoneConcedida()) {
                        resultado.success(true)
                    } else {
                        pedidoPermissao = resultado
                        requestPermissions(arrayOf(Manifest.permission.READ_PHONE_STATE), PEDIDO_TELEFONE)
                    }
                }
                else -> resultado.notImplemented()
            }
        }
    }

    private fun telefoneConcedida() =
        checkSelfPermission(Manifest.permission.READ_PHONE_STATE) == PackageManager.PERMISSION_GRANTED

    override fun onRequestPermissionsResult(codigo: Int, permissoes: Array<out String>, resultados: IntArray) {
        super.onRequestPermissionsResult(codigo, permissoes, resultados)
        if (codigo == PEDIDO_TELEFONE) {
            pedidoPermissao?.success(telefoneConcedida())
            pedidoPermissao = null
        }
    }

    companion object {
        const val CANAL_PERMISSOES = "ao.ngolacloud.delonixphone/permissoes"
        private const val PEDIDO_TELEFONE = 4401
    }
}
