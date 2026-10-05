/**
 * O menu de contexto de uma gravação, partilhado pela GRELHA e pela TABELA.
 *
 * As duas vistas desenham a mesma gravação de maneiras diferentes e tinham o
 * mesmo problema: as acções (partilhar, editar no Estúdio, abrir em página
 * inteira) só existiam DEPOIS de a seleccionar, no painel da direita, e numa
 * biblioteca com cinquenta gravações isso são dois cliques e uma leitura de
 * vídeo para chegar a «Partilhar».
 *
 * Uma gravação FALHADA não abre menu (R59): não há ficheiro, e oferecer acções
 * sobre o que não existe é prometer duas vezes à mesma pessoa.
 */
import { useState, type ReactElement } from 'react'
import { useTranslation } from 'react-i18next'
import { Menu, useMenuDeContexto, type AccaoDeMenu, type EventoDePonteiro } from '../../ui/Menu'
import type { RecordingView } from './recordingView'
import { playerHash, studioEditHash } from './studioLink'

export function useMenuDaGravacao({
  onOpen,
  onShare,
}: {
  onOpen: (r: RecordingView) => void
  onShare?: (r: RecordingView) => void
}): {
  /** Para o `onContextMenu` do cartão ou da linha. */
  aoContexto: (r: RecordingView) => (e: EventoDePonteiro) => void
  /** O menu em si — a vista desenha-o uma vez, fora do laço. */
  elemento: ReactElement
} {
  const { t } = useTranslation()
  const menu = useMenuDeContexto()
  const [alvo, setAlvo] = useState<RecordingView | null>(null)

  const aoContexto = (r: RecordingView) => (e: EventoDePonteiro) => {
    if (r.failed) return
    setAlvo(r)
    menu.abrir(e)
  }

  const accoes: AccaoDeMenu[] = []
  if (alvo) {
    accoes.push(
      { id: 'abrir', label: t('recordings.abrir', { name: alvo.name }), icon: 'play', onPick: () => onOpen(alvo) },
      {
        id: 'leitor',
        label: t('player.paginaInteira'),
        icon: 'maximize',
        onPick: () => {
          location.hash = playerHash(alvo.id).slice(1)
        },
      },
      {
        id: 'editar',
        label: t('player.editarStudio'),
        icon: 'scissors',
        onPick: () => {
          location.hash = studioEditHash(alvo.id).slice(1)
        },
      },
    )
    // Partilhar é só de quem é dona — a mesma guarda do painel.
    if (onShare && alvo.owned) {
      accoes.push({ id: 'partilhar', label: t('recordings.accoes.partilhar'), icon: 'share', onPick: () => onShare(alvo) })
    }
  }

  const elemento = (
    <Menu ponto={menu.ponto} accoes={accoes} label={t('recordings.menuDe', { name: alvo?.name ?? '' })} onFechar={menu.fechar}>
      {alvo?.name}
    </Menu>
  )
  return { aoContexto, elemento }
}
