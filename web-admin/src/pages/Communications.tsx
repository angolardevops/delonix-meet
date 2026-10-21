/**
 * Comunicações — placeholder explícito. A administração de FreeSWITCH,
 * Kamailio e dos PBX de cliente é uma fase posterior do backoffice: esta
 * secção existe no rail para que a estrutura das cinco áreas já esteja
 * certa, mas não finge dados nem formulários que ainda não têm rota de
 * operador por trás.
 */
import { useTranslation } from 'react-i18next'
import { Empty } from '../ui/kit'

export default function Communications() {
  const { t } = useTranslation()
  return (
    <div className="page">
      <Empty icon="phone" title={t('communications.titulo')}>
        {t('communications.texto')}
      </Empty>
    </div>
  )
}
