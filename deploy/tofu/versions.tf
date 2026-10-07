# As VMs do cluster de produção do Meet, no Proxmox (ADR-0020).
#
# Infra PRÓPRIA do Meet: não partilha cluster, base de dados nem observabilidade
# com o resto do ngolacloud. É a excepção escrita no ADR — a regra do workspace
# manda o substrato vir do `delonix-deploy`, e o Meet vende-se por si.
terraform {
  required_version = ">= 1.6"
  required_providers {
    proxmox = {
      # O provider da comunidade (bpg) é o que suporta clonagem de template,
      # cloud-init e discos em `local-lvm`/ZFS sem truques.
      source  = "bpg/proxmox"
      version = "~> 0.66"
    }
  }
}

provider "proxmox" {
  endpoint = var.proxmox_endpoint
  # O token NUNCA entra no repositório: vem de PROXMOX_VE_API_TOKEN no ambiente.
  insecure = var.proxmox_insecure
  ssh {
    agent    = true
    username = var.proxmox_ssh_user
  }
}
