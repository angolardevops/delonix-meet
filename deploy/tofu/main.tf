# ---------------------------------------------------------------------------
#  As VMs. Dois grupos com a mesma forma — control-plane e trabalho — porque a
#  diferença entre eles é tamanho e papel, não receita.
#
#  `for_each` e não `count`: com `count`, tirar o nó do meio renumera os que
#  vêm a seguir e o OpenTofu destrói e recria VMs que não foram tocadas.
# ---------------------------------------------------------------------------
locals {
  control_planes = {
    for i in range(var.control_plane.count) :
    "${var.cluster_name}-cp-${i + 1}" => {
      ip   = "${var.ip_prefix}.${var.control_plane.ip_start + i}"
      role = "control-plane"
    }
  }
  workers = {
    for i in range(var.workers.count) :
    "${var.cluster_name}-worker-${i + 1}" => {
      ip   = "${var.ip_prefix}.${var.workers.ip_start + i}"
      role = "worker"
    }
  }
  # O cloud-init é o mesmo para todas: quem distingue os papéis é o Ansible.
  todas = merge(local.control_planes, local.workers)
}

resource "proxmox_virtual_environment_vm" "no" {
  for_each = local.todas

  name      = each.key
  node_name = var.node_name
  tags      = ["delonix-meet", local.todas[each.key].role]
  # Arranca com o host: um reinício do Proxmox não deixa o cluster em baixo à
  # espera de alguém carregar em «start».
  on_boot = true

  clone {
    vm_id = var.template_id
    full  = true
  }

  cpu {
    cores = contains(keys(local.control_planes), each.key) ? var.control_plane.cores : var.workers.cores
    # `host` em vez do modelo genérico: o kubelet e o Postgres usam instruções
    # que o `kvm64` esconde, e o custo é não poder migrar a quente entre CPUs
    # diferentes — o que num host só não é custo nenhum.
    type = "host"
  }

  memory {
    dedicated = contains(keys(local.control_planes), each.key) ? var.control_plane.memory_mb : var.workers.memory_mb
    # Sem balão: o Kubernetes decide por `requests`/`limits` e um balão a tirar
    # memória por baixo do kubelet dá OOM onde o cluster julga ter folga.
    floating = 0
  }

  disk {
    datastore_id = var.datastore_id
    interface    = "scsi0"
    size         = contains(keys(local.control_planes), each.key) ? var.control_plane.disk_gb : var.workers.disk_gb
    discard      = "on"
    ssd          = true
  }

  network_device {
    bridge = var.network_bridge
  }

  agent {
    # O qemu-guest-agent vem no cloud-init (ver `user_data.yaml`): sem ele o
    # OpenTofu não sabe o IP da VM e o `tofu apply` fica à espera para sempre.
    enabled = true
  }

  initialization {
    datastore_id = var.datastore_id
    ip_config {
      ipv4 {
        address = "${local.todas[each.key].ip}/${var.ip_cidr}"
        gateway = var.gateway
      }
    }
    dns {
      servers = var.dns_servers
    }
    user_account {
      username = var.vm_user
      keys     = var.ssh_public_keys
    }
    user_data_file_id = proxmox_virtual_environment_file.cloud_init[each.key].id
  }

  lifecycle {
    # O disco cresce com `tofu apply`; encolher destrói dados. Mudar o tamanho
    # para baixo tem de ser um acto deliberado, fora daqui.
    prevent_destroy = false
    ignore_changes  = [initialization[0].user_account[0].keys]
  }
}

resource "proxmox_virtual_environment_file" "cloud_init" {
  for_each = local.todas

  content_type = "snippets"
  datastore_id = "local"
  node_name    = var.node_name

  source_raw {
    file_name = "${each.key}-user-data.yaml"
    data = templatefile("${path.module}/user_data.yaml.tftpl", {
      hostname = each.key
      user     = var.vm_user
      keys     = var.ssh_public_keys
    })
  }
}
