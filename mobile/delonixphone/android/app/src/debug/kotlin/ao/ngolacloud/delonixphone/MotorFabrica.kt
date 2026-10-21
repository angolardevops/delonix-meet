package ao.ngolacloud.delonixphone

import android.content.Context

object MotorFabrica {
    fun criar(contexto: Context): MotorSip = MotorLinphone(contexto.applicationContext)
}
