"use strict";

const tauri = window.__TAURI__;
const invoke = (cmd, args) => tauri.core.invoke(cmd, args);

const app = {
  view: "browse",
  filter: "all",
  search: "",
  detail: null,
  status: null,
  mods: [],
  localOnly: [],
  listError: null,
  busy: new Map(),
  packResult: null,
  confirmNative: null,
};

const NET = {
  safe: { label: "Safe for public matches", cls: "good" },
  lobby: { label: "Lobby members need it too", cls: "warn" },
  offline: { label: "Offline only", cls: "info" },
};

const FILTERS = [
  ["all", "All"],
  ["multiplayer", "Multiplayer"],
  ["interface", "Interface"],
  ["fixes", "Fixes"],
  ["offline", "Offline"],
  ["safe", "Safe for public matches"],
];

const TINTS = ["#3A6BFF", "#28B98A", "#F2734B", "#A66BFF", "#E8B427", "#E0507A", "#2BA7C9"];

function el(tag, props, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k === "class") node.className = v;
    else if (k === "text") node.textContent = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else if (k === "vars") for (const [n, val] of Object.entries(v)) node.style.setProperty(n, val);
    else node.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat(Infinity)) {
    if (c === null || c === undefined || c === false) continue;
    node.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return node;
}

const SVG_NS = "http://www.w3.org/2000/svg";
const ICONS = {
  search: ["M11 4a7 7 0 1 1 0 14 7 7 0 0 1 0-14z", "M20 20l-3.5-3.5"],
  box: ["M21 8l-9-5-9 5 9 5 9-5z", "M3 8v8l9 5 9-5V8"],
  down: ["M12 4v12", "M7 11l5 5 5-5", "M5 20h14"],
  plus: ["M12 5v14", "M5 12h14"],
  gear: ["M12 9a3 3 0 1 1 0 6 3 3 0 0 1 0-6z", "M4 12h2M18 12h2M12 4v2M12 18v2M6.3 6.3l1.4 1.4M16.3 16.3l1.4 1.4M6.3 17.7l1.4-1.4M16.3 7.7l1.4-1.4"],
  check: ["M5 12l5 5 9-10"],
  back: ["M15 6l-6 6 6 6"],
  warn: ["M12 4l9 16H3z", "M12 10v4", "M12 17h.01"],
  folder: ["M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"],
  play: [],
};

function icon(name, size = 20, color = "currentColor") {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("width", size);
  svg.setAttribute("height", size);
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  if (name === "play") {
    const p = document.createElementNS(SVG_NS, "path");
    p.setAttribute("d", "M7 4.5v15a1 1 0 0 0 1.5.9l12-7.5a1 1 0 0 0 0-1.8l-12-7.5A1 1 0 0 0 7 4.5z");
    p.setAttribute("fill", color);
    svg.append(p);
    return svg;
  }
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", color);
  svg.setAttribute("stroke-width", "2.2");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  for (const d of ICONS[name] || []) {
    const p = document.createElementNS(SVG_NS, "path");
    p.setAttribute("d", d);
    svg.append(p);
  }
  return svg;
}

function toast(message, kind = "") {
  const t = el("div", { class: `toast ${kind}`, text: message });
  document.getElementById("toasts").append(t);
  setTimeout(() => t.remove(), kind === "error" ? 9000 : 4500);
}

function monogram(name) {
  const words = name.split(/\s+/).filter(Boolean);
  const letters = words.length > 1 ? words[0][0] + words[1][0] : name.slice(0, 2);
  return letters.toUpperCase();
}

function tint(mod) {
  if (mod.color && /^#[0-9a-fA-F]{6}$/.test(mod.color)) return mod.color;
  let h = 0;
  for (const c of mod.id) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return TINTS[h % TINTS.length];
}

const updates = () => app.mods.filter((m) => m.update);
const installed = () => app.mods.filter((m) => m.installed);

async function refreshStatus() {
  try {
    app.status = await invoke("status");
  } catch (e) {
    toast(String(e), "error");
  }
}

async function refreshMods(force) {
  try {
    const list = await invoke("mods", { refresh: force });
    app.mods = list.mods;
    app.localOnly = list.local_only;
    app.listError = null;
  } catch (e) {
    app.listError = String(e);
  }
}

async function reload(force = false) {
  await refreshMods(force);
  await refreshStatus();
  render();
}

async function run(id, label, fn) {
  if (app.busy.has(id)) return;
  app.busy.set(id, label);
  render();
  try {
    await fn();
  } catch (e) {
    toast(String(e), "error");
  } finally {
    app.busy.delete(id);
    await reload(false);
  }
}

function installMod(m) {
  app.confirmNative = null;
  return run(m.id, m.installed ? "Updating" : "Installing", async () => {
    await invoke("install", { id: m.id });
    toast(`${m.name} ${m.installed ? "updated" : "installed"}`, "ok");
  });
}

function uninstallMod(m) {
  return run(m.id, "Removing", async () => {
    await invoke("uninstall", { id: m.id });
    toast(`${m.name} removed`, "ok");
  });
}

function toggleMod(m, enabled) {
  return run(m.id, enabled ? "Enabling" : "Disabling", async () => {
    await invoke("set_enabled", { id: m.id, enabled });
    toast(`${m.name} ${enabled ? "enabled" : "disabled"}`, "ok");
  });
}

const neededByActive = (m) => app.mods.filter((o) => o.installed && !o.disabled && o.needs.includes(m.name)).map((o) => o.name);

function updateAll() {
  return run("*", "Updating", async () => {
    const done = await invoke("update_all");
    toast(done.length ? `Updated ${done.join(", ")}` : "Everything is up to date", "ok");
  });
}

function go(view, detail = null) {
  app.view = view;
  app.detail = detail;
  render();
  document.getElementById("view").focus();
}

function renderNav() {
  const items = [
    ["browse", "Browse", "search", null],
    ["installed", "Installed", "box", el("span", { class: "count", text: installed().length || "" })],
    ["updates", "Updates", "down", updates().length ? el("span", { class: "pill", text: updates().length }) : null],
    ["submit", "Submit a mod", "plus", null],
    ["setup", "Game setup", "gear", app.status && !app.status.ready ? el("span", { class: "pill", text: "!" }) : null],
  ];
  const current = app.view === "detail" ? "browse" : app.view;
  document.getElementById("nav").replaceChildren(
    ...items.map(([id, label, ic, extra]) =>
      el("button", { "aria-current": current === id ? "page" : null, onclick: () => go(id) }, icon(ic), label, extra)
    )
  );
}

function renderGameCard() {
  const s = app.status;
  const card = document.getElementById("game-card");
  if (!s) return card.replaceChildren(el("span", { class: "hint", text: "Looking for the game…" }));
  const build = s.game ? s.game.build_name || s.game.build_id || "" : "";
  let line;
  if (!s.game) line = el("div", { class: "dot-line bad" }, el("span", { class: "dot" }), "Game not found");
  else if (s.game_running) line = el("div", { class: "dot-line info" }, el("span", { class: "dot" }), "Game is running");
  else if (s.ready) line = el("div", { class: "dot-line good" }, el("span", { class: "dot" }), "Mod loader ready");
  else line = el("div", { class: "dot-line warn" }, el("span", { class: "dot" }), "Setup needed");
  card.replaceChildren(
    el("div", { class: "row" }, el("span", { class: "title", text: "Project Playtime" }), el("span", { class: "build", text: build })),
    line,
    el(
      "button",
      {
        class: "btn btn-accent btn-big",
        disabled: !s.game || s.game_running,
        onclick: () => invoke("play").catch((e) => toast(String(e), "error")),
      },
      icon("play", 20, "#1A1405"),
      "PLAY"
    ),
    el("span", { class: "hint", text: "Starts through Steam. Matchmaking works as usual." })
  );
}

function netBadge(network) {
  const n = NET[network] || NET.safe;
  return el("span", { class: `net ${n.cls}` }, el("span", { class: "dot" }), n.label);
}

function nativeBadge(m) {
  return m.native ? el("span", { class: "net info", title: "Runs its own program code through the Playkit loader" }, el("span", { class: "dot" }), "Native code") : null;
}

function actionButton(m, compact) {
  const busy = app.busy.get(m.id) || (app.busy.has("*") && m.update ? "Updating" : null);
  if (busy) return el("button", { class: "btn btn-ghost", disabled: true, text: `${busy}…` });
  const blocked = !app.status?.ready || app.status?.game_running;
  if (!m.compatible) return el("button", { class: "btn btn-ghost", disabled: true, text: "Other game version" });
  if (m.update) return el("button", { class: "btn btn-blue", disabled: blocked, onclick: () => installMod(m), text: compact ? "Update" : `Update to ${m.version}` });
  if (m.installed) {
    if (m.disabled) return el("button", { class: "btn btn-accent", disabled: blocked, onclick: () => toggleMod(m, true), text: "Enable" });
    if (m.broken.length) return el("button", { class: "btn btn-accent", disabled: blocked, onclick: () => installMod(m), text: "Repair" });
    return compact
      ? el("button", { class: "btn btn-ghost", onclick: () => go("detail", m.id) }, icon("check", 16), "Installed")
      : null;
  }
  if (m.native && app.confirmNative !== m.id) {
    return el("button", { class: "btn btn-accent", disabled: blocked, title: "Runs its own program code inside the game", onclick: () => { app.confirmNative = m.id; render(); }, text: "Install" });
  }
  if (m.native) return el("button", { class: "btn btn-blue", disabled: blocked, title: "This mod runs its own program code inside the game. Its source code was reviewed before it was listed.", onclick: () => installMod(m), text: compact ? "Confirm" : "Install native code" });
  return el("button", { class: "btn btn-accent", disabled: blocked, onclick: () => installMod(m), text: "Install" });
}

function modCard(m) {
  return el(
    "article",
    { class: m.disabled ? "card off" : "card" },
    el("div", { class: "card-top", vars: { "--tint": tint(m) } }, el("div", { class: "mono", text: monogram(m.name) }), el("span", { class: "ver", text: m.disabled ? "off" : `v${m.installed || m.version}` })),
    el(
      "div",
      { class: "card-body" },
      el("button", { class: "card-title", onclick: () => go("detail", m.id), text: m.name }),
      el("span", { class: "card-sum", text: m.summary }),
      netBadge(m.network),
      nativeBadge(m),
      el("div", { class: "card-foot" }, el("span", { class: "by", text: `by ${m.authors.join(", ")}` }), actionButton(m, true))
    )
  );
}

function setupBanner() {
  const s = app.status;
  if (!s || s.ready || !s.game) return null;
  return el(
    "div",
    { class: "banner setup" },
    el("div", { class: "banner-icon" }, icon("gear", 24, "#1A1405")),
    el("div", { class: "banner-text" }, el("b", { text: "One step before installing mods" }), el("span", { text: "Playkit needs to set up the mod loader for your game." })),
    el("button", { class: "btn btn-accent", onclick: () => go("setup"), text: "Set up game" })
  );
}

function updateBanner() {
  const ups = updates();
  if (!ups.length) return null;
  return el(
    "div",
    { class: "banner" },
    el("div", { class: "banner-icon" }, icon("down", 24, "#fff")),
    el(
      "div",
      { class: "banner-text" },
      el("b", { text: ups.length === 1 ? "1 update ready" : `${ups.length} updates ready` }),
      el("span", { text: ups.map((m) => `${m.name} ${m.installed} → ${m.version}`).join(" · ") })
    ),
    el("button", { class: "btn btn-blue", disabled: !app.status?.ready || app.busy.has("*"), onclick: updateAll, text: "Update all" })
  );
}

function matches(m) {
  if (m.category === "library" && !m.installed && app.view === "browse") return false;
  const q = app.search.trim().toLowerCase();
  if (q && !`${m.name} ${m.summary} ${m.authors.join(" ")}`.toLowerCase().includes(q)) return false;
  if (app.filter === "all") return true;
  if (app.filter === "safe") return m.network === "safe";
  return m.category === app.filter;
}

function renderList() {
  const titles = { browse: "Browse mods", installed: "Installed", updates: "Updates" };
  let list = app.view === "installed" ? installed() : app.view === "updates" ? updates() : app.mods;
  list = list.filter(matches);
  const search = el(
    "label",
    { class: "search" },
    icon("search", 18),
    el("span", { class: "sr-only", text: "Search mods" }),
    el("input", {
      type: "search",
      placeholder: "Search mods",
      value: app.search,
      oninput: (e) => {
        app.search = e.target.value;
        const grid = document.getElementById("grid");
        if (grid) grid.replaceChildren(...gridItems());
      },
    })
  );
  const gridItems = () => {
    const items = (app.view === "installed" ? installed() : app.view === "updates" ? updates() : app.mods).filter(matches);
    return items.map(modCard);
  };
  const chips = el(
    "div",
    { class: "chips", role: "group", "aria-label": "Filter" },
    FILTERS.map(([id, label]) =>
      el("button", { class: "chip", "aria-pressed": String(app.filter === id), onclick: () => { app.filter = id; render(); } }, id === "safe" ? icon("check", 16) : null, label)
    )
  );
  let body;
  if (app.listError) {
    body = el("div", { class: "empty" }, el("b", { text: "Could not load the mod list" }), el("span", { text: app.listError }), el("button", { class: "btn btn-ghost", onclick: () => reload(true), text: "Try again" }));
  } else if (!list.length) {
    const msg = app.view === "updates" ? "Everything is up to date." : app.view === "installed" ? "No mods installed yet." : "No mods match.";
    body = el("div", { class: "empty" }, el("b", { text: msg }));
  } else {
    body = el("div", { class: "grid", id: "grid" }, list.map(modCard));
  }
  const local = app.view === "installed" && app.localOnly.length
    ? el("div", { class: "panel" }, el("h2", { text: "No longer in the mod list" }), el("ul", {}, app.localOnly.map((l) => el("li", {}, `${l.name} ${l.version} `, el("button", { class: "btn btn-danger", onclick: () => uninstallMod(l), text: "Remove" })))))
    : null;
  return [
    el("header", { class: "page-head" }, el("h1", { text: titles[app.view] }), search),
    setupBanner(),
    app.view !== "installed" ? updateBanner() : null,
    chips,
    body,
    local,
  ];
}

function renderDetail() {
  const m = app.mods.find((x) => x.id === app.detail);
  if (!m) return [el("button", { class: "back", onclick: () => go("browse") }, icon("back", 18), "All mods")];
  const latest = m.changelog.find((c) => c.version === m.version) || m.changelog[0];
  const n = NET[m.network] || NET.safe;
  const netText = {
    safe: ["Public matches stay open.", " You can still matchmake with players who have no mods."],
    lobby: ["Everyone in a private lobby", " needs this mod in the same version."],
    offline: ["Offline only.", " Only used in tutorials or solo play."],
  }[m.network];
  const buildName = app.status?.game?.build_name || app.status?.game?.build_id || "your game";
  const subtitle = [`by ${m.authors.join(", ")}`, m.installed ? `installed ${m.installed}${m.disabled ? ", disabled" : ""}` : null, `latest ${m.version}`].filter(Boolean).join(" · ");
  return [
    el("button", { class: "back", onclick: () => go("browse") }, icon("back", 18), "All mods"),
    el(
      "section",
      { class: "hero", vars: { "--tint": tint(m) } },
      el("div", { class: "mono", text: monogram(m.name) }),
      el("div", { class: "hero-text" }, el("h1", { text: m.name }), el("span", { text: subtitle })),
      el(
        "div",
        { class: "actions" },
        actionButton(m, false),
        m.installed && !m.disabled ? el("button", { class: "btn btn-ghost", disabled: app.busy.has(m.id) || app.status?.game_running || neededByActive(m).length > 0, title: neededByActive(m).length ? `Needed by ${neededByActive(m).join(", ")}` : "Turn it off without removing it", onclick: () => toggleMod(m, false), text: "Disable" }) : null,
        m.installed ? el("button", { class: "btn btn-ghost", disabled: app.busy.has(m.id) || app.status?.game_running || m.needed_by.length > 0, title: m.needed_by.length ? `Needed by ${m.needed_by.join(", ")}` : null, onclick: () => uninstallMod(m), text: "Uninstall" }) : null
      )
    ),
    el(
      "div",
      { class: "split" },
      el(
        "div",
        { class: "main-col" },
        el("p", { class: "prose", text: m.description || m.summary }),
        latest && latest.changes.length ? el("div", { class: "panel" }, el("h2", { text: `What’s new in ${latest.version}` }), el("ul", {}, latest.changes.map((c) => el("li", { text: c })))) : null,
        m.broken.length ? el("div", { class: "panel" }, el("h2", { class: "warn", text: "Changed outside Playkit" }), el("p", { class: "prose", text: "These files no longer match. Press Repair to install them again." }), el("div", { class: "code", text: m.broken.join("\n") })) : null,
        el("h2", { text: "Files it changes" }),
        el("div", { class: "code", text: m.files.map((f) => f.path.replace(/^Playtime_Multiplayer\//, "")).join("\n") }),
        el("span", { class: "hint", text: "Originals are backed up before anything is replaced, and restored when you disable or uninstall." })
      ),
      el(
        "aside",
        { class: "side-col" },
        el(
          "div",
          { class: "panel" },
          el("h2", { text: "Compatibility" }),
          el("div", { class: "check" }, el("span", { class: `check-icon ${n.cls}` }, icon(m.network === "safe" ? "check" : "warn", 16)), el("span", {}, el("b", { text: netText[0] }), netText[1])),
          el(
            "div",
            { class: "check" },
            el("span", { class: `check-icon ${m.compatible ? "good" : "bad"}` }, icon(m.compatible ? "check" : "warn", 16)),
            el("span", {}, el("b", { text: m.compatible ? `Game build ${buildName}` : "Made for another game version." }), m.compatible ? " matches." : " Wait for an update from the author.")
          ),
          m.native
            ? el(
                "div",
                { class: "check" },
                el("span", { class: "check-icon warn" }, icon("warn", 16)),
                el("span", {}, el("b", { text: "Runs its own code." }), " A native mod for the Playkit loader. Its source code was reviewed before it was listed. ", m.source_url ? el("a", { href: "#", onclick: (e) => { e.preventDefault(); invoke("open_url", { url: m.source_url }); }, text: "Read the source" }) : null)
              )
            : null,
          m.needs.length
            ? el("div", { class: "check" }, el("span", { class: "check-icon info" }, icon("box", 16)), el("span", {}, el("b", { text: `Also installs ${m.needs.join(", ")}` }), ". Shared parts other mods use too. Removed again when nothing needs them."))
            : null,
          m.needed_by.length
            ? el("div", { class: "check" }, el("span", { class: "check-icon info" }, icon("box", 16)), el("span", {}, el("b", { text: `Used by ${m.needed_by.join(", ")}` }), m.auto ? ". Installed automatically." : "."))
            : null,
          m.shares_with.length
            ? el("div", { class: "check" }, el("span", { class: "check-icon info" }, icon("check", 16)), el("span", {}, el("b", { text: "Combined automatically" }), ` with ${m.shares_with.join(", ")}. Both change the same files, Playkit merges them.`))
            : null
        ),
        el(
          "div",
          { class: "panel facts" },
          el("div", {}, el("span", { text: "Version" }), el("span", { text: m.version })),
          el("div", {}, el("span", { text: "Category" }), el("span", { text: m.category })),
          el("div", {}, el("span", { text: "Files" }), el("span", { text: String(m.files.length) })),
          el("div", {}, el("span", { text: "Download" }), el("span", { text: formatSize(m.files.reduce((a, f) => a + f.size, 0)) })),
          m.homepage && m.homepage.startsWith("https://") ? el("a", { href: "#", onclick: (e) => { e.preventDefault(); invoke("open_url", { url: m.homepage }); }, text: "Mod homepage" }) : null
        )
      )
    ),
  ];
}

function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

async function chooseGameDir() {
  const dir = await tauri.dialog.open({ directory: true, title: "Choose the Project Playtime folder" });
  if (!dir) return;
  try {
    await invoke("set_game_dir", { path: dir });
    toast("Game folder saved", "ok");
  } catch (e) {
    toast(String(e), "error");
  }
  await reload(false);
}

function step(n, state, title, ...body) {
  const cls = state === "done" ? "done" : state === "todo" ? "todo" : "";
  return el("li", { class: `step ${state === "todo" ? "active" : ""}` }, el("span", { class: `step-num ${cls}` }, state === "done" ? icon("check", 20, "#0E2A1B") : String(n)), el("div", { class: "step-body" }, el("h3", { text: title }), body));
}

function lastRunInfo(run) {
  if (!run) return el("p", { class: "warn", text: "The loader has not run yet. Start the game once, then come back here. If this stays, the game does not load dxgi.dll." });
  const when = new Date(run.modified * 1000).toLocaleString();
  const files = run.loose_files == null ? "" : ` and found ${run.loose_files} mod files`;
  const ok = !run.problems.length;
  return el(
    "details",
    { class: "lastrun" },
    el("summary", { class: ok ? "good" : "warn", text: ok ? `Last game start (${when}): the loader ran${files}.` : `Last game start (${when}): the loader ran with problems.` }),
    el("div", { class: "code", text: run.log })
  );
}

function renderSetup() {
  const s = app.status;
  if (!s) return [el("h1", { text: "Game setup" })];
  const game = s.game;
  const dll = s.loader?.dll;
  const lo = s.loader?.launch_option;
  const ac = s.loader?.anti_cheat;
  const linux = s.platform.os === "linux";
  const osName = s.platform.steam_deck ? "Steam Deck" : s.platform.name;
  const dllText = {
    missing: "Not installed yet.",
    current: "Installed and up to date.",
    outdated: "An older Playkit loader is installed. Patch the game to update it.",
    foreign: "Another loose-file loader is installed and works. Patch game replaces it with the Playkit loader and keeps the old one as a backup.",
  }[dll] || "";
  const launch = !linux
    ? null
    : step(
        ac && ac !== "absent" ? 4 : 3,
        lo?.state === "ok" ? "done" : "todo",
        "Steam launch option",
        el("p", { text: "On Linux the game runs through Proton, which needs to be told to load the mod loader." }),
        lo?.state === "ok"
          ? el("p", { class: "good", text: "Already set." })
          : lo?.state === "missing"
            ? [
                el("p", { text: "Playkit adds this to the game’s launch options. Your existing options stay." }),
                el("div", { class: "inline" }, el("code", { class: "path", text: lo.suggested }), el("button", { class: "btn btn-ghost", onclick: () => navigator.clipboard.writeText(lo.suggested).then(() => toast("Copied", "ok")), text: "Copy" })),
                s.steam_running ? el("p", { class: "warn", text: "Steam is running. Close Steam completely and press Patch game, or paste the line yourself: Steam › Project Playtime › Properties › Launch options." }) : null,
              ]
            : el("p", { class: "warn", text: "Could not read your Steam settings. Set the launch option by hand: WINEDLLOVERRIDES=\"dxgi=n,b\" %command%" })
      );
  return [
    el("header", { class: "page-head" }, el("h1", { text: s.ready ? "Your game is ready" : "Let’s get your game ready" })),
    el("p", { class: "lede", text: `Detected ${osName}. Nothing is changed until you press Patch game. Your game still starts and matchmakes normally through Steam.` }),
    el(
      "ol",
      { class: "steps" },
      step(
        1,
        game ? "done" : "todo",
        game ? "Game found" : "Find the game",
        game
          ? [el("div", { class: "inline" }, el("span", { class: "path", text: game.root }), el("button", { class: "btn btn-ghost", onclick: chooseGameDir, text: "Change" }), game.custom_dir ? el("button", { class: "btn btn-ghost", onclick: async () => { await invoke("set_game_dir", { path: null }); reload(false); }, text: "Auto-detect" }) : null), el("span", { class: "hint", text: `Build ${game.build_name || game.build_id || "unknown"}` })]
          : [el("p", { text: s.game_error || "Project Playtime was not found." }), el("button", { class: "btn btn-accent", onclick: chooseGameDir }, icon("folder", 18, "#1A1405"), "Choose folder")]
      ),
      step(2, dll === "current" || dll === "foreign" ? "done" : game ? "todo" : "", "Mod loader", el("p", { text: "A small loader next to the game lets it read mod files from its Content folder." }), dll ? el("p", { class: dll === "current" ? "good" : dll === "foreign" ? "info" : "warn", text: dllText }) : null, dll === "current" ? lastRunInfo(s.loader.last_run) : null),
      ac && ac !== "absent"
        ? step(3, ac === "patched" ? "done" : "todo", "Easy Anti-Cheat settings", el("p", { text: "Playkit sets the product, sandbox and deployment IDs in EasyAntiCheat/Settings.json to 123. The original file is kept as a backup." }), ac === "patched" ? el("p", { class: "good", text: "Already set." }) : null)
        : null,
      launch
    ),
    el(
      "div",
      { class: "inline" },
      el("button", { class: "btn btn-accent btn-big", disabled: !game || s.game_running || app.busy.has("setup"), onclick: patchGame, text: app.busy.has("setup") ? "Patching…" : "Patch game" }),
      s.game_running ? el("span", { class: "warn", text: "Close the game first." }) : null,
      dll && dll !== "missing" ? el("button", { class: "btn btn-danger", disabled: s.game_running, onclick: removeLoader, text: "Remove loader" }) : null
    ),
    el(
      "div",
      { class: "panel" },
      el("h2", { text: "Mod list source" }),
      el("p", { class: "hint", text: "Only change this if you run your own mod list." }),
      el(
        "div",
        { class: "inline" },
        el("input", { class: "field", id: "index-url", value: s.index_url, "aria-label": "Mod list URL" }),
        el("button", { class: "btn btn-ghost", onclick: async () => { await invoke("set_index_url", { url: document.getElementById("index-url").value }); reload(true); }, text: "Save" }),
        el("button", { class: "btn btn-ghost", onclick: async () => { await invoke("set_index_url", { url: null }); reload(true); }, text: "Default" })
      )
    ),
    el("span", { class: "hint", text: `Playkit ${s.app_version}` }),
  ];
}

function patchGame() {
  return run("setup", "Patching", async () => {
    const r = await invoke("setup");
    if (r.launch_option_error) toast(r.launch_option_error, "error");
    else toast("Game patched", "ok");
  });
}

function removeLoader() {
  return run("setup", "Removing", async () => {
    await invoke("remove_loader");
    toast("Loader removed", "ok");
  });
}

async function packFolder() {
  const dir = await tauri.dialog.open({ directory: true, title: "Choose your mod folder" });
  if (!dir) return;
  await run("pack", "Packing", async () => {
    app.packResult = await invoke("pack", { src: dir });
    toast(`Packed ${app.packResult.id}`, "ok");
  });
}

const MOD_TOML = `id          = "my-mod"
name        = "My Mod"
version     = "1.0.0"
authors     = ["your name"]
summary     = "One sentence about what it does."
category    = "fixes"        # multiplayer, interface, fixes, offline, other
network     = "safe"         # safe, lobby, offline
game_builds = ["12576441"]`;

function renderSubmit() {
  const r = app.packResult;
  const checks = [
    ["Manifest is valid", "Name, version and game build are filled in."],
    ["Only game content", "Changed files inside the Content folder, Blueprint patches (.bps), or one native mod DLL with its source code."],
    ["Patches, not game files", "Playkit stores only the difference to the original game files, so no game content is uploaded."],
    ["Reviewed by a maintainer", "Including the network badge: safe for public matches, or lobby members need it."],
  ];
  return [
    el("header", { class: "page-head" }, el("h1", { text: "Share your mod" })),
    el("p", { class: "lede", text: "Put a mod.toml next to a Playtime_Multiplayer folder that holds your changed files, exactly where they would go in the game. A .bps file next to an asset (UI_Foo.bps for UI_Foo.uasset) is applied to the original as a Blueprint patch. Playkit turns it all into small patches you can send in." }),
    el(
      "div",
      { class: "split" },
      el(
        "div",
        { class: "main-col" },
        el(
          "div",
          { class: "drop" },
          icon("folder", 40, "#FFC531"),
          el("b", { text: "Pack your mod folder" }),
          el("span", { class: "hint", text: "mod.toml + Playtime_Multiplayer/Content/…" }),
          el("button", { class: "btn btn-ghost", disabled: app.busy.has("pack") || !app.status?.game, onclick: packFolder, text: app.busy.has("pack") ? "Packing…" : "Choose folder" })
        ),
        r
          ? el(
              "div",
              { class: "panel" },
              el("h2", { text: `${r.id} is packed` }),
              el("div", { class: "code", text: r.files.map((f) => `${f.base_sha256 ? "patch" : "file "}  ${f.path}`).join("\n") }),
              r.unchanged.length ? el("span", { class: "hint", text: `${r.unchanged.length} unchanged file(s) left out.` }) : null,
              r.sources.length ? el("span", { class: "hint", text: `Source included for review: ${r.sources.join(", ")}` }) : null,
              el(
                "div",
                { class: "inline" },
                el("button", { class: "btn btn-accent", onclick: () => invoke("open_url", { url: r.issue_url }), text: "Send as issue" }),
                el("button", { class: "btn btn-ghost", onclick: () => invoke("reveal", { path: r.zip }), text: "Show zip" }),
                el("button", { class: "btn btn-ghost", onclick: () => invoke("open_url", { url: r.pull_request_url }), text: "Pull request guide" })
              ),
              el("span", { class: "hint", text: "Attach the zip to the issue. A maintainer reviews it and adds it to the list." })
            )
          : null,
        el("h2", { text: "mod.toml" }),
        el("div", { class: "code", text: MOD_TOML }),
        el("div", { class: "inline" }, el("button", { class: "btn btn-ghost", onclick: () => navigator.clipboard.writeText(MOD_TOML).then(() => toast("Copied", "ok")), text: "Copy template" }))
      ),
      el(
        "aside",
        { class: "side-col" },
        el("div", { class: "panel" }, el("h2", { text: "Checked before it goes live" }), checks.map(([t, d]) => el("div", { class: "check" }, el("span", { class: "check-icon good" }, icon("check", 16)), el("span", {}, el("b", { text: t }), el("br"), d))))
      )
    ),
  ];
}

function render() {
  renderNav();
  renderGameCard();
  const view = document.getElementById("view");
  const content =
    app.view === "detail" ? renderDetail() : app.view === "setup" ? renderSetup() : app.view === "submit" ? renderSubmit() : renderList();
  view.replaceChildren(...content.flat().filter(Boolean));
}

const MAX_ICON = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="1.8" y="1.8" width="8.4" height="8.4" rx="2" fill="none" stroke="currentColor" stroke-width="1.6"/></svg>';
const RESTORE_ICON = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="1.4" y="3.6" width="7" height="7" rx="1.6" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M3.8 1.6h5a1.6 1.6 0 0 1 1.6 1.6v5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>';

function windowControls() {
  const win = tauri.window.getCurrentWindow();
  const max = document.getElementById("win-max");
  const sync = async () => {
    const maximized = await win.isMaximized();
    max.innerHTML = maximized ? RESTORE_ICON : MAX_ICON;
    max.setAttribute("aria-label", maximized ? "Restore" : "Maximize");
  };
  document.getElementById("win-min").onclick = () => win.minimize();
  max.onclick = () => win.toggleMaximize();
  document.getElementById("win-close").onclick = () => win.close();
  win.onResized(sync);
  sync();
}

async function start() {
  windowControls();
  tauri.event.listen("progress", (e) => {
    const { id, done, total } = e.payload;
    if (app.busy.has(id)) {
      app.busy.set(id, `${app.busy.get(id).split(" ")[0]} ${done}/${total}`);
      render();
    }
  });
  render();
  await refreshStatus();
  if (app.status && (!app.status.ready || !app.status.game)) app.view = "setup";
  render();
  await reload(false);
  setInterval(async () => {
    const before = JSON.stringify([app.status?.game_running, app.status?.steam_running, app.status?.ready]);
    await refreshStatus();
    if (JSON.stringify([app.status?.game_running, app.status?.steam_running, app.status?.ready]) !== before) render();
  }, 5000);
}

start();
