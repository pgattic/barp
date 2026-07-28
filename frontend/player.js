const params = new URLSearchParams(location.search);
const dataset = document.body.dataset;

const path = dataset.path || params.get("path");
const savePath = dataset.savePath || params.get("savePath");
const core = dataset.core || params.get("core");
const shader = dataset.shader || params.get("shader") || "disabled";
const smooth = (dataset.smooth || params.get("smooth") || "0") === "1";
const integerScale =
  (dataset.integerScale || params.get("integerScale") || "0") === "1";
const hasSave = (dataset.hasSave || params.get("hasSave") || "0") === "1";
const threads = (dataset.threads || params.get("threads") || "0") === "1";

if (!path || !savePath || !core) {
  document.body.textContent = "Missing emulator parameters.";
  throw new Error("Missing emulator parameters.");
}

// Server layout: saves/<user>/<system>/<ROM basename>.state1 and .srm
const romUrl = `/api/roms/${encodePath(path)}`;
const stateUrl = `/api/saves/${encodePath(`${savePath}.state1`)}`;
const sramUrl = `/api/saves/${encodePath(`${savePath}.srm`)}`;
const browseUrl = browseUrlFor(path);
// Upload battery saves this often. Older EmulatorJS builds don't fire the
// `saveSaveFiles` event, so BARP drives its own flush instead of relying on it.
const sramFlushSeconds = 5;
// Fetch rejects keepalive requests whose body exceeds 64 KiB with a bare
// "Failed to fetch". Battery saves fit and are worth keeping alive across page
// unload; save states are larger and are always sent from a live page.
const keepaliveMaxBytes = 64 * 1024;
let lastSramWrite = Promise.resolve();
let lastSramSignature = null;
let pendingSramSignature = null;
let sramWriteFailed = false;
let sramFlushTimer = null;
let exiting = false;

window.EJS_player = "#game";
window.EJS_gameUrl = romUrl;
window.EJS_core = core;
window.EJS_pathtodata = "/emulatorjs/data/";
// Auto-start only for link navigations. Browser reload has no reliable
// permission/gesture and often sticks on a gray screen until a click.
window.EJS_startOnLoaded = navigationType() === "navigate";
// Keep EmulatorJS IndexedDB so cores (and small ROMs) are not re-downloaded
// and decompressed on every visit. BARP still owns save/state persistence via
// the save/load callbacks below.
window.EJS_disableDatabases = false;
window.EJS_threads = threads;
window.EJS_Buttons = {
  playPause: true,
  restart: true,
  mute: true,
  settings: true,
  fullscreen: true,
  saveState: true,
  loadState: true,
  cacheManager: false,
};
window.EJS_defaultOptions = {
  // EmulatorJS shader menu values: "disabled" or a built-in shader key.
  shader,
  "save-state-slot": 1,
  // Flush battery saves often enough that a short session still persists.
  "save-save-interval": 5,
};

// Save-state button. Handler replaces EmulatorJS download/browser storage.
// Payload is { screenshot, format, state } — only `state` is the bytes.
window.EJS_onSaveState = async ({ state }) => {
  try {
    await putBytes(stateUrl, state);
    notify("SAVED STATE");
  } catch (error) {
    console.error(error);
    notify(`Could not save state: ${error.message}`);
  }
};

// Load-state button. Return value is ignored; we must inject the state ourselves.
window.EJS_onLoadState = async () => {
  try {
    const data = await getBytes(stateUrl);
    if (!data) {
      notify("NO SAVED STATE");
      return;
    }
    window.EJS_emulator.gameManager.loadState(data);
    notify("LOADED STATE");
  } catch (error) {
    console.error(error);
    notify(`Could not load state: ${error.message}`);
  }
};

// Bonus flush path. Released EmulatorJS loaders never register this event, so
// BARP must never depend on it firing — see flushSram().
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
  try {
    if (hasSave) {
      await restoreSram();
    }
  } finally {
    startSramFlushTimer();
    applyDisplay();
  }
};

// Register before GameManager adds its own exit cleanup. This lets us flush
// and copy SRAM bytes before EmulatorJS unmounts its virtual filesystem.
window.EJS_ready = () => {
  window.EJS_emulator.on("exit", exitToBrowser);
  patchRetroArchConfig();
  patchCoreSettingsFile();
  applyPhysicalButtonLayout();
  setupVirtualGamepadAutoHide();
  setupLeftStickAsDpad();
};

window.addEventListener("resize", applyDisplay);
// Closing the tab skips the exit button entirely. PUTs use keepalive so this
// last flush can still finish after the page goes away.
window.addEventListener("pagehide", () => {
  if (!exiting) flushSram();
});

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

function navigationType() {
  const entry = performance.getEntriesByType?.("navigation")?.[0];
  return entry?.type || "navigate";
}

function browseUrlFor(romPath) {
  const parent = romPath.split("/").slice(0, -1).join("/");
  return parent ? `/${encodePath(parent)}` : "/";
}

function patchRetroArchConfig() {
  const GameManager = window.EJS_GameManager;
  if (!GameManager?.prototype?.getRetroArchCfg) return;
  if (GameManager.prototype.getRetroArchCfg.__barpPatched) return;

  const original = GameManager.prototype.getRetroArchCfg;
  GameManager.prototype.getRetroArchCfg = function patchedRetroArchCfg() {
    let cfg = original.call(this);
    // EmulatorJS/RetroArch default aspect_ratio_index is 22 (Core Provided).
    // Many CRT-era cores then apply non-square PAR (e.g. fceumm "8:7 PAR"),
    // which stretches pixels. 21 is 1:1 PAR (square pixels) for every core.
    cfg += 'aspect_ratio_index = "21"\n';
    cfg += "video_force_aspect = true\n";
    cfg += `video_scale_integer = ${integerScale ? "true" : "false"}\n`;
    return cfg;
  };
  GameManager.prototype.getRetroArchCfg.__barpPatched = true;
}

function patchCoreSettingsFile() {
  const emu = window.EJS_emulator;
  if (!emu || emu.getCoreSettings.__barpPatched) return;

  // EmulatorJS writes getCoreSettings() into the core .opt file. Dumping
  // EJS_defaultOptions there (shader, save slots, …) breaks parsing.
  emu.getCoreSettings = () => "";
  emu.getCoreSettings.__barpPatched = true;
}

function applyPhysicalButtonLayout() {
  const emu = window.EJS_emulator;
  if (!emu) return;

  // EmulatorJS defaults to Xbox letter semantics (A on the bottom, B on the
  // right). Map RetroPad buttons by position instead: B/A bottom/right and
  // Y/X left/top, matching Nintendo-style layouts used by most older cores.
  const faceButtons = {
    0: "BUTTON_1", // B: bottom (DualShock cross)
    1: "BUTTON_3", // Y: left   (DualShock square)
    8: "BUTTON_2", // A: right  (DualShock circle)
    9: "BUTTON_4", // X: top    (DualShock triangle)
  };

  for (const controls of [emu.defaultControllers?.[0], emu.controls?.[0]]) {
    if (!controls) continue;
    for (const [button, gamepadInput] of Object.entries(faceButtons)) {
      controls[button] ??= {};
      controls[button].value2 = gamepadInput;
    }
  }
}

// Browser gamepads are injected via EmulatorJS simulateInput, so RetroArch's
// input_player*_analog_dpad_mode never sees the stick. Mirror left-stick
// thresholds onto the D-pad buttons ourselves.
function setupLeftStickAsDpad() {
  const emu = window.EJS_emulator;
  const gp = emu?.gamepad;
  if (!gp?.listeners) return;

  const prev = gp.listeners.axischanged;
  gp.listeners.axischanged = (e) => {
    prev?.(e);
    if (!emu.started || !emu.gameManager?.simulateInput) return;
    if (e.axis !== "LEFT_STICK_X" && e.axis !== "LEFT_STICK_Y") return;

    const pad =
      gp.gamepads.find((entry) => entry?.index === e.gamepadIndex) ||
      gp.gamepads[e.gamepadIndex];
    if (!pad) return;
    const player = emu.gamepadSelection.indexOf(`${pad.id}_${pad.index}`);
    if (player < 0) return;

    const value = e.value || 0;
    if (e.axis === "LEFT_STICK_X") {
      emu.gameManager.simulateInput(player, 7, value > 0.5 ? 1 : 0); // RIGHT
      emu.gameManager.simulateInput(player, 6, value < -0.5 ? 1 : 0); // LEFT
    } else {
      emu.gameManager.simulateInput(player, 5, value > 0.5 ? 1 : 0); // DOWN
      emu.gameManager.simulateInput(player, 4, value < -0.5 ? 1 : 0); // UP
    }
  };
}

// Lemuroid-style: hide on-screen controls once a physical pad is used, and
// bring them back on a screen touch. Hide-on-input (not connect) avoids ghost
// devices that only report as connected.
function setupVirtualGamepadAutoHide() {
  const emu = window.EJS_emulator;
  const gp = emu?.gamepad;
  if (!gp?.listeners || typeof emu.toggleVirtualGamepad !== "function") return;

  let hiddenByGamepad = false;

  function hideFromGamepad() {
    if (hiddenByGamepad) return;
    if (emu.virtualGamepad?.style.display === "none") return;
    hiddenByGamepad = true;
    emu.toggleVirtualGamepad(false);
  }

  function showFromTouch(e) {
    if (!hiddenByGamepad) return;
    // Ignore touches that land on the EmulatorJS menu chrome.
    if (e.target?.closest?.(".ejs_menu_bar, .ejs_settings_parent, .ejs_popup_container")) {
      return;
    }
    hiddenByGamepad = false;
    if (emu.getSettingValue?.("virtual-gamepad") !== "disabled") {
      emu.toggleVirtualGamepad(true);
    }
  }

  function wrap(event, before) {
    const prev = gp.listeners[event];
    gp.listeners[event] = (e) => {
      before?.(e);
      prev?.(e);
    };
  }

  wrap("buttondown", hideFromGamepad);
  wrap("axischanged", (e) => {
    if (Math.abs(e.value || 0) > 0.5) hideFromGamepad();
  });

  const parent = emu.elements?.parent || document.querySelector("#game");
  parent?.addEventListener("touchstart", showFromTouch, { passive: true });
}

function applyDisplay() {
  const canvas = window.EJS_emulator?.canvas;
  if (!canvas) return;

  // EmulatorJS hardcodes RetroArch video_smooth=false. Browser CSS controls
  // how the canvas bitmap is upscaled to the page.
  canvas.style.imageRendering = smooth ? "auto" : "pixelated";

  const gm = window.EJS_emulator.gameManager;
  // Use framebuffer width/height (1:1 pixels), not getVideoDimensions("aspect"),
  // which follows the core's PAR and stays wrong even when RA is set to 1:1.
  const nativeWidth = gm?.getVideoDimensions?.("width") || canvas.width || 0;
  const nativeHeight = gm?.getVideoDimensions?.("height") || canvas.height || 0;
  const box = document.querySelector("#game");
  const maxWidth = box?.clientWidth || window.innerWidth;
  const maxHeight = box?.clientHeight || window.innerHeight;
  if (!nativeWidth || !nativeHeight || !maxWidth || !maxHeight) return;

  const aspect = nativeWidth / nativeHeight;
  let width;
  let height;
  if (integerScale) {
    const scale = Math.max(
      1,
      Math.floor(Math.min(maxWidth / nativeWidth, maxHeight / nativeHeight)),
    );
    width = nativeWidth * scale;
    height = nativeHeight * scale;
  } else if (maxWidth / maxHeight > aspect) {
    height = maxHeight;
    width = height * aspect;
  } else {
    width = maxWidth;
    height = width / aspect;
  }
  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;
}

async function putBytes(url, data) {
  if (data == null) return;
  const body = data instanceof Uint8Array ? data : new Uint8Array(data);
  if (body.byteLength === 0) return;
  const response = await fetch(url, {
    method: "PUT",
    body,
    keepalive: body.byteLength <= keepaliveMaxBytes,
  });
  if (!response.ok) {
    throw new Error(`Save failed (${response.status}): ${await response.text()}`);
  }
}

// Ask the core to flush battery RAM into its virtual save file, then upload it.
function flushSram() {
  const gameManager = window.EJS_emulator?.gameManager;
  if (!gameManager?.getSaveFile) return lastSramWrite;

  let data;
  try {
    data = gameManager.getSaveFile();
  } catch (error) {
    console.warn("Could not read battery save", error);
    return lastSramWrite;
  }
  return queueSramWrite(data);
}

function startSramFlushTimer() {
  if (sramFlushTimer !== null) return;
  sramFlushTimer = setInterval(flushSram, sramFlushSeconds * 1000);
}

function queueSramWrite(data) {
  if (data == null) return lastSramWrite;
  const body = data instanceof Uint8Array ? data : new Uint8Array(data);
  if (body.byteLength === 0) return lastSramWrite;

  // Cores rewrite the whole save file on every flush; only upload changes.
  const signature = sramSignature(body);
  if (signature === lastSramSignature || signature === pendingSramSignature) {
    return lastSramWrite;
  }
  pendingSramSignature = signature;

  lastSramWrite = lastSramWrite
    .catch(() => {})
    .then(async () => {
      try {
        await putBytes(sramUrl, body);
      } catch (error) {
        // Leave the signature unrecorded so the next flush retries instead of
        // deduplicating a save that never reached the server.
        pendingSramSignature = null;
        if (!sramWriteFailed) {
          sramWriteFailed = true;
          notify(`Could not store battery save: ${error.message}`);
        }
        throw error;
      }
      lastSramSignature = signature;
      pendingSramSignature = null;
      sramWriteFailed = false;
    });
  return lastSramWrite;
}

function sramSignature(bytes) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i += 1) {
    hash ^= bytes[i];
    hash = Math.imul(hash, 0x01000193);
  }
  return `${bytes.length}:${hash >>> 0}`;
}

function exitToBrowser() {
  if (exiting) return;
  exiting = true;

  // EmulatorJS unmounts its save filesystem and aborts the core right after
  // this event, so retrying is impossible. Leave either way, and only warn.
  flushSram()
    .catch((error) => {
      console.error(error);
      alert(
        "BARP could not store this session's battery save, so recent progress may be lost.",
      );
    })
    .then(() => location.assign(browseUrl));
}

function notify(message) {
  window.EJS_emulator?.displayMessage?.(message);
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
