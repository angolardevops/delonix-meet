#!/usr/bin/env python3
"""Avalia o resumo JSON de uma corrida do loadgen (stdin).

  loadgen-avaliar.py veredicto  → «OK|FALHA|INCONCLUSIVO motivos»
  loadgen-avaliar.py linha      → uma linha legível com os números
"""
import json, sys

s = json.load(sys.stdin)
if sys.argv[1] == "linha":
    print(
        f"  {s['label']:>16}  {s['participantes']:>4} pessoas  "
        f"vídeo {s['video_ativos_min']:.0f}/{s['video_esperados']}  "
        f"perda {s['perda_video_pct']:>5}%  jitter95 {s['jitter_p95_ms']:>5} ms  "
        f"{s['video_mbps']:>6} Mbps  srv {s['srv_cores']:>5} cores  "
        f"RSS {s['srv_rss_mb']:.0f} MB  gerador {s['gerador_cores']} cores "
        f"(atrasos {s['gerador_ticks_atrasados_pct']}%)  máquina {s['maquina_ocupada_pct']:.0f}%"
    )
    sys.exit()
m = []
# CRITERIO=media: mede o limite da MÁQUINA a encaminhar media. As subscrições
# em falta por corrida na entrada (bug do SFU, não de capacidade) só reprovam
# abaixo de 90%; continuam a aparecer no número de vídeo activo.
limiar = 0.90 if __import__("os").environ.get("CRITERIO") == "media" else 0.98
if s["video_ativos_min"] < limiar * s["video_esperados"]:
    m.append(f"vídeo activo {s['video_ativos_min']:.0f}/{s['video_esperados']}")
if s["perda_video_pct"] >= 2:
    m.append(f"perda {s['perda_video_pct']}%")
if s["jitter_p95_ms"] >= 30:
    m.append(f"jitter p95 {s['jitter_p95_ms']} ms")
if s["pc_falhadas"] > 0 or s["clientes_com_erro"] > 0:
    m.append(f"{s['pc_falhadas']} PC falhadas, {s['clientes_com_erro']} clientes com erro")
gen = s["gerador_ticks_atrasados_pct"] >= 5
estado = "INCONCLUSIVO" if gen and m else "FALHA" if m else "OK"
print(estado + " " + ("; ".join(m) or "-") + (" [gerador saturado]" if gen else ""))
