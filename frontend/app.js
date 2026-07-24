let bootstrap = null;
let currentPath = "";

const loginSection = document.querySelector("#login");
const browserSection = document.querySelector("#browser");
const playerSection = document.querySelector("#player");
const entries = document.querySelector("#entries");
const crumbs = document.querySelector("#crumbs");
const error = document.querySelector("#error");

document.querySelector("#login-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  clearError();
  const username = document.querySelector("#username").value;
  const password = document.querySelector("#password").value;
  await api("/api/login", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ username, password }),
  });
  await start();
});

document.querySelector("#logout").addEventListener("click", async () => {
  await api("/api/logout", { method: "POST" });
  location.reload();
});

document.querySelector("#close-player").addEventListener("click", () => {
  playerSection.style.display = "none";
  browserSection.hidden = false;
  document.querySelector("#game").replaceChildren();
});

async function start() {
  try {
    bootstrap = await api("/api/bootstrap");
    document.querySelector("#who").textContent = bootstrap.display_name;
    loginSection.hidden = true;
    browserSection.hidden = false;
    await browse("");
  } catch {
    loginSection.hidden = false;
    browserSection.hidden = true;
  }
}

async function browse(path) {
  clearError();
  currentPath = path;
  const list = await api(path ? `/api/browse/${encodePath(path)}` : "/api/browse");
  crumbs.replaceChildren();
  if (path) {
    const up = document.createElement("button");
    up.type = "button";
    up.textContent = "Up";
    up.addEventListener("click", () => browse(parentPath(path)));
    crumbs.append(up, " ", path);
  } else {
    crumbs.textContent = "Systems";
  }

  entries.replaceChildren();
  for (const item of list) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = item.name + (item.type === "dir" ? "/" : "");
    const nextPath = path ? `${path}/${item.name}` : item.name;
    button.addEventListener("click", () => {
      if (item.type === "dir") browse(nextPath);
      else play(nextPath);
    });
    entries.append(button);
  }
}

async function play(path) {
  clearError();
  const system = bootstrap.systems.find((candidate) => candidate.folder === path.split("/")[0]);
  if (!system) {
    showError(`Unknown system for ${path}`);
    return;
  }

  browserSection.hidden = true;
  playerSection.style.display = "block";
  document.querySelector("#game").replaceChildren();

  window.EJS_player = "#game";
  window.EJS_gameUrl = `/api/roms/${encodePath(path)}`;
  window.EJS_core = system.core;
  window.EJS_pathtodata = "/emulatorjs/data/";
  window.EJS_startOnLoaded = true;
  window.EJS_disableDatabases = true;
  window.EJS_threads = false;
  window.EJS_Buttons = { playPause: true, restart: true, mute: true, settings: false, fullscreen: true, saveState: true, loadState: true };
  window.EJS_defaultOptions = {
    "shader": bootstrap.options.display_filter === "pixelated" ? "nearest" : "default",
    "screenRecords": false,
    "save-state-slot": 1
  };
  window.EJS_integerScale = Boolean(bootstrap.options.integer_scaling);
  window.EJS_onSaveState = async (data) => saveBytes(`${path}.state1`, data);
  window.EJS_onSaveSRAM = async (data) => saveBytes(`${path}.srm`, data);
  window.EJS_onLoadState = async () => loadBytes(`${path}.state1`);
  window.EJS_onLoadSRAM = async () => loadBytes(`${path}.srm`);

  const script = document.createElement("script");
  script.src = "/emulatorjs/data/loader.js";
  script.onerror = () => showError("EmulatorJS loader missing at /emulatorjs/data/loader.js");
  document.body.append(script);
}

async function saveBytes(path, data) {
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  await api(`/api/saves/${encodePath(path)}`, { method: "PUT", body: bytes });
}

async function loadBytes(path) {
  const response = await fetch(`/api/saves/${encodePath(path)}`);
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(await response.text());
  return new Uint8Array(await response.arrayBuffer());
}

async function api(path, options = {}) {
  const response = await fetch(path, options);
  if (!response.ok) {
    const text = await response.text();
    showError(text);
    throw new Error(text);
  }
  if (response.status === 204) return null;
  return response.json();
}

function encodePath(path) {
  return path.split("/").map(encodeURIComponent).join("/");
}

function parentPath(path) {
  return path.split("/").slice(0, -1).join("/");
}

function clearError() {
  error.textContent = "";
}

function showError(message) {
  error.textContent = message;
}

start();
