// Library browser gamepad nav. Flat list of .row a links; no SPA.
// A / bottom face = open, B / right face = parent folder,
// D-pad / left stick = move selection.
//
// No row is focused until the pad is used. A/B only fire on press edges so a
// held button from the previous page does not immediately navigate again.

const rows = [...document.querySelectorAll("section .row a")];
const parentHref = document.body.dataset.parent || "";

let index = -1; // nothing selected until the pad moves
const wasDown = new Map(); // key -> previously down
const hold = new Map(); // key -> { since, repeated } for d-pad/stick repeat

const confirmButtons = [0]; // A / Cross
const backButtons = [1]; // B / Circle
const upButtons = [12];
const downButtons = [13];

const repeatDelayMs = 400;
const repeatEveryMs = 70;
const stickThreshold = 0.55;

requestAnimationFrame(poll);

function poll() {
  const pads = navigator.getGamepads?.() || [];
  const now = performance.now();

  for (const pad of pads) {
    if (!pad) continue;
    handleButtons(pad, now);
    handleStick(pad, now);
  }

  requestAnimationFrame(poll);
}

function handleButtons(pad, now) {
  for (const i of upButtons) {
    heldRepeat(pad, `btn-${i}`, buttonDown(pad, i), now, () => move(-1));
  }
  for (const i of downButtons) {
    heldRepeat(pad, `btn-${i}`, buttonDown(pad, i), now, () => move(1));
  }
  for (const i of confirmButtons) {
    pressEdge(pad, `btn-${i}`, buttonDown(pad, i), activate);
  }
  for (const i of backButtons) {
    pressEdge(pad, `btn-${i}`, buttonDown(pad, i), goParent);
  }
}

function handleStick(pad, now) {
  const y = pad.axes?.[1] ?? 0;
  heldRepeat(pad, "axis-up", y < -stickThreshold, now, () => move(-1));
  heldRepeat(pad, "axis-down", y > stickThreshold, now, () => move(1));
}

function buttonDown(pad, buttonIndex) {
  const button = pad.buttons?.[buttonIndex];
  return !!button && (button.pressed || button.value > 0.5);
}

// Fire only on a real up→down edge. The first sample after a page load only
// arms the button, so a hold carried over from the previous page is ignored.
function pressEdge(pad, name, down, action) {
  const key = `${pad.index}:${name}`;
  const previously = wasDown.get(key); // undefined | true | false
  wasDown.set(key, down);
  if (previously === undefined) return;
  if (down && previously === false) action();
}

// Move on press, then repeat while held. Same unknown→down rule as pressEdge.
function heldRepeat(pad, name, down, now, action) {
  const key = `${pad.index}:${name}`;
  const previously = wasDown.get(key); // undefined | true | false
  wasDown.set(key, down);

  if (!down) {
    hold.delete(key);
    return;
  }

  if (previously === undefined) {
    // Held across the page load: wait for release before arming.
    return;
  }

  if (previously === false) {
    hold.set(key, { since: now, repeated: false });
    action();
    return;
  }

  const state = hold.get(key);
  if (!state) return;

  const elapsed = now - state.since;
  const due = state.repeated
    ? elapsed >= repeatEveryMs
    : elapsed >= repeatDelayMs;
  if (!due) return;

  state.since = now;
  state.repeated = true;
  action();
}

function move(delta) {
  if (rows.length === 0) return;
  if (index < 0) {
    index = delta > 0 ? 0 : rows.length - 1;
  } else {
    index = (index + delta + rows.length) % rows.length;
  }
  focusRow(index);
}

function focusRow(i) {
  if (rows.length === 0) return;
  index = Math.max(0, Math.min(i, rows.length - 1));
  const el = rows[index];
  el.focus({ preventScroll: true });
  el.scrollIntoView({ block: "nearest", behavior: "smooth" });
}

function activate() {
  if (index < 0) return;
  rows[index]?.click();
}

function goParent() {
  if (!parentHref) return;
  location.assign(parentHref);
}
