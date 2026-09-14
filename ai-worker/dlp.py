"""DLP (redação de dados sensíveis) — espelha server/src/dlp.rs.

Duplicado de propósito: o worker é Python e o servidor é Rust, sem lib
partilhada entre os dois. Os PADRÕES têm de se manter em sincronia à mão —
qualquer mudança num lado pede a mesma mudança no outro.

Sem isto, o `transcribe_worker.py` escrevia a transcrição de uma gravação
directamente na base de dados a partir do áudio bruto, sem nunca passar
pelo DLP: um cartão de crédito dito durante a reunião ficava gravado tal
e qual (achado ao alinhar com o OWASP Top 10 para LLM — LLM06, divulgação
de informação sensível).
"""
import re

CREDIT_CARD_RE = re.compile(r"(?:\d[ -]*?){13,16}")
NIF_RE = re.compile(r"\b[1-9]\d{8}\b")
API_KEY_RE = re.compile(r"sk-[a-zA-Z0-9]{32}")

PROFANITY_RE = re.compile(
    r"\b(?:merda|caralho|foda(?:-se)?|fode(?:-te)?|cabr[ãa]o|puta|putas|"
    r"filho da puta|foda|porra|cona|badalhoc[oa]|corno|otári[oa]|"
    r"fuck(?:ing|ed|er)?|shit|bitch|asshole|bastard|cunt|dick(?:head)?|"
    r"motherfucker)\b",
    re.IGNORECASE,
)


def mask_profanity(text: str) -> str:
    """Substitui palavras ofensivas por asteriscos do mesmo comprimento."""
    return PROFANITY_RE.sub(lambda m: "*" * len(m.group(0)), text)


def censor(text: str) -> str:
    """Redige cartão de crédito, NIF e chaves de API."""
    text = CREDIT_CARD_RE.sub("[CARTÃO DE CRÉDITO BLOQUEADO PELO DLP]", text)
    text = NIF_RE.sub("[NIF BLOQUEADO PELO DLP]", text)
    text = API_KEY_RE.sub("[CHAVE API CENSURADA]", text)
    return text


def clean_caption(text: str) -> str:
    """Limpeza completa: DLP (PII) + máscara de palavrões."""
    return mask_profanity(censor(text))


if __name__ == "__main__":
    # Sem pytest no repo (nenhum ficheiro `test_*.py` existe) — casos
    # espelhados dos testes de `dlp.rs`, corridos com `python3 dlp.py`.
    assert mask_profanity("isto é uma merda pegada") == "isto é uma ***** pegada"
    assert mask_profanity("that is pure shit man") == "that is pure **** man"
    assert mask_profanity("that is bullshit") == "that is bullshit"
    assert mask_profanity("MERDA total") == "***** total"
    out = clean_caption("paga com o cartão 1234 5678 1234 5678 seu merda")
    assert "[CARTÃO DE CRÉDITO BLOQUEADO PELO DLP]" in out
    assert "*****" in out and "merda" not in out
    assert censor("O meu cartão é 1234 5678 1234 5678, usa-o bem.") == (
        "O meu cartão é [CARTÃO DE CRÉDITO BLOQUEADO PELO DLP], usa-o bem."
    )
    assert censor("Fatura para o NIF 234567890 por favor.") == (
        "Fatura para o NIF [NIF BLOQUEADO PELO DLP] por favor."
    )
    assert censor(
        "A chave secreta é sk-abcdef1234567890abcdef1234567890 não partilhes."
    ) == "A chave secreta é [CHAVE API CENSURADA] não partilhes."
    assert censor("Olá, tudo bem? 12345") == "Olá, tudo bem? 12345"
    print("dlp.py: todos os testes passaram")
