// Mello library admin. Talks to /api with the admin key kept in localStorage.
// All server data is inserted with textContent, never as HTML.
"use strict";

const KEY_STORE = "mello-api-key";
const $ = (id) => document.getElementById(id);

function storedKey() {
  try { return localStorage.getItem(KEY_STORE) || ""; } catch { return ""; }
}
function storeKey(key) {
  try { key ? localStorage.setItem(KEY_STORE, key) : localStorage.removeItem(KEY_STORE); } catch { /* private mode */ }
}

function status(text) { $("status").textContent = text; }

async function api(method, path) {
  const response = await fetch("/api" + path, {
    method,
    headers: { Authorization: "Bearer " + storedKey() },
    cache: "no-store",
  });
  if (response.status === 401) {
    storeKey("");
    show(false);
    throw new Error("That key was not accepted.");
  }
  const body = response.status === 204 ? null : await response.json().catch(() => null);
  if (!response.ok) throw new Error((body && body.error) || response.statusText);
  return body;
}

function item(label, meta, buttonText, onClick) {
  const li = document.createElement("li");
  const text = document.createElement("div");
  text.textContent = label;
  if (meta) {
    const small = document.createElement("div");
    small.className = "meta";
    small.textContent = meta;
    text.append(small);
  }
  li.append(text);
  if (buttonText) {
    const button = document.createElement("button");
    button.textContent = buttonText;
    button.className = buttonText === "Remove" ? "secondary" : "";
    button.addEventListener("click", async () => {
      button.disabled = true;
      try { await onClick(); } catch (e) { status(e.message); button.disabled = false; }
    });
    li.append(button);
  }
  return li;
}

function empty(list, text) {
  const li = document.createElement("li");
  li.className = "empty";
  li.textContent = text;
  list.replaceChildren(li);
}

let inLibrary = new Set();

async function loadLibrary() {
  const lib = await api("GET", "/library");
  const list = $("library");
  const rows = [];
  inLibrary = new Set();
  for (const [kind, map] of [["movie", lib.movies || {}], ["tv", lib.shows || {}]]) {
    for (const [id, entry] of Object.entries(map)) {
      inLibrary.add(kind + ":" + id);
      rows.push({ kind, id, title: entry.title || "TMDB " + id, added: entry.added_at });
    }
  }
  rows.sort((a, b) => (b.added || "").localeCompare(a.added || ""));
  if (!rows.length) return empty(list, "Nothing yet. Search above to add a title.");
  list.replaceChildren(...rows.map((row) =>
    item(row.title, row.kind === "tv" ? "Show" : "Movie", "Remove", async () => {
      await api("DELETE", `/library/${row.kind}/${row.id}`);
      status(`Removed ${row.title}.`);
      await loadLibrary();
    })));
}

async function search(q) {
  const list = $("results");
  empty(list, "Searching…");
  const { results } = await api("GET", "/search?q=" + encodeURIComponent(q));
  if (!results.length) return empty(list, "No matches.");
  list.replaceChildren(...results.slice(0, 12).map((r) => {
    const have = inLibrary.has(r.media_type + ":" + r.id);
    const meta = [r.media_type === "tv" ? "Show" : "Movie", r.year].filter(Boolean).join(" · ");
    return item(r.title, meta, have ? null : "Add", async () => {
      await api("POST", `/library/${r.media_type}/${r.id}`);
      status(`Added ${r.title}. Jellyfin will show it after the next scan.`);
      await loadLibrary();
      await search(q);
    });
  }));
}

async function refreshStatus() {
  const s = await api("GET", "/status");
  const sync = s.library.last_sync || {};
  const when = sync.finished_at ? new Date(sync.finished_at).toLocaleString() : "not yet";
  const issues = (sync.failures || []).length;
  status(`${s.library.movies} movies, ${s.library.shows} shows · last sync ${when}` +
    (issues ? ` · ${issues} issue(s)` : "") + (s.tmdb_configured ? "" : " · TMDB key missing"));
}

function show(loggedIn) {
  $("login").hidden = loggedIn;
  $("app").hidden = !loggedIn;
  if (!loggedIn) status("Enter the API key to manage the library.");
}

async function start() {
  if (!storedKey()) return show(false);
  show(true);
  try { await refreshStatus(); await loadLibrary(); } catch (e) { status(e.message); }
}

$("login-form").addEventListener("submit", (event) => {
  event.preventDefault();
  storeKey($("key").value.trim());
  $("key").value = "";
  start();
});
$("search-form").addEventListener("submit", (event) => {
  event.preventDefault();
  search($("q").value.trim()).catch((e) => status(e.message));
});
$("sync").addEventListener("click", async (event) => {
  event.target.disabled = true;
  status("Syncing…");
  try { await api("POST", "/library/sync"); await refreshStatus(); await loadLibrary(); }
  catch (e) { status(e.message); }
  event.target.disabled = false;
});
$("logout").addEventListener("click", () => { storeKey(""); show(false); });

start();
