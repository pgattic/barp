const params = new URLSearchParams(location.search);
const dataset = document.body.dataset;

const path = dataset.path || params.get("path");
const core = dataset.core || params.get("core");
const filter = dataset.filter || params.get("filter") || "smooth";
const integerScaling =
  (dataset.integerScaling || params.get("integerScaling") || "0") === "1";

if (!path || !core) {
  document.body.textContent = "Missing emulator parameters.";
  throw new Error("Missing emulator parameters.");
}

const romUrl = `/api/roms/${encodePath(path)}`;
const stateUrl = `/api/saves/${encodePath(`${path}.state1`)}`;
const sramUrl = `/api/saves/${encodePath(`${path}.srm`)}`;

window.EJS_player = "#game";
window.EJS_gameUrl = romUrl;
window.EJS_core = core;
window.EJS_pathtodata = "/emulatorjs/data/";
window.EJS_startOnLoaded = true;
window.EJS_disableDatabases = true;
window.EJS_threads = false;
window.EJS_Buttons = {
  playPause: true,
  restart: true,
  mute: true,
  settings: false,
  fullscreen: true,
  saveState: true,
  loadState: true,
};
window.EJS_defaultOptions = {
  shader: filter === "pixelated" ? "nearest" : "default",
  screenRecords: false,
  "save-state-slot": 1,
};
window.EJS_integerScale = integerScaling;
window.EJS_onSaveState = async (data) => {
  await fetch(stateUrl, { method: "PUT", body: data });
};
window.EJS_onSaveSRAM = async (data) => {
  await fetch(sramUrl, { method: "PUT", body: data });
};
window.EJS_onLoadState = async () => {
  const response = await fetch(stateUrl);
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(await response.text());
  return new Uint8Array(await response.arrayBuffer());
};
window.EJS_onLoadSRAM = async () => {
  const response = await fetch(sramUrl);
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(await response.text());
  return new Uint8Array(await response.arrayBuffer());
};

const script = document.createElement("script");
script.src = "/emulatorjs/data/loader.js";
script.onerror = () => {
  document.body.textContent = "EmulatorJS loader missing at /emulatorjs/data/loader.js";
};
document.body.append(script);

function encodePath(path) {
  return path
    .split("/")
    .map(encodeURIComponent)
    .join("/");
}
