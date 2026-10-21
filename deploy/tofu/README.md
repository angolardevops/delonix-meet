# As VMs do cluster de produção do Meet

O que levanta as máquinas de `meet.ngolacloud.com` no Proxmox `192.168.1.10`.
A decisão e os limites estão no [ADR-0020](../../docs/adr/0020-producao-do-meet-em-meet-ngolacloud-com.md);
isto é o como.

## Antes de correr

1. **Um template cloud-init no Proxmox** (Debian 12 ou Ubuntu 24.04) e o VMID dele.
2. **Um token de API** com direito a criar VMs:
   ```bash
   export PROXMOX_VE_API_TOKEN='utilizador@pam!token=xxxxxxxx-...'
   ```
   O token **não entra no repositório** e não há sítio nenhum nestes ficheiros onde o pôr.
3. `cp terraform.tfvars.example terraform.tfvars` e preencher — em especial a chave SSH,
   sem a qual não há como entrar nas VMs.

## Correr

```bash
tofu init
tofu plan            # LÊ O PLANO. São seis VMs.
tofu apply
tofu output -raw inventario_ansible > ../ansible/inventory-producao.ini
```

O inventário do Ansible **sai do `tofu output`** e não se escreve à mão: um sítio só a
dizer que máquinas existem. Escrevê-lo à mão foi sempre o princípio da deriva entre o que
o OpenTofu criou e o que o Ansible configura.

## O que isto dá, e o que não dá

**Dá:** seis VMs (três de control-plane, três de trabalho) com IP fixo, utilizador com a
tua chave, `qemu-guest-agent`, relógio sincronizado, swap desligada e os `sysctl` que o
`kubeadm` confere no pré-voo. Arrancam com o host.

**Não dá:** o Kubernetes. Isso é o Ansible, a seguir.

**E não dá alta disponibilidade a sério.** Três control-planes em três VMs do **mesmo**
servidor físico protegem contra a morte de uma VM, de um kubelet ou de um etcd — não
contra a morte do host, do disco ou da fonte. Está no ADR-0020 e repete-se aqui porque é
a frase que mais facilmente se perde entre a palavra «HA» e a realidade.

**Os tamanhos são um palpite declarado.** Ninguém mediu o que o `192.168.1.10` tem.
Seis VMs com os valores por omissão pedem **30 vCPU, 60 GB de RAM e 720 GB de disco** —
mede o host antes de correr o `apply`.

## Mudar o tamanho de um nó

Editar `variables.tf` (ou o `terraform.tfvars`) e `tofu apply`. O disco **cresce**;
encolher destrói dados e o OpenTofu recusa-se — se for mesmo preciso, é um acto
deliberado, fora daqui.

Tirar um nó do meio não renumera os outros: os nós são um `for_each` por nome, e não um
`count` por índice, precisamente para isso.
