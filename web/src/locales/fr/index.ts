// Dicionário FR — um ficheiro por área (namespace). Cada chave é uma
// string numa só linha: o portão de paridade (lote2 3.2.7) lê-os linha a linha.

import ui from './ui'
import shell from './shell'
import auth from './auth'
import home from './home'
import schedule from './schedule'
import room from './room'
import studio from './studio'
import recordings from './recordings'
import boards from './boards'
import org from './org'
import analytics from './analytics'
import integrations from './integrations'
import publico from './publico'

export default { ui, shell, auth, home, schedule, room, studio, recordings, boards, org, analytics, integrations, publico }
