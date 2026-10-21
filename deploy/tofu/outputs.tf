# O inventário do Ansible sai daqui: UM sítio a dizer que máquinas existem.
# Escrever o inventário à mão foi sempre o princípio da deriva entre o que o
# OpenTofu criou e o que o Ansible configura.
output "inventario_ansible" {
  description = "Escreve com: tofu output -raw inventario_ansible > ../ansible/inventory-producao.ini"
  value = join("\n", concat(
    ["# GERADO por `tofu output` — não editar à mão (ADR-0020).", "[control_plane]"],
    [for n, v in local.control_planes : "${n} ansible_host=${v.ip} ansible_user=${var.vm_user}"],
    ["", "[workers]"],
    [for n, v in local.workers : "${n} ansible_host=${v.ip} ansible_user=${var.vm_user}"],
    ["", "[k8s:children]", "control_plane", "workers"],
    ["", "[k8s:vars]", "cluster_name=${var.cluster_name}",
    "api_endpoint=${[for n, v in local.control_planes : v.ip][0]}"],
  ))
}

output "ips" {
  description = "Os endereços, por papel."
  value = {
    control_plane = { for n, v in local.control_planes : n => v.ip }
    workers       = { for n, v in local.workers : n => v.ip }
  }
}

output "primeiro_control_plane" {
  description = "Onde o `kubeadm init` corre, e de onde o kubeconfig vem."
  value       = [for n, v in local.control_planes : v.ip][0]
}
