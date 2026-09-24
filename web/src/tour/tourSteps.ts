/**
 * Guia de primeira utilização do Início (template DelonixTour): três dos
 * quatro mosaicos de acção rápida, na mesma ordem do mockup (`anchor` 0, 1,
 * 3 — «Entrar com código» fica de fora, tal como no ficheiro de origem).
 * `anchor` liga ao `data-tour` do mosaico em `pages/Home.tsx`.
 */
export interface TourStep {
  anchor: string
  titleKey: string
  bodyKey: string
  tipKey?: string
}

export const HOME_TOUR_STEPS: TourStep[] = [
  {
    anchor: 'iniciar',
    titleKey: 'tour.home.iniciar.titulo',
    bodyKey: 'tour.home.iniciar.corpo',
    tipKey: 'tour.home.iniciar.dica',
  },
  {
    anchor: 'agendar',
    titleKey: 'tour.home.agendar.titulo',
    bodyKey: 'tour.home.agendar.corpo',
  },
  {
    anchor: 'estudio',
    titleKey: 'tour.home.estudio.titulo',
    bodyKey: 'tour.home.estudio.corpo',
    tipKey: 'tour.home.estudio.dica',
  },
]
