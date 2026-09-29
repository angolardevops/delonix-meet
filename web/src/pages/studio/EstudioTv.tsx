/**
 * O estúdio de TV (Navegavel5): mesa de corte, mesa de som, iluminação,
 * fontes e cena completa, sobre o compositor do Estúdio.
 *
 * Entra por `lazy()` na primeira visita a `#/studio?vista=tv` e FICA MONTADO
 * enquanto o Estúdio estiver aberto: a sessão (câmaras ligadas, mesa de som,
 * programa no ar) não pode morrer por se voltar ao palco do Estúdio a meio de
 * uma emissão. Com `ecra = null` não desenha nada — o canvas do programa
 * volta ao palco — mas continua a mandar no som e no programa.
 */
import { useTranslation } from 'react-i18next'
import '../../ui/studio-tv.css'
import CenaCompleta from './tv/CenaCompleta'
import type { ContextoDoEstudio } from './tv/comum'
import Fontes from './tv/Fontes'
import Iluminacao from './tv/Iluminacao'
import MesaDeCorte from './tv/MesaDeCorte'
import MesaDeSomEcra from './tv/MesaDeSomEcra'
import type { EcraTv } from '../../studio/tv/ecras'
import { useSessaoTv } from './tv/useSessaoTv'

export default function EstudioTv({ ecra, ...c }: ContextoDoEstudio & { ecra: EcraTv | null }) {
  useTranslation()
  const s = useSessaoTv({
    compRef: c.compRef,
    palco: c.palco,
    temEcra: c.temEcra,
    participantes: c.participantes,
    haSondagem: c.haSondagem,
    gravando: c.gravacao.estado !== 'parado',
    noAr: c.directo.fase === 'no-ar',
    onPararGravacao: c.onPararGravacao,
    onTerminarEmissao: c.onTerminarEmissao,
    atalhosActivos: ecra === 'mesa-de-corte' || ecra === 'cena',
  })
  switch (ecra) {
    case 'mesa-de-corte':
      return <MesaDeCorte s={s} c={c} />
    case 'mesa-de-som':
      return <MesaDeSomEcra s={s} c={c} />
    case 'iluminacao':
      return <Iluminacao s={s} c={c} />
    case 'fontes':
      return <Fontes s={s} c={c} />
    case 'cena':
      return <CenaCompleta s={s} c={c} />
    default:
      return null
  }
}
