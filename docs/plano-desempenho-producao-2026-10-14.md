# Desempenho e isolamento em produção — o que está medido e o que se decide

**Data:** 2026-10-14 · **Estado:** para decisão do dono · **Mede:** o cluster
`delonix-lda` (namespace `ngolacloud-meet`, ADR-0021), pelo túnel.

Este documento nasceu de uma pergunta simples — «pôr o Ollama em produção» — que
se revelou já feita. O que **não** estava feito era perceber porque é que ele é
**28× mais lento em produção do que numa máquina de desenvolvimento carregada**.

Tudo o que segue é medido. O que não foi medido está dito como tal.

---

## 0. O que já lá está (e não precisa de trabalho)

| | Estado medido |
|---|---|
| **Servidor MCP** | **a servir**: `GET https://meet.ngolacloud.com/.well-known/oauth-protected-resource` → `200` com a metadata certa. A imagem em produção (`4bbb6080`) contém a #322 |
| **Ollama** | **a servir**: `delonix-ollama` 1/1, `OLLAMA_URL` apontado ao Service, `qwen2.5:7b` e `qwen2.5:1.5b` carregados |
| **Whisper (ASR)** | 1/1 |

**Não está em produção:** a dobragem ao vivo e os recortes (#346, fundida depois
da imagem actual).

---

## 1. A medição que motiva tudo

Tradução de uma frase de reunião (`qwen2.5:1.5b`, o modelo das legendas e da
dobragem):

| | Máquina de desenvolvimento | **Produção** |
|---|---:|---:|
| mediana | 0,48 s | **13,60 s** |
| débito | — | **1,7 tok/s** |
| arranque a frio | 19 s | **86 s** (64 s só a carregar) |

A máquina de desenvolvimento estava com `load average` 48 — ou seja, **a
produção perde para uma máquina sobrecarregada, por 28×**.

---

## 2. As duas causas, medidas

### 2.1 As VMs não têm AVX — e o silício tem

As flags de CPU dentro do contentor do Ollama são **`sse4_2 ssse3`**. Sem `avx`,
sem `avx2`, sem `fma`. O `llama.cpp` assenta nessas instruções para a aritmética
matricial; sem elas corre o caminho escalar.

A razão está na configuração das VMs:

```
qm config 204  →  cpu: x86-64-v2-AES     # SSE4.2 + AES, NENHUM AVX
```

E o ferro por baixo tem mais do que isso:

| Hipervisor | CPU | AVX | AVX2 | AVX-512 |
|---|---|:--:|:--:|:--:|
| `ngola` (192.168.1.10) | Xeon **E5-2620 v1** (Sandy Bridge, 2012) | ✅ | ❌ | ❌ |
| `delonix02` (192.168.1.20) | Xeon **E5-2620 v1** | ✅ | ❌ | ❌ |
| `delonix03` (192.168.1.30) | Xeon **Bronze 3106** (Skylake-SP, 2017) | ✅ | ✅ | ✅ |

**Estamos a deitar fora capacidade já paga.** Todas as VMs correm sem AVX quando
os três hipervisores o têm.

Nota operacional: já existe uma VM `cpu: x86-64-v3` (`delonix-lda-w-6`, VMID 209,
no delonix03) — **mas não é membro deste cluster de Kubernetes**. O `x86-64-v3`
exige AVX2, por isso essa VM **só arranca no delonix03**: se alguma vez for
migrada para os outros dois, não liga. Isso é uma armadilha a conhecer, não um
defeito.

### 2.2 O Ollama é travado 94% do tempo

O contentor **vê 6 CPU** (`nproc`, os do nó) e cria fios a essa conta, mas a
quota do cgroup é de **4**:

```
/sys/fs/cgroup/cpu.max  →  400000 100000     # 4 CPU por período de 100 ms
```

Medido numa única tradução:

```
UMA tradução: 15,89 s
  períodos travados: 159
  tempo travado:     14,99 s   (94% do pedido)
  CPU usado:         63,93 s
```

Ele queima a quota em ~67 ms de cada 100 ms e **dorme os outros 33**. Fixar os
fios ao limite **duplica o débito** (medido: 1,7 → 3,4 tok/s com `num_thread: 4`).

---

## 3. O risco que é maior do que a IA lenta

O nó **`delonix-lda-w-3` corre o Ollama E uma réplica do `delonix-server`**. O
mesmo acontece no `w-2` com o Whisper.

O servidor é o **SFU**: media em tempo real. Um SFU que perde CPU não fica
«lento» — fica com **som entrecortado**, e quem está na chamada não tem como
saber porquê. O nó está com os limites a **184%** de sobrecompromisso.

**Uma acta que demora três minutos é aceitável. Uma chamada que corta não é.**

---

## 4. O caminho recomendado, por ordem de valor

### 4.1 Separar os planos *(estrutural, e a mais importante)*

O **plano de tempo real** (SFU, coturn, FreeSWITCH) e o **plano de inferência**
(Ollama, Whisper, composição ffmpeg) **nunca partilham nó**. Faz-se com um
*taint* no nó de inferência e tolerância só nesses pods.

Isto é o que permite dizer «a acta demora três minutos» sem que isso signifique
«a chamada entrecortou».

### 4.2 O modelo de CPU das VMs — e o compromisso que decide

**`cpu: host` dá tudo o que o silício tem, mas impede migração AO VIVO entre
hipervisores diferentes.** Com dois Sandy Bridge e um Skylake, `host` significa
que uma VM do `delonix03` não migra para os outros dois.

A escolha depende de **uma pergunta que é tua**:

| Se… | Então |
|---|---|
| a HA **reinicia** a VM noutro nó (normal num cluster pequeno) | **`cpu: host` em todas.** O mais rápido e o mais simples |
| usas **migração ao vivo** | modelo `SandyBridge` — dá **AVX** a todas, mantém a migração, e o AVX2 fica só para o `delonix03` |

Em qualquer dos casos, sair de `x86-64-v2-AES` é um ganho: hoje não há AVX
nenhum.

### 4.3 A inferência vive no `delonix03`

É o único hipervisor com AVX2/AVX-512. Um nó dedicado, `cpu: host`, QoS
**Guaranteed** (pedidos = limites) e **sem sobrecompromisso**.

### 4.4 Nunca pôr quota de CPU em inferência com muitos fios

Ou se dá o nó inteiro, ou se fixam os fios ao limite. O certo é fazer as duas —
e a segunda vale 2× mesmo sem mais nada (§2.2).

### 4.5 Tapar o buraco das métricas *(primeiro, na prática)*

```
kubectl top node  →  error: Metrics API not available
```

**Um ambiente crítico sem métricas não tem HPA, não tem sinal de capacidade, e
não tem como provar nada do que está neste documento.** É o primeiro a tapar,
porque sem ele os outros não se verificam depois de aplicados.

---

## 5. A verdade dura sobre IA em CPU

Mesmo com tudo isto corrigido, o Bronze 3106 são **8 núcleos a 1,7 GHz**.

| Caso de uso | Com AVX2 no delonix03 | Veredicto |
|---|---|---|
| **acta (MoM)** — é uma fila, minutos são aceitáveis | passa a ser viável | ✅ CPU chega |
| **legendas ao vivo / dobragem** — precisam de **menos de um segundo** | continua longe | ❌ **quer GPU** (ADR-0024) |

Dizer o contrário seria vender o que não se entrega.

---

## 6. O que se pode fazer já, sem mexer em VMs

Duas correcções pequenas, medidas, e sem reiniciar nada de produção:

1. **fixar os fios do Ollama ao limite do contentor** (vale 2×);
2. **o *taint* que tira o SFU do nó de inferência** (tira o risco do §3).

As mudanças de CPU das VMs (§4.2) **exigem reiniciar VMs de produção** e são
decisão do dono.

---

## 7. O que NÃO foi medido

- o ganho real do AVX2 neste modelo e neste ferro — estima-se grande, mas **não
  foi medido** porque nenhuma VM deste cluster tem AVX2 ligado;
- o `qwen2.5:7b` em produção: só se conhece o piso de ~1 640 s de medições
  anteriores, e não se repetiu aqui;
- o efeito do `taint` sob carga real de sala;
- porque é que a `delonix-lda-w-6` existe e não está no cluster.
