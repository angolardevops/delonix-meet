# Pre-join (green room) da sala — desenho + plano de teste

> **Estado: IMPLEMENTADO** em [Room.tsx](../web/src/pages/Room.tsx) (`roomState
> 'prejoin'`, handoff do preview, skip por voz/rejoin). Verificado headless em
> modo espectador: prejoin → Entrar → `joined` → sala (barra + painéis), skip no
> reload de reconexão, Cancelar limpa e volta. **Falta SÓ o passo 4** do plano de
> teste (§4): media bidirecional com 2 browsers reais com câmara — obrigatório
> antes de considerar fechado (R1/R2 vivem aqui).

---

## 1. Objetivo (paridade Zoom/Teams)

Um **green room** antes de entrar na chamada:
- Pré-visualização da câmara (com blur/fundos já aplicáveis).
- Seleção de microfone / câmara / altifalante + **teste de áudio** (nível do mic, tom de teste).
- Alternar câmara/mic ON/OFF antes de entrar.
- Botão **Entrar agora** (e "Entrar só a ouvir" / espectador).
- Nome da reunião + quem já está lá (se disponível).

## 2. Desenho seguro (respeita R1/R2)

O princípio-chave alinha-se com **R2**: **não montar a `SfuCall` antes de o utilizador confirmar**.

```
estado da sala:  'prejoin'  ──[Entrar]──►  'joining'  ──[joined]──►  'in'
                    │ preview local             │ callHolder.start()      │ SfuCall ativa
                    │ getUserMedia p/ preview    │ (cria SfuCall UMA vez)  │
                    └ SEM SfuCall                └ oferta no construtor    └ R1 intacto (R1)
```

- A `getUserMedia` do **preview** obtém um stream **local** (para o `<video>` de pré-visualização e para escolher dispositivos) — **não** cria PC nem SfuCall.
- Ao clicar **Entrar**, passa-se o stream já obtido ao fluxo existente e só então `callHolder.start()` cria a `SfuCall` (mantendo a oferta no construtor — **R1**).
- Enquanto está em `prejoin`, comporta-se como o convidado em espera de **R2**: **nenhuma** `SfuCall`, **nenhuma** oferta stale.

## 3. Passos de implementação (Room.tsx)

1. **Estado**: adicionar `'prejoin'` ao `roomState` (hoje começa a adquirir media e a juntar). Iniciar em `'prejoin'` (exceto quando se entra por link já "pronto").
2. **Ecrã prejoin**: novo bloco render (à parte da `waiting-page`) com `<video>` do preview + os menus de dispositivo já existentes (reutilizar `DeviceMenu`/`switchMic`/`switchCam`) + o painel de efeitos (`fx-panel`).
3. **Aquisição de preview**: reutilizar a função de aquisição de media local que já existe, mas **sem** ligar à SFU. Guardar o stream em ref para reentregar no join.
4. **Entrar**: `onJoin()` → transita para `'joining'` → o `callHolder.start()` (que já é idempotente, R2) usa o stream do preview → handler `joined` → `'in'`.
5. **Não regredir**: manter a oferta SFU **no construtor** da `SfuCall` (R1); manter `callHolder.start()` idempotente e só chamado no clique/`joined` (R2). Não "gatear" a oferta pelo `joined`.
6. **Limpeza**: se sair do prejoin sem entrar, parar os tracks do preview (`getTracks().forEach(stop)`).

## 4. Plano de teste de media (2 browsers — OBRIGATÓRIO)

Ambiente: `make dev` (backend systemd + coturn local) OU stage k8s. Duas janelas/browsers distintos.

| # | Passo | Esperado |
|---|---|---|
| 1 | Browser A: criar sala → **ecrã prejoin** | Preview da câmara visível; menus de dispositivo funcionam; **sem** logs de `pc connected`/`track published` (ainda não juntou) |
| 2 | A: escolher câmara/mic, testar áudio | Nível do mic mexe; tom de teste ouve-se no altifalante escolhido |
| 3 | A: **Entrar** | Transita para a sala; logs mostram `sfu-offer` (construtor), `sfu-answer`, `pc connected`, `track published` (**R1 OK**) |
| 4 | Browser B: abrir link da sala → prejoin → **Entrar** | Ambos se veem/ouvem — **media bidirecional** |
| 5 | Repetir B várias vezes (entrar/sair) | Sem reload em loop, sem flood/rate-limit (**R2 OK**) |
| 6 | Sala com **sala de espera**: B entra, fica em espera, A admite | B só monta a SfuCall **após admissão** (não em espera) — sem glare/rollback (**R2 OK**) |
| 7 | K8s multi-réplica: A e B na mesma sala | Caem no mesmo pod (`upstream-hash-by`), media nos dois sentidos (**R3 OK**) |

**Critério de aceitação:** passos 3–7 verdes em 2 browsers reais. Se algum falhar → **não** fazer merge (é regressão de media).

## 5. Painel com abas — estado

Já **implementado** (seguro, pura UI sobre `panel`): [Room.tsx](../web/src/pages/Room.tsx) `PanelTabs` (Pessoas · Chat · Ferramentas). Não toca em media. O green room acima é o que falta.

*Ver [ux-review.md](ux-review.md) (Room UX) e [regressions.md](reference/regressions.md) (R1/R2/R3).*
