const params = new URLSearchParams(location.search);
const dataset = document.body.dataset;

const path = dataset.path || params.get("path");
const savePath = dataset.savePath || params.get("savePath");
const core = dataset.core || params.get("core");
const filter = dataset.filter || params.get("filter") || "smooth";
const integerScaling =
  (dataset.integerScaling || params.get("integerScaling") || "0") === "1";
const hasSave = (dataset.hasSave || params.get("hasSave") || "0") === "1";

if (!path || !savePath || !core) {
  document.body.textContent = "Missing emulator parameters.";
  throw new Error("Missing emulator parameters.");
}

// Server layout: saves/<user>/<system>/<ROM basename>.state1 and .srm
const romUrl = `/api/roms/${encodePath(path)}`;
const stateUrl = `/api/saves/${encodePath(`${savePath}.state1`)}`;
const sramUrl = `/api/saves/${encodePath(`${savePath}.srm`)}`;
const browseUrl = browseUrlFor(path);
let lastSramWrite = Promise.resolve();
let exiting = false;

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
  // Flush battery saves often enough that a short session still persists.
  "save-save-interval": 30,
};
window.EJS_integerScale = integerScaling;

// Save-state button. Handler replaces EmulatorJS download/browser storage.
// Payload is { screenshot, format, state } — only `state` is the bytes.
window.EJS_onSaveState = async ({ state }) => {
  await putBytes(stateUrl, state);
};

// Load-state button. Return value is ignored; we must inject the state ourselves.
window.EJS_onLoadState = async () => {
  const data = await getBytes(stateUrl);
  if (data) {
    window.EJS_emulator.gameManager.loadState(data);
  }
};

// Periodic / exit flush of battery RAM (and manual "Save SAV" button).
window.EJS_onSaveSaveFiles = async (data) => {
  await queueSramWrite(data);
};
window.EJS_onSaveSave = async ({ save }) => {
  await queueSramWrite(save);
};

// Manual "Load SAV" button — inject into the core's expected FS path.
window.EJS_onLoadSave = async () => {
  await restoreSram();
};

// After the game is running, drop any existing battery save into the FS.
window.EJS_onGameStart = async () => {
  if (hasSave) {
    await restoreSram();
  }
};

// Register before GameManager adds its own exit cleanup. This lets us flush
// and copy SRAM bytes before EmulatorJS unmounts its virtual filesystem.
window.EJS_ready = () => {
  window.EJS_emulator.on("exit", exitToBrowser);
};

const script = document.createElement("script");
script.src = "/emulatorjs/data/loader.js";
script.onerror = () => {
  document.body.textContent =
    "EmulatorJS loader missing at /emulatorjs/data/loader.js";
};
document.body.append(script);

function encodePath(value) {
  return value.split("/").map(encodeURIComponent).join("/");
}

function browseUrlFor(romPath) {
  const parent = romPath.split("/").slice(0, -1).join("/");
  return parent ? `/${encodePath(parent)}` : "/";
}

async function putBytes(url, data) {
  if (data == null) return;
  const body = data instanceof Uint8Array ? data : new Uint8Array(data);
  if (body.byteLength === 0) return;
  const response = await fetch(url, { method: "PUT", body, keepalive: true });
  if (!response.ok) {
    throw new Error(`Save failed (${response.status}): ${await response.text()}`);
  }
}

function queueSramWrite(data) {
  if (data == null) return lastSramWrite;
  lastSramWrite = lastSramWrite
    .catch(() => {})
    .then(() => putBytes(sramUrl, data));
  return lastSramWrite;
}

function exitToBrowser() {
  if (exiting) return;
  exiting = true;

  try {
    // Flush the core's current battery save into its virtual save file.
    // This synchronously fires EJS_onSaveSaveFiles, which queues the PUT.
    window.EJS_emulator.gameManager.getSaveFile();
  } catch (error) {
    console.error("Could not flush save data on exit", error);
  }

  lastSramWrite
    .then(() => location.assign(browseUrl))
    .catch((error) => {
      exiting = false;
      console.error(error);
      alert("Could not store save data. Please try exiting again.");
    });
}

async function getBytes(url) {
  const response = await fetch(url);
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(await response.text());
  const buffer = await response.arrayBuffer();
  if (buffer.byteLength === 0) return null;
  return new Uint8Array(buffer);
}

async function restoreSram() {
  const data = await getBytes(sramUrl);
  if (!data) return;

  const gameManager = window.EJS_emulator?.gameManager;
  const savePath = gameManager?.getSaveFilePath?.();
  const fs = gameManager?.FS;
  if (!savePath || !fs) return;

  ensureParentDirectories(fs, savePath);
  if (fs.analyzePath(savePath).exists) {
    fs.unlink(savePath);
  }
  fs.writeFile(savePath, data);
  gameManager.loadSaveFiles();
}

function ensureParentDirectories(fs, filePath) {
  const parts = filePath.split("/");
  let current = "";
  for (let i = 0; i < parts.length - 1; i += 1) {
    if (!parts[i]) continue;
    current += `/${parts[i]}`;
    if (!fs.analyzePath(current).exists) {
      fs.mkdir(current);
    }
  }
}
