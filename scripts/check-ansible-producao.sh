#!/usr/bin/env bash
# ============================================================
#  Fitness function: o Ansible da PRODUÇÃO lê-se e resolve-se (ADR-0020, fase 2).
#
#  PORQUE EXISTE. O `deploy/ansible/producao.yml` e os cinco roles entraram no
#  repo sem nunca terem sido lidos por um Ansible: `tofu validate` cobria as
#  VMs, e do playbook não havia portão nenhum. Um `hosts:` com um grupo que o
#  inventário não tem, um role com o nome trocado, um `when` mal fechado — tudo
#  isso só aparecia com seis VMs já criadas e um `ansible-playbook` a meio.
#
#  O que se mede, com um inventário FALSO com a forma do que o `tofu output`
#  gera (três control-planes, três workers):
#    1. o playbook e todos os roles fazem parse (`--syntax-check`);
#    2. os `hosts:` de cada play casam com grupos que existem — um play que não
#       casa com host nenhum é um play que nunca corre, e o Ansible só avisa;
#    3. a lista de tarefas resolve-se até ao fim (`--list-tasks`), o que obriga
#       cada `roles:` a ser encontrado.
#
#  NÃO corre nada contra máquina nenhuma, e não substitui a primeira instalação.
#  Sem `ansible-playbook`, salta (e di-lo).
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."

AP=deploy/ansible/producao.yml
r=$'\e[31m'; g=$'\e[32m'; y=$'\e[33m'; z=$'\e[0m'

command -v ansible-playbook >/dev/null 2>&1 || {
  echo "  ${y}· ansible-playbook ausente — o playbook de produção NÃO foi medido${z}"
  exit 0
}
[ -f "$AP" ] || { echo "${r}✗ ansible: falta $AP${z}"; exit 1; }

inv=$(mktemp "${TMPDIR:-/tmp}/delonix-inv.XXXXXX")
trap 'rm -f "$inv"' EXIT
# A MESMA forma que o `deploy/tofu/outputs.tf` gera. Se ela mudar lá e não aqui,
# este portão deixa de medir o que o Ansible vai receber — por isso os nomes dos
# grupos estão nos dois sítios e são a parte que importa.
cat > "$inv" <<'INV'
[control_plane]
cp1 ansible_host=10.0.0.11 ansible_user=delonix
cp2 ansible_host=10.0.0.12 ansible_user=delonix
cp3 ansible_host=10.0.0.13 ansible_user=delonix

[workers]
w1 ansible_host=10.0.0.21 ansible_user=delonix
w2 ansible_host=10.0.0.22 ansible_user=delonix
w3 ansible_host=10.0.0.23 ansible_user=delonix

[k8s:children]
control_plane
workers

[k8s:vars]
cluster_name=prova
api_endpoint=10.0.0.11
INV

falhas=0

saida=$(ansible-playbook --syntax-check -i "$inv" "$AP" 2>&1)
if [ $? -ne 0 ]; then
  echo "${r}✗ ansible: o playbook de produção não faz parse${z}"
  printf '%s\n' "$saida" | tail -20
  falhas=$((falhas + 1))
else
  echo "  ${g}✓ parse do playbook e dos roles${z}"
fi

# Um `hosts:` que não casa com nada é um play que NUNCA corre, e o Ansible
# limita-se a avisar. Aqui é erro.
if grep -q "Could not match supplied host pattern" <<<"$saida"; then
  echo "${r}✗ ansible: há um «hosts:» que não casa com o inventário gerado pelo tofu:${z}"
  grep "Could not match supplied host pattern" <<<"$saida" | sed 's/^/     /'
  falhas=$((falhas + 1))
fi

tarefas=$(ansible-playbook --list-tasks -i "$inv" "$AP" 2>&1)
if [ $? -ne 0 ]; then
  echo "${r}✗ ansible: a lista de tarefas não resolve (um role não foi encontrado?)${z}"
  printf '%s\n' "$tarefas" | tail -20
  falhas=$((falhas + 1))
else
  n=$(grep -cE '^\s+[a-zA-Z].*TAGS:' <<<"$tarefas")
  [ "${n:-0}" -gt 0 ] || { echo "${r}✗ ansible: zero tarefas — o playbook não faz nada${z}"; falhas=$((falhas + 1)); }
  [ "${n:-0}" -gt 0 ] && echo "  ${g}✓ $n tarefas resolvidas${z}"
fi

[ "$falhas" -gt 0 ] && { echo "${r}✗ ansible: $falhas problema(s)${z}"; exit 1; }
echo "${g}✓ ansible de produção: lê-se, os plays casam com o inventário do tofu, e as tarefas resolvem${z}"
