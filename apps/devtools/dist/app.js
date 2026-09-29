"use strict";
// Syrup devtools: what Syrup sees, believes, knows and says, live.
// Served by `syrup live --devtools` / `syrup replay --devtools` on 127.0.0.1.
const $ = (id) => {
    const el = document.getElementById(id);
    if (!el)
        throw new Error(`missing #${id}`);
    return el;
};
function esc(s) {
    return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}
function pct(v) {
    return v == null ? "–" : `${Math.round(v * 100)}%`;
}
function place(n) {
    const cx = n.x + n.w / 2, cy = n.y + n.h / 2;
    const v = cy < 0.33 ? "top" : cy > 0.66 ? "bottom" : "middle";
    const h = cx < 0.33 ? "left" : cx > 0.66 ? "right" : "centre";
    return `${v} ${h}`;
}
function value(c) {
    if (c.value == null)
        return esc(c.text ?? "–");
    if (c.unit === "fraction")
        return c.max ? `${pct(c.value)} (${Math.round(c.value * c.max)}/${c.max})` : pct(c.value);
    if (c.unit === "seconds")
        return `${c.value.toFixed(0)} s`;
    return c.max ? `${c.value}/${c.max}` : String(c.value);
}
function skillLabel(s) {
    const n = s.alpha + s.beta - 2;
    if (n < 4)
        return "not enough seen yet";
    const m = s.alpha / (s.alpha + s.beta);
    const word = m >= 0.75 ? "strong" : m >= 0.55 ? "solid" : m >= 0.35 ? "developing" : "struggling";
    return `${word} · ${pct(m)} · ${pct(n / (n + 6))} sure`;
}
async function post(what, body) {
    await fetch(`/api/${what}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
}
async function feedback(a, kind) {
    await post("feedback", { advice_id: a.id, topic: a.topic, kind });
}
const MODES = ["hidden", "minimal", "normal", "analysis"];
function renderSyrup(s) {
    document.getElementById("overlay").src = `/api/overlay.png?t=${s.t_ms}`;
    const current = s.view?.mode ?? "normal";
    const modes = $("modes");
    if (modes.dataset.mode === current)
        return;
    modes.dataset.mode = current;
    modes.innerHTML = "";
    for (const m of MODES) {
        const b = document.createElement("button");
        b.textContent = m;
        if (m === current)
            b.className = "on";
        b.onclick = () => void post("mode", { mode: m });
        modes.appendChild(b);
    }
}
function onSubmit(id, field, send) {
    $(id).addEventListener("submit", (e) => {
        e.preventDefault();
        const input = $(field);
        const value = input.value.trim();
        if (value)
            void send(value).then(() => { input.value = ""; });
    });
}
onSubmit("ask", "topic", (topic) => post("research", { topic }));
onSubmit("confirm", "title", (title) => post("confirm", { title }));
function renderGame(s) {
    const g = s.game;
    const head = g
        ? `<b>${esc(g.title)}</b> <span class="muted">${esc(g.game_id)}</span> ${g.confirmed ? '<span class="tag">confirmed</span>' : ""} ${pct(g.confidence)}`
        : '<span class="muted">not sure which game this is yet</span>';
    const ev = (g?.evidence ?? []).map((e) => `<li>${esc(e.signal)}: ${esc(e.detail)} <span class="muted">${pct(e.weight)}</span></li>`).join("");
    const others = s.candidates.slice(1, 4).map((c) => `${esc(c.title)} ${pct(c.confidence)}`).join(" · ");
    $("game").innerHTML = `${head}<ul>${ev}</ul>${others ? `<div class="muted">also considered: ${others}</div>` : ""}
    <div class="muted">plugin: ${esc(s.plugin ?? "none")} · OCR: ${esc(s.ocr)} · research: ${s.research ? "on" : "off"}</div>`;
    $("status").textContent = `${g ? g.title : "?"} · ${s.scene ? `${s.scene.kind} (${pct(s.scene.confidence)})` : ""} · ${s.fps.captured.toFixed(1)} fps captured, ${s.fps.analysed.toFixed(1)} analysed`;
}
function renderConcepts(s) {
    const rows = Object.values(s.state.concepts)
        .sort((a, b) => b.confidence - a.confidence)
        .map((c) => `<tr><td><b>${esc(c.name)}</b></td><td>${value(c)}</td><td>${pct(c.confidence)}</td><td>${esc(c.reliability)}</td><td>${esc(c.trend)}</td><td class="muted">${esc(c.source)}</td><td class="small">${c.evidence.map(esc).join("; ")}</td></tr>`)
        .join("");
    $("concepts").innerHTML = rows ? `<table><tr><th>concept</th><th>value</th><th>sure</th><th>how read</th><th>trend</th><th>from</th><th>why</th></tr>${rows}</table>` : '<span class="muted">nothing yet</span>';
    const hyp = s.state.hypotheses.map((h) => `<li>${esc(h.statement)} <span class="muted">${pct(h.confidence)}</span></li>`).join("");
    const unsure = s.uncertainties.map((u) => `<li class="muted">${esc(u)}</li>`).join("");
    $("hypotheses").innerHTML = `<ul>${hyp}${unsure}</ul>`;
    const act = s.state.activity;
    $("activity").textContent = `scene ${s.state.scene} · intensity ${pct(act.intensity)} · motion ${pct(act.motion)} · idle ${(act.idle_ms / 1000).toFixed(0)} s · ${s.regions} interface regions`;
}
function renderAdvice(s) {
    const list = $("advice");
    list.innerHTML = "";
    for (const r of s.advice.slice().reverse().slice(0, 25)) {
        const li = document.createElement("li");
        li.className = r.shown ? "shown" : "suppressed";
        const a = r.advice;
        li.innerHTML = `<div><b>${esc(a.text)}</b> <span class="tag">${esc(a.kind)}</span> <span class="tag">${esc(a.urgency)}</span> <span class="muted">${pct(a.confidence)} · ${esc(a.origin)}</span></div>
      <div class="small muted">why: ${a.why.map(esc).join("; ")}${r.reason ? ` · <i>not said: ${esc(r.reason)}</i>` : ""}</div>`;
        if (r.shown) {
            const row = document.createElement("div");
            for (const [kind, label] of [["useful", "👍"], ["wrong", "👎"], ["explain", "❓"], ["stop_suggesting", "🔇"], ["research", "🔎"]]) {
                const b = document.createElement("button");
                b.textContent = label;
                b.title = kind;
                b.onclick = () => void feedback(a, kind);
                row.appendChild(b);
            }
            li.appendChild(row);
        }
        list.appendChild(li);
    }
}
function renderPlayer(s) {
    const p = s.player;
    if (!p) {
        $("player").innerHTML = '<span class="muted">no game yet</span>';
        return;
    }
    const skills = Object.entries(p.skills).map(([k, v]) => `<li>${esc(k.replace(/_/g, " "))}: ${skillLabel(v)}</li>`).join("");
    const habits = Object.entries(p.habits).map(([k, v]) => `<li>${esc(k)} ×${v}</li>`).join("");
    const muted = p.muted_topics.map(esc).join(", ");
    $("player").innerHTML = `<div>${p.deaths} deaths · ${p.wins} wins</div><ul>${skills}</ul>${habits ? `<div>habits</div><ul>${habits}</ul>` : ""}${muted ? `<div class="muted">muted: ${muted}</div>` : ""}`;
}
function renderKnowledge(s) {
    const facts = s.facts.slice(0, 30).map((f) => `<li>${esc(f.claim)} <span class="tag">${esc(f.kind.replace(/_/g, " "))}</span>${f.stale ? '<span class="tag warn">old version</span>' : ""}${f.spoiler !== "none" ? `<span class="tag warn">spoiler: ${esc(f.spoiler)}</span>` : ""}
    <div class="small muted"><a href="${esc(f.source.url)}" target="_blank" rel="noreferrer">${esc(f.source.title)}</a> · ${pct(f.confidence)}${f.game_version ? ` · v${esc(f.game_version)}` : ""}</div></li>`).join("");
    const asked = s.researched.slice(-6).map((r) => `<li class="small">${esc(r.question)}: ${r.facts_added} new facts${r.note ? ` <span class="muted">(${esc(r.note)})</span>` : ""}</li>`).join("");
    $("knowledge").innerHTML = `<ul>${facts || '<li class="muted">nothing looked up yet</li>'}</ul><div>research</div><ul>${asked}</ul>`;
}
function renderProfile(s) {
    // Not while the player is typing a correction.
    const typing = document.activeElement;
    if (typing instanceof HTMLInputElement && $("profile").contains(typing))
        return;
    const p = s.profile;
    if (!p) {
        $("profile").innerHTML = '<span class="muted">no profile yet</span>';
        return;
    }
    const els = p.elements
        .filter((e) => e.seen > 3 || e.concept)
        .slice(0, 20)
        .map((e) => `<tr><td>#${e.id}</td><td>${esc(e.kind)}</td><td>${esc(place(e.norm))}</td><td>${esc(e.concept ?? "")}${e.corrected ? " ✓" : ""}</td><td>${e.seen}</td><td>${pct(e.confidence)}</td>
      <td><input class="small" size="9" placeholder="it is…" data-el="${e.id}" title="tell Syrup what this is (empty: nothing)"></td></tr>`)
        .join("");
    const terms = p.terms.slice(0, 16).map(([t, n]) => `${esc(t)} (${n})`).join(", ");
    $("profile").innerHTML = `<div><b>${esc(p.title)}</b> · ${p.genres.map(esc).join(", ") || "genres unknown"} · hat: ${esc(p.hat.replace(/_/g, " "))} · ${p.sessions} sessions · ${p.observed_min.toFixed(0)} min watched${p.version ? ` · v${esc(p.version)}` : ""}</div>
    <table><tr><th></th><th>kind</th><th>where</th><th>is</th><th>seen</th><th>sure</th><th></th></tr>${els}</table><div class="small muted">words: ${terms}</div>`;
    for (const input of Array.from(document.querySelectorAll("input[data-el]"))) {
        const el = p.elements.find((e) => String(e.id) === input.dataset.el);
        if (!el)
            continue;
        input.onkeydown = (ev) => {
            if (ev.key !== "Enter")
                return;
            void post("correct", { norm: el.norm, kind: el.kind, concept: input.value.trim().toLowerCase().replace(/\s+/g, "_") });
            input.value = "";
        };
    }
}
function renderTimings(s) {
    $("timings").innerHTML = Object.entries(s.timings).map(([k, v]) => `<span class="tag">${esc(k)} ${v.toFixed(1)} ms</span>`).join(" ");
    $("texts").innerHTML = s.texts.slice(0, 30).map((t) => `<span class="tag">${esc(t)}</span>`).join(" ");
}
let lastSeq = 0;
const events = [];
function summarize(e) {
    const { type, ...rest } = e;
    const short = JSON.stringify(rest);
    return `${type} ${short.length > 160 ? short.slice(0, 160) + "…" : short}`;
}
async function pollEvents() {
    const res = await fetch(`/api/events?since=${lastSeq}`);
    const got = await res.json();
    for (const r of got) {
        lastSeq = Math.max(lastSeq, r.seq);
        if (r.event.type !== "frame_captured")
            events.push(r);
    }
    while (events.length > 80)
        events.shift();
    $("events").innerHTML = events.slice().reverse().map((r) => `<li class="small"><span class="muted">#${r.seq}</span> ${esc(summarize(r.event))}</li>`).join("");
}
async function tick() {
    try {
        const s = await (await fetch("/api/snapshot")).json();
        renderSyrup(s);
        renderGame(s);
        renderConcepts(s);
        renderAdvice(s);
        renderPlayer(s);
        renderKnowledge(s);
        renderProfile(s);
        renderTimings(s);
        await pollEvents();
        document.getElementById("frame").src = `/api/frame.png?t=${s.t_ms}`;
        $("offline").style.display = "none";
    }
    catch {
        $("offline").style.display = "block";
    }
}
setInterval(() => void tick(), 1000);
void tick();
