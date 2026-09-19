"""Acta (MoM) extractiva a partir da transcrição — sem LLM, sem estado."""
import re

_ACTION_KEYWORDS = ("decid", "ficou", "vamos", "próximo", "proximo", "ação", "acao",
                    "tarefa", "responsáv", "responsav", "prazo", "até", "ate ", "todo")


def build_mom(transcript: str) -> str:
    """Ata (MoM) simples e extractiva a partir da transcrição — sem LLM.
    Resumo por tópicos: primeiras frases + linhas com marcadores de ação."""
    text = " ".join(transcript.split())
    if not text:
        return ""
    # Divide em frases de forma tosca mas robusta.
    sentences = [s.strip() for s in re.split(r"(?<=[.!?])\s+", text) if s.strip()]
    actions = [s for s in sentences if any(k in s.lower() for k in _ACTION_KEYWORDS)]
    lines = ["# Ata (gerada automaticamente)", "", "## Resumo"]
    lines += [f"- {s}" for s in sentences[:5]]
    if actions:
        lines += ["", "## Ações / decisões"]
        lines += [f"- {s}" for s in actions[:8]]
    return "\n".join(lines)
