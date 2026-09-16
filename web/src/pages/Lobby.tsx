import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ApiError,
  apiErrorMessage,
  currentUser,
  isAbort,
  joinRoom,
} from "../api";
import { BrandLockup } from "../components/BrandMark";
import { BreakoutRoom, ClientMsg, PeerInfo, Signaling } from "../signaling";
import { Icon } from "../ui/icons";
import {
  Alert,
  Avatar,
  Button,
  Empty,
  Select,
  Spinner,
  StatusBadge,
  Toggle,
  cx,
} from "../ui/kit";
import { BreakoutsCard, type BreakoutsApi } from "../room/BreakoutsCard";
import "../ui/room.css";

type Filtro = "todos" | "maos" | "semSom";

/** Uma linha da tabela: quem está na sala, à porta, ou numa sala paralela. */
type Linha =
  | { kind: "eu"; nome: string }
  | { kind: "sala"; peer: PeerInfo }
  | { kind: "espera"; peer: PeerInfo }
  | { kind: "paralela"; nome: string; sala: BreakoutRoom };

/**
 * Consola do anfitrião (template DelonixModeration): quem está na reunião e
 * onde, a sala de espera, as salas paralelas e as regras da sala.
 *
 * REGRA (R2): esta página NUNCA cria uma chamada — é só sinalização. A media
 * nasce dentro da sala, depois de `joined`. Quem não é dono da sala é mandado
 * para a sala; e mesmo que ficasse, o servidor recusa as acções de anfitrião.
 *
 * Fica FORA do rail da consola (decisão, não esquecimento): a moderação é de
 * UMA sala e abre-se a partir dela, com o código na rota; um destino
 * «Moderação» no rail não teria sala nenhuma para mostrar.
 */
export default function Lobby({ code }: { code: string }) {
  const { t } = useTranslation();
  const signalRef = useRef<Signaling | null>(null);
  const [erro, setErro] = useState("");
  const [connected, setConnected] = useState(false);
  const [roomName, setRoomName] = useState("");
  const [training, setTraining] = useState(false);
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [waiting, setWaiting] = useState<PeerInfo[]>([]);
  const [locked, setLocked] = useState(false);
  const [hostShare, setHostShare] = useState(false);
  const [filtro, setFiltro] = useState<Filtro>("todos");
  /** Quem grava: servidor (`server-recording`) e gravações locais de cada participante (`recording`). */
  const [recServidor, setRecServidor] = useState<string | null>(null);
  const [recLocais, setRecLocais] = useState<Record<string, string>>({});
  const [rooms, setRooms] = useState<BreakoutRoom[]>([]);
  const [endsAt, setEndsAt] = useState<number | null>(null);
  const [minutes, setMinutes] = useState(0);

  useEffect(() => {
    let cancelled = false;
    let signal: Signaling | null = null;
    void (async () => {
      try {
        const { room, room_token } = await joinRoom(code);
        if (cancelled) return;
        if (room.owner_id !== currentUser()?.id) {
          location.hash = `/r/${code}`;
          return;
        }
        setRoomName(room.name);
        setTraining(room.format === "training");
        signal = new Signaling(room_token, code);
        signalRef.current = signal;
        signal.on("joined", (m) => {
          setConnected(true);
          setPeers(m.peers);
        });
        signal.on("peer-joined", (m) =>
          setPeers((p) => [
            ...p.filter((x) => x.peer_id !== m.peer.peer_id),
            m.peer,
          ]),
        );
        signal.on("peer-left", (m) => {
          setPeers((p) => p.filter((x) => x.peer_id !== m.peer_id));
          setRecLocais((r) => {
            if (!(m.peer_id in r)) return r;
            const n = { ...r };
            delete n[m.peer_id];
            return n;
          });
        });
        signal.on("waiting-join", (m) =>
          setWaiting((q) => [
            ...q.filter((x) => x.peer_id !== m.peer.peer_id),
            m.peer,
          ]),
        );
        signal.on("waiting-left", (m) =>
          setWaiting((q) => q.filter((x) => x.peer_id !== m.peer_id)),
        );
        signal.on("room-settings", (m) => {
          setLocked(m.locked);
          setHostShare(m.host_share_only);
        });
        signal.on("media", (m) =>
          setPeers((p) =>
            p.map((x) =>
              x.peer_id === m.from ? { ...x, cam: m.cam, mic: m.mic } : x,
            ),
          ),
        );
        signal.on("hand", (m) =>
          setPeers((p) =>
            p.map((x) => (x.peer_id === m.from ? { ...x, hand: m.raised } : x)),
          ),
        );
        signal.on("peer-role", (m) =>
          setPeers((p) =>
            p.map((x) =>
              x.peer_id === m.peer_id ? { ...x, can_admit: m.can_admit } : x,
            ),
          ),
        );
        signal.on("server-recording", (m) =>
          setRecServidor(m.active ? m.by : null),
        );
        signal.on("recording", (m) =>
          setRecLocais((r) => {
            const n = { ...r };
            if (m.active) n[m.from] = m.username;
            else delete n[m.from];
            return n;
          }),
        );
        signal.on("breakouts-created", (m) => {
          setRooms(m.rooms);
          setEndsAt(m.ends_at);
        });
        signal.on("error", (m) => setErro(m.message));
      } catch (e) {
        // Um 5xx (ou a rede) não tem frase para a pessoa: «Internal Server Error»
        // não se mostra a ninguém. Só um 4xx traz um motivo do servidor.
        if (cancelled || isAbort(e)) return;
        setErro(
          e instanceof ApiError && e.status < 500
            ? apiErrorMessage(e, t("room.lobby.erroLigar"))
            : t("room.lobby.erroLigar"),
        );
      }
    })();
    return () => {
      cancelled = true;
      signal?.close();
    };
  }, [code, t]);

  const send = (msg: ClientMsg) => signalRef.current?.send(msg);
  const me = currentUser();
  const admit = (peerId: string, ok: boolean) => {
    send({ type: ok ? "admit" : "deny", to: peerId });
    setWaiting((q) => q.filter((x) => x.peer_id !== peerId));
  };
  const maos = peers.filter((p) => p.hand).length;
  const semSom = peers.filter((p) => !p.mic).length;
  const emParalelas = rooms.reduce((n, r) => n + r.people.length, 0);
  const gravacao = recServidor
    ? t("room.gravacao.noServidorPor", { nome: recServidor })
    : Object.values(recLocais)[0]
      ? t("room.gravacao.aGravarPor", { nome: Object.values(recLocais)[0] })
      : null;

  const breakouts: BreakoutsApi = {
    rooms,
    endsAt,
    minutes,
    setMinutes,
    create: (count) =>
      send({ type: "breakouts-create", count, minutes: minutes || null }),
    rename: (roomCode, label) =>
      send({ type: "breakout-rename", code: roomCode, label }),
    add: () => send({ type: "breakout-add" }),
    moveUser: (name, roomCode) =>
      send({ type: "breakout-move-user", name, code: roomCode }),
    closeAll: () => send({ type: "breakouts-close" }),
    // Visitar um grupo como na sala: o caminho de volta fica guardado.
    visit: (roomCode) => {
      sessionStorage.setItem(`dx_return_${roomCode}`, code);
      location.hash = `/r/${roomCode}`;
    },
  };

  const linhas = useMemo<Linha[]>(() => {
    const out: Linha[] = [];
    if (filtro === "todos" && connected)
      out.push({ kind: "eu", nome: me?.username ?? "" });
    for (const p of peers) {
      if (filtro === "maos" && !p.hand) continue;
      if (filtro === "semSom" && p.mic) continue;
      out.push({ kind: "sala", peer: p });
    }
    if (filtro === "todos") {
      for (const r of rooms)
        for (const nome of r.people)
          out.push({ kind: "paralela", nome, sala: r });
      for (const w of waiting) out.push({ kind: "espera", peer: w });
    }
    return out;
  }, [filtro, connected, me?.username, peers, rooms, waiting]);

  const papel = (p: PeerInfo) =>
    p.host ? (
      <span className="lb-role is-host">{t("room.papel.anfitriao")}</span>
    ) : p.can_admit ? (
      <span
        className="lb-role is-cohost"
        title={t("room.papel.coAnfitriaoDica")}
      >
        {t("room.papel.coAnfitriao")}
      </span>
    ) : (
      <span className="lb-role">{t("room.lobby.participante")}</span>
    );

  const origem = (p: PeerInfo) =>
    p.is_pstn
      ? t("room.papel.telefone")
      : p.is_bot
        ? t("room.papel.assistente")
        : `${p.mic ? t("room.lobby.micLigado") : t("room.lobby.micDesligado")} · ${p.cam ? t("room.lobby.camLigada") : t("room.lobby.camDesligada")}`;

  return (
    <div className="lb-page">
      <header className="lb-bar">
        <a
          href="#/"
          className="lb-bar__brand"
          aria-label={t("room.lobby.inicio")}
        >
          <BrandLockup size={22} />
        </a>
        <h1 className="lb-bar__title">
          {roomName
            ? t("room.lobby.tituloSala", { nome: roomName })
            : t("room.lobby.titulo")}
        </h1>
        <span className="dx-num dx-muted lb-bar__code">{code}</span>
        {gravacao && (
          <span title={gravacao} role="status">
            <StatusBadge tone="record">{t("room.topo.rec")}</StatusBadge>
            <span className="dx-sr-only">{gravacao}</span>
          </span>
        )}
        <span className="dx-spacer" />
        <Button
          variant="outline"
          icon="micOff"
          aria-label={t("room.lobby.silenciarTodos")}
          disabled={!connected || peers.length === 0}
          onClick={() => send({ type: "mute-all", allow_unmute: true })}
        >
          <span className="lb-hide-narrow">
            {t("room.lobby.silenciarTodos")}
          </span>
        </Button>
        <Button
          variant="primary"
          onClick={() => (location.hash = `/r/${code}`)}
        >
          {t("room.lobby.voltarSessao")}
        </Button>
      </header>

      <main className="lb-body">
        {erro && <Alert tone="danger">{erro}</Alert>}
        {!connected && !erro && (
          <p className="lb-connecting dx-muted" role="status">
            <Spinner /> {t("room.lobby.aLigar")}
          </p>
        )}

        <div className="lb-grid">
          <section className="lb-main" aria-labelledby="lb-part-h">
            <div className="lb-main__head">
              <h2 id="lb-part-h">{t("room.lobby.participantes")}</h2>
              <span className="dx-num dx-muted">
                {t("room.lobby.resumo", {
                  naSala: peers.length + (connected ? 1 : 0),
                  espera: waiting.length,
                })}
                {emParalelas > 0 &&
                  ` · ${t("room.lobby.emParalelas", { count: emParalelas })}`}
              </span>
              <span className="dx-spacer" />
              <div
                className="lb-filters"
                role="group"
                aria-label={t("room.lobby.filtro")}
              >
                {(
                  [
                    ["todos", t("room.lobby.todos")],
                    ["maos", t("room.lobby.maosNoAr", { n: maos })],
                    ["semSom", t("room.lobby.semSom", { n: semSom })],
                  ] as const
                ).map(([v, label]) => (
                  <button
                    key={v}
                    type="button"
                    className={cx("lb-filter", filtro === v && "is-on")}
                    aria-pressed={filtro === v}
                    onClick={() => setFiltro(v)}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>

            <div className="lb-tablecard">
              <div className="dx-table-wrap">
                <table className="lb-table">
                  <thead>
                    <tr>
                      <th scope="col">{t("room.lobby.pessoa")}</th>
                      <th scope="col">{t("room.lobby.papel")}</th>
                      <th scope="col">{t("room.lobby.sala")}</th>
                      <th scope="col">{t("room.lobby.accoes")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {linhas.map((l) => {
                      if (l.kind === "eu")
                        return (
                          <tr key="eu">
                            <td>
                              <span className="lb-person">
                                <Avatar name={l.nome} size={27} />
                                <span>
                                  <strong>{l.nome}</strong>
                                  <small className="dx-num dx-muted">
                                    {t("room.lobby.naConsola")}
                                  </small>
                                </span>
                              </span>
                            </td>
                            <td>
                              <span className="lb-role is-host">
                                {t("room.papel.anfitriao")}
                              </span>
                            </td>
                            <td className="dx-num dx-muted">
                              {t("room.lobby.principal")}
                            </td>
                            <td />
                          </tr>
                        );
                      if (l.kind === "paralela")
                        return (
                          <tr key={`bo-${l.sala.code}-${l.nome}`}>
                            <td>
                              <span className="lb-person">
                                <Avatar name={l.nome} size={27} />
                                <span>
                                  <strong>{l.nome}</strong>
                                </span>
                              </span>
                            </td>
                            <td>
                              <span className="lb-role">
                                {t("room.lobby.participante")}
                              </span>
                            </td>
                            <td className="dx-num dx-muted">{l.sala.label}</td>
                            <td>
                              <div className="lb-actions">
                                <Select
                                  aria-label={t("room.paralelas.moverPessoa", {
                                    nome: l.nome,
                                  })}
                                  value={l.sala.code}
                                  onChange={(e) =>
                                    breakouts.moveUser(l.nome, e.target.value)
                                  }
                                >
                                  {rooms.map((o) => (
                                    <option key={o.code} value={o.code}>
                                      {o.label}
                                    </option>
                                  ))}
                                  <option value={code}>
                                    {t("room.paralelas.principal")}
                                  </option>
                                </Select>
                              </div>
                            </td>
                          </tr>
                        );
                      const p = l.peer;
                      if (l.kind === "espera")
                        return (
                          <tr key={`w-${p.peer_id}`}>
                            <td>
                              <span className="lb-person">
                                <Avatar name={p.username} size={27} />
                                <span>
                                  <strong>{p.username}</strong>
                                  <small className="dx-num dx-muted">
                                    {p.is_pstn
                                      ? t("room.papel.telefone")
                                      : t("room.avisos.querEntrar")}
                                  </small>
                                </span>
                              </span>
                            </td>
                            <td>
                              <span className="lb-role">
                                {t("room.lobby.participante")}
                              </span>
                            </td>
                            <td className="dx-num lb-waiting-cell">
                              {t("room.lobby.emEspera")}
                            </td>
                            <td>
                              <div className="lb-actions">
                                <button
                                  type="button"
                                  className="lb-act"
                                  onClick={() => admit(p.peer_id, true)}
                                >
                                  {t("room.avisos.admitir")}
                                </button>
                                <button
                                  type="button"
                                  className="lb-act"
                                  onClick={() => admit(p.peer_id, false)}
                                >
                                  {t("room.avisos.negar")}
                                </button>
                              </div>
                            </td>
                          </tr>
                        );
                      return (
                        <tr key={p.peer_id}>
                          <td>
                            <span className="lb-person">
                              <Avatar name={p.username} size={27} />
                              <span>
                                <strong>{p.username}</strong>
                                <small className="dx-num dx-muted">
                                  {origem(p)}
                                </small>
                              </span>
                              {p.hand && (
                                <span className="lb-hand">
                                  <Icon name="hand" size={10} />
                                  {t("room.tile.mao")}
                                </span>
                              )}
                            </span>
                          </td>
                          <td>{papel(p)}</td>
                          <td className="dx-num dx-muted">
                            {t("room.lobby.principal")}
                          </td>
                          <td>
                            <div className="lb-actions">
                              {!p.host && (
                                <>
                                  <button
                                    type="button"
                                    className="lb-act"
                                    disabled={!p.mic}
                                    onClick={() =>
                                      send({
                                        type: "force-mute",
                                        to: p.peer_id,
                                      })
                                    }
                                  >
                                    {t("room.lobby.silenciar")}
                                  </button>
                                  <button
                                    type="button"
                                    className="lb-act"
                                    aria-pressed={!!p.can_admit}
                                    onClick={() =>
                                      send({
                                        type: "promote-admit",
                                        to: p.peer_id,
                                        allowed: !p.can_admit,
                                      })
                                    }
                                  >
                                    {p.can_admit
                                      ? t("room.lobby.retirarCoAnfitriao")
                                      : t("room.lobby.tornarCoAnfitriao")}
                                  </button>
                                  <button
                                    type="button"
                                    className="lb-act is-danger"
                                    onClick={() =>
                                      send({ type: "kick", to: p.peer_id })
                                    }
                                  >
                                    {t("room.lobby.remover")}
                                  </button>
                                </>
                              )}
                            </div>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
              {connected && linhas.length <= 1 && filtro === "todos" && (
                <Empty icon="people" title={t("room.lobby.ninguemNaSala")}>
                  {t("room.lobby.ninguemNaSalaTexto")}
                </Empty>
              )}
            </div>
          </section>

          <div className="lb-side">
            <section
              className={cx(
                "lb-card lb-waitcard",
                waiting.length > 0 && "is-busy",
              )}
              aria-labelledby="lb-wait-h"
            >
              <div className="lb-card__head">
                <h2 id="lb-wait-h">{t("room.avisos.salaDeEspera")}</h2>
                <span className="dx-num">
                  {t("room.lobby.aAguardarAprovacao", {
                    count: waiting.length,
                  })}
                </span>
              </div>
              {waiting.length === 0 ? (
                <p className="dx-muted lb-empty">{t("room.lobby.filaVazia")}</p>
              ) : (
                waiting.map((w) => (
                  <div key={w.peer_id} className="lb-queue__row">
                    <Avatar name={w.username} size={27} />
                    <span className="lb-queue__who">
                      <strong>{w.username}</strong>
                      <small className="dx-num dx-muted">
                        {w.is_pstn
                          ? t("room.papel.telefone")
                          : t("room.avisos.querEntrar")}
                      </small>
                    </span>
                    <Button
                      size="sm"
                      variant="primary"
                      onClick={() => admit(w.peer_id, true)}
                    >
                      {t("room.avisos.admitir")}
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={t("room.lobby.negarNome", {
                        nome: w.username,
                      })}
                      onClick={() => admit(w.peer_id, false)}
                    >
                      {t("room.avisos.negar")}
                    </Button>
                  </div>
                ))
              )}
              {waiting.length > 1 && (
                <Button
                  block
                  variant="outline"
                  onClick={() => waiting.forEach((w) => admit(w.peer_id, true))}
                >
                  {t("room.avisos.admitirTodos", { count: waiting.length })}
                </Button>
              )}
            </section>

            {training && (
              <BreakoutsCard
                code={code}
                api={breakouts}
                className="lb-card lb-bocard"
              />
            )}

            <section className="lb-card" aria-labelledby="lb-rules-h">
              <div className="lb-card__head">
                <h2 id="lb-rules-h">{t("room.lobby.regras")}</h2>
              </div>
              <div className="lb-rules">
                <Toggle
                  label={t("room.pessoas.bloquear")}
                  hint={t("room.pessoas.bloquearDica")}
                  checked={locked}
                  disabled={!connected}
                  onChange={(e) => {
                    setLocked(e.target.checked);
                    send({ type: "room-lock", locked: e.target.checked });
                  }}
                />
                <Toggle
                  label={t("room.pessoas.soAnfitriaoPartilha")}
                  hint={t("room.pessoas.soAnfitriaoPartilhaDica")}
                  checked={hostShare}
                  disabled={!connected}
                  onChange={(e) => {
                    setHostShare(e.target.checked);
                    send({ type: "host-share-only", on: e.target.checked });
                  }}
                />
              </div>
            </section>
          </div>
        </div>
      </main>
    </div>
  );
}
