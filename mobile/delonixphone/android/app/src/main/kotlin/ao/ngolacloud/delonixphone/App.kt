package ao.ngolacloud.delonixphone

import android.app.Application
import ao.ngolacloud.push.android.DelonixPush

/**
 * O serviço do delonix-push pode nascer sem a Activity (reinício, ou o sistema a recriar o processo): o
 * tratamento das mensagens tem de estar definido aqui, e não numa Activity.
 */
class App : Application() {
    override fun onCreate() {
        super.onCreate()
        DelonixPush.notificationTitle = "DelonixPhone: pronto para receber chamadas"
        DelonixPush.handler = ChamadaPorPush
    }
}
