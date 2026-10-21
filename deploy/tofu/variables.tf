# ---------------------------------------------------------------------------
#  O que é PRÓPRIO desta instalação. Nada aqui tem segredo nem IP público.
#
#  Os TAMANHOS são um ponto de partida declarado, não um dimensionamento: o
#  ADR-0020 diz que não foi medido o que o host tem. Mede antes de subir.
# ---------------------------------------------------------------------------
variable "proxmox_endpoint" {
  description = "API do Proxmox, ex. https://192.168.1.10:8006/"
  type        = string
}

variable "proxmox_insecure" {
  description = "Aceitar o certificado do Proxmox sem o verificar (laboratório)."
  type        = bool
  default     = false
}

variable "proxmox_ssh_user" {
  description = "Utilizador SSH no nó Proxmox, para as operações que a API não faz."
  type        = string
  default     = "root"
}

variable "node_name" {
  description = "Nome do nó Proxmox onde as VMs nascem."
  type        = string
  default     = "pve"
}

variable "template_id" {
  description = "VMID do template cloud-init (Debian 12 ou Ubuntu 24.04) a clonar."
  type        = number
}

variable "datastore_id" {
  description = "Onde os discos ficam, ex. local-lvm."
  type        = string
  default     = "local-lvm"
}

variable "network_bridge" {
  description = "Bridge do Proxmox para a rede das VMs."
  type        = string
  default     = "vmbr0"
}

variable "gateway" {
  description = "Gateway da rede das VMs."
  type        = string
  default     = "192.168.1.1"
}

variable "dns_servers" {
  type    = list(string)
  default = ["192.168.1.1", "1.1.1.1"]
}

variable "ssh_public_keys" {
  description = "Chaves que entram nas VMs. Sem isto não há como lá chegar."
  type        = list(string)
}

variable "vm_user" {
  description = "Utilizador criado pelo cloud-init."
  type        = string
  default     = "delonix"
}

# ---- control-plane ----
#
# TRÊS para o etcd ter quórum. Com UM só host Proxmox isto protege contra a
# morte de uma VM, de um kubelet ou de um etcd — NÃO contra a morte do host.
# Está escrito no ADR-0020 e repete-se aqui para quem lê só o código.
variable "control_plane" {
  description = "Os nós de control-plane: quantos, e o tamanho de cada um."
  type = object({
    count     = number
    cores     = number
    memory_mb = number
    disk_gb   = number
    ip_start  = number # último octeto do primeiro; os outros seguem-se
  })
  default = {
    count     = 3
    cores     = 2
    memory_mb = 4096
    disk_gb   = 40
    ip_start  = 20 # 192.168.1.20, .21, .22
  }
}

# ---- nós de trabalho ----
#
# O servidor do Meet pede 500m de CPU e 512Mi por réplica, com tecto de 2 CPU e
# 2Gi, e o HPA sobe até oito. Três nós destes dão folga para oito réplicas mais
# o Postgres, o Redis, o MinIO e a observabilidade — no papel.
variable "workers" {
  description = "Os nós de trabalho: quantos, e o tamanho de cada um."
  type = object({
    count     = number
    cores     = number
    memory_mb = number
    disk_gb   = number
    ip_start  = number
  })
  default = {
    count     = 3
    cores     = 8
    memory_mb = 16384
    disk_gb   = 200
    ip_start  = 30 # 192.168.1.30, .31, .32
  }
}

variable "ip_prefix" {
  description = "Os três primeiros octetos da rede das VMs."
  type        = string
  default     = "192.168.1"
}

variable "ip_cidr" {
  description = "Máscara da rede, em bits."
  type        = number
  default     = 24
}

variable "cluster_name" {
  description = "Prefixo dos nomes das VMs. `meet-` separa-as de tudo o resto no Proxmox."
  type        = string
  default     = "meet"
}
