package ao.ngolacloud.delonixphone

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import ao.ngolacloud.push.android.DelonixPush

/**
 * SÓ em debug: configura e arranca o push por `adb shell am broadcast` (a prova no emulador não tem o Meet
 * pelo meio). Exportado para o `adb`; um build de release não o contém.
 */
class PushConfigReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val url = intent.getStringExtra("url") ?: return
        val segredo = intent.getStringExtra("segredo") ?: return
        DelonixPush.configure(context, url, segredo)
        DelonixPush.start(context)
    }
}
