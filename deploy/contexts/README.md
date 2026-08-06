# Contextos de deploy — escolhe um, copia, corre

Três cenários prontos. O deploy do **Delonix Meet** é autónomo: não se mistura
com o da plataforma NgolaCloud (`delonix-paas`, `delonix-runtime`,
`delonix-portal`, `delonix-admin`), que tem o seu próprio ciclo.

```bash
cp deploy/contexts/<contexto>.yml deploy/config.yml
$EDITOR deploy/config.yml          # preencher o que está marcado ⚑
make deploy
```

`deploy/config.yml` está no `.gitignore` — valores reais nunca vão para o repo.

## Qual escolher

| | [`publico.yml`](publico.yml) | [`interno.yml`](interno.yml) | [`cluster.yml`](cluster.yml) |
|---|---|---|---|
| **Quando** | Acesso pela Internet, participantes externos | Só rede da empresa/VPN | Não pode parar; centenas em simultâneo |
| **Servidores** | 1 | 1 | 3+ |
| **Certificado** | Let's Encrypt (automático, confiável) | Self-signed | Let's Encrypt via ingress |
| **Aviso no browser** | Não | **Sim**, até instalarem o CA interno | Não |
| **Precisas de ter** | domínio + DNS a apontar + porta 80/443 abertas | IP fixo do servidor | cluster ou nós para o kubeadm |
| **Actualizar** | `make deploy` | `make deploy` | `make image-push` |
| **Se cair** | serviço para até reiniciar | idem | outras réplicas continuam |

Na dúvida entre os dois primeiros: se alguém de fora da empresa precisa de
entrar numa reunião, é o **público**. Não há meio-termo confortável — um
certificado self-signed num convidado externo é uma página de aviso vermelha
antes da reunião com o cliente.

## O que é comum aos três

**A media é o que costuma falhar**, não o HTTPS. Duas coisas têm de estar
certas em qualquer cenário:

1. **O `TURN_HOST` tem de ser alcançável pelo CLIENTE**, não pelo servidor.
   Um endereço que só resolve dentro do datacenter funciona nos teus testes e
   falha para toda a gente.
2. **UDP 49152–59152 aberto** (a gama do relay). Fechada, o ICE liga e a
   imagem fica preta — o sintoma mais confuso de todos.

Detalhe e diagnóstico: [docs/deployment.md](../../docs/deployment.md).

**As migrações correm sozinhas** no arranque do servidor. São aditivas, mas
não há migrações de reversão: **tira um backup antes do primeiro deploy de
uma versão nova**.

**Os segredos são gerados uma vez** e persistidos em
`deploy/ansible/.secrets/` (gitignored). Correr `make deploy` de novo reutiliza
— não desloga ninguém. Para rodar um segredo, apaga a linha respectiva e
volta a correr; trocar o `JWT_SECRET` desliga **todas** as sessões.

## Integração com o Odoo

Independente do cenário. Para ligar o login por conta Odoo, acrescenta ao
`/etc/delonix/delonix.env` do servidor:

```bash
PLATFORM_ODOO_URL=https://erp.empresa.com
PLATFORM_ODOO_DB=empresa_prod
```

Vazias = desligado. Ver [docs/deployment.md §7](../../docs/deployment.md#7-integração-odoo)
para os limites (uma instância Odoo por plataforma) e as implicações de
segurança (quem é desactivado no Odoo perde o acesso no login seguinte).
