// Cenário do lote 2 da sala: seis pessoas reais, papéis, mãos, chat com fio e
// reacções, perguntas, moderação, quadro e telemóvel. Corre por cima de
// `sala.mjs` (CENARIO=./cenario-l2.mjs) e fotografa cada estado.
//
// Pessoas: convidados = [Joaquim, Teresa, Domingos, Luísa, Paulo]; todas
// começam na sala de espera; o anfitrião admite três e deixa duas à porta.
export default async function ({ hp, convidados, code, esperar, fotografar, want, APP, log }) {
  const [joaquim, teresa, domingos] = convidados
  const passo = async (nome, fn) => {
    try {
      await fn()
      log('passo', nome)
    } catch (e) {
      log('FALHOU', nome, e.message.split('\n')[0])
    }
  }

  await passo('a pré-entrada admitiu quem já esperava', async () => {
    await hp.locator('.rm-tile[data-peer="remoto"]').nth(2).waitFor({ timeout: 20000 })
  })
  await esperar(4000)

  // Moderação numa segunda aba do anfitrião: papéis pelo set-role.
  const lp = await hp.context().newPage()
  await lp.goto(`${APP}/#/lobby/${code}`)
  await lp.locator('.lb-table').waitFor({ timeout: 20000 })
  await esperar(2500)
  const linha = (nome) => lp.locator('.lb-table tr', { hasText: nome })
  await passo('promover Joaquim a co-anfitrião', async () => {
    await linha('Joaquim').getByRole('button', { name: 'Promover' }).click({ timeout: 8000 })
    await linha('Joaquim').locator('.lb-role', { hasText: /co-anfitri/i }).waitFor({ timeout: 8000 })
  })
  await passo('Teresa oradora (Promover → Passar palco)', async () => {
    await linha('Teresa').getByRole('button', { name: 'Promover' }).click({ timeout: 8000 })
    await linha('Teresa').getByRole('button', { name: 'Passar palco' }).click({ timeout: 8000 })
    await linha('Teresa').locator('.lb-role', { hasText: /orador/i }).waitFor({ timeout: 8000 })
  })

  await passo('Teresa levanta a mão', async () => {
    await teresa.getByRole('button', { name: /levantar a mão/i }).click({ timeout: 8000 })
  })
  // A consola de moderação é outra ligação do anfitrião (aparece como pessoa na
  // sala): sai-se dela para a grelha, e volta-se no fim.
  await lp.goto(`${APP}/#/`)
  await esperar(800)
  await hp.bringToFront()
  await esperar(1500)
  if (want.has('DelonixRoomGrid')) await fotografar(hp, 'DelonixRoomGrid')

  // Chat: Joaquim pergunta, o anfitrião responde no fio, Teresa reage.
  await passo('chat com fio e reacções', async () => {
    await joaquim.getByRole('button', { name: /^chat$/i }).first().click({ timeout: 8000 })
    const campo = joaquim.getByRole('textbox', { name: /mensagem para/i })
    await campo.fill('O failover entre os dois SBC está testado com o operador? Tenho registo de uma queda na semana passada.')
    await campo.press('Enter')
    await esperar(1200)
    await hp.getByRole('button', { name: /^chat$/i }).first().click({ timeout: 8000 })
    const msg = hp.locator('.rm-chat__msg', { hasText: 'failover entre os dois SBC' })
    await msg.waitFor({ timeout: 10000 })
    await msg.hover()
    await msg.getByRole('button', { name: 'Responder', exact: true }).first().click({ timeout: 5000 })
    const campoH = hp.getByRole('textbox', { name: /mensagem para/i })
    await campoH.fill('Sim — corrigido na versão 2.4. Mostro o relatório no diapositivo 21.')
    await campoH.press('Enter')
    await teresa.getByRole('button', { name: /^chat$/i }).first().click({ timeout: 8000 })
    const msgT = teresa.locator('.rm-chat__msg', { hasText: 'failover entre os dois SBC' })
    await msgT.waitFor({ timeout: 10000 })
    await msgT.hover()
    await msgT.getByRole('button', { name: /reagir com 👍/i }).click({ timeout: 5000 })
    await esperar(600)
    const campoT = teresa.getByRole('textbox', { name: /mensagem para/i })
    await campoT.fill('Deixo o plano de numeração actualizado.')
    await campoT.press('Enter')
    await domingos.getByRole('button', { name: /^chat$/i }).first().click({ timeout: 8000 })
    const msgD = domingos.locator('.rm-chat__msg', { hasText: 'failover entre os dois SBC' })
    await msgD.waitFor({ timeout: 10000 })
    await msgD.hover()
    await msgD.getByRole('button', { name: /reagir com 🎯/i }).click({ timeout: 5000 })
  })
  await passo('pergunta no Q&A', async () => {
    await domingos.getByRole('tab', { name: /perguntas/i }).click({ timeout: 8000 })
    const q = domingos.getByRole('textbox', { name: /faz uma pergunta/i })
    await q.fill('A gravação em 4K fica disponível para quem assiste pelo YouTube?')
    await q.press('Enter')
  })
  await passo('Teresa partilha o ecrã', async () => {
    // A 900 px o painel cobre a barra: fecha-se primeiro.
    await teresa.getByRole('button', { name: /^fechar painel$/i }).first().click({ timeout: 4000 }).catch(() => {})
    await teresa.getByRole('button', { name: /^partilhar ecrã|^pedir para partilhar/i }).first().click({ timeout: 8000 })
    // Só o anfitrião partilha sem pedir: autoriza e ela partilha.
    const permitir = hp.locator('.rm-notices button', { hasText: /^permitir$/i }).first()
    if (await permitir.waitFor({ timeout: 6000 }).then(() => true).catch(() => false)) {
      await permitir.click()
      log('partilha: autorizada (a partilha arranca sozinha)')
    }
  })
  await passo('sondagem no fio', async () => {
    await hp.getByRole('tab', { name: /^chat/i }).click({ timeout: 8000 })
    await hp.getByRole('button', { name: 'Nova sondagem' }).click({ timeout: 8000 })
    await hp.locator('#rm-poll-q').fill('Onde preferem a gravação desta formação?')
    const ops = ['Storage interno (MinIO)', 'Nextcloud da instituição', 'Apenas emissão, sem gravação']
    for (let i = 0; i < ops.length; i++) {
      const campo = hp.getByRole('textbox', { name: `Opção ${i + 1}` })
      if (!(await campo.count())) await hp.getByRole('button', { name: /^opção$/i }).click()
      await hp.getByRole('textbox', { name: `Opção ${i + 1}` }).fill(ops[i])
    }
    await hp.getByRole('button', { name: /lançar sondagem/i }).click({ timeout: 5000 })
    await esperar(1500)
    for (const [p, i] of [[joaquim, 0], [teresa, 0], [domingos, 1]]) {
      await p.locator('.rm-notices .rm-poll__opt').nth(i).click({ timeout: 8000 }).catch(() => {})
    }
    await hp.getByRole('button', { name: /^chat$/i }).first().click({ timeout: 8000 })
  })
  await esperar(6000)
  await hp.mouse.move(5, 5)
  if (want.has('DelonixRoomChat')) await fotografar(hp, 'DelonixRoomChat')

  const cont = (await import('./cenario-quadro.mjs').catch(() => null))?.default
  if (cont) await cont({ hp, convidados, code, esperar, fotografar, want, APP, log, passo, lp })

  // Telemóvel (reunião activa): a mesma sessão do anfitrião a 390×844.
  if (want.has('DelonixMobile')) {
    await passo('telemóvel', async () => {
      await hp.getByRole('button', { name: /^quadro branco$/i }).first().click({ timeout: 4000 }).catch(() => {})
      await hp.setViewportSize({ width: 390, height: 844 })
      await hp.reload()
      await hp.locator('.rm-shell').waitFor({ timeout: 30000 })
      await esperar(5000)
      await fotografar(hp, 'DelonixMobile')
      await hp.setViewportSize({ width: 1440, height: 900 })
    })
  }

  // Moderação no fim: com as duas pessoas à porta e três salas paralelas.
  if (want.has('DelonixModeration')) {
    await passo('moderação com salas paralelas', async () => {
      // A Luísa e o Paulo saem e voltam: ficam outra vez à porta.
      for (const p of convidados.slice(3)) {
        await p.goto(`${APP}/#/`)
        await esperar(800)
        await p.goto(`${APP}/#/r/${code}`)
        const botao = p.getByRole('button', { name: /^entrar na sess/i })
        if (await botao.waitFor({ timeout: 8000 }).then(() => true).catch(() => false)) await botao.click()
      }
      await lp.goto(`${APP}/#/lobby/${code}`)
      await lp.locator('.lb-table').waitFor({ timeout: 20000 })
      await esperar(2500)
      await lp.locator('.rm-bocard select').selectOption('15').catch(() => {})
      await lp.getByRole('button', { name: /^3 grupos$/ }).click({ timeout: 8000 })
      await esperar(2500)
      await lp.mouse.move(5, 5)
      await fotografar(lp, 'DelonixModeration')
    })
  }
}
