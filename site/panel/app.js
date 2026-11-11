// Brink panel. Runs inside the Tauri popover; in a plain browser it falls back to a mock
// backend so the panel can be previewed (and embedded on the website).
const T = window.__TAURI__;
const mock = !T;

const SUN = '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="4" fill="currentColor"/><path d="M12 2.5v2M12 19.5v2M4.6 4.6 6 6M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4"/></svg>';
const SPK = '<svg viewBox="0 0 24 24"><path d="M11 5 6 9H3v6h3l5 4z" fill="currentColor"/><path d="M15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13"/></svg>';
const HAPTICS = ["off", "light", "medium", "strong"];

const mockState = {
  settings: {
    enabled: true, haptic: "medium", quickMute: true, corners: { topLeft: "off", topRight: "mute", bottomLeft: "off", bottomRight: "off" }, mediaStrip: true, topStrip: true, reverseSwipe: false, tapToPlay: false, bottomSeek: true,
    sensitivity: 2, edgeWidth: 0.12, swapSides: false, activeArea: "anywhere", requireModifier: false,
    freezeCursor: true, fineControl: true, preciseSliding: false, appearance: "dark", backdrop: "ribbons", openAtLogin: true,
    magicMouse: false, windowGestures: true, dockGestures: true, dragDividers: true, windowKeys: false, levelHud: true, monitorBrightness: true, onboarded: true,
  },
  levels: { volume: 0.62, brightness: 0.48, muted: false },
  trusted: true,
  version: "1.0.0",
  update: null,
};

const listeners = {};
const api = {
  invoke(cmd, args) {
    if (!mock) return T.core.invoke(cmd, args);
    if (cmd === "get_state") return Promise.resolve(structuredClone(mockState));
    if (cmd === "get_levels") return Promise.resolve({ ...mockState.levels });
    if (cmd === "wallpaper") {
      const wall = new URLSearchParams(location.search).get("wall");
      return wall ? fetch(wall).then((r) => r.arrayBuffer()) : Promise.resolve(new ArrayBuffer(0));
    }
    if (cmd === "placement") return Promise.resolve(null);
    if (cmd === "set_settings") {
      Object.assign(mockState.settings, args.patch);
      return Promise.resolve(structuredClone(mockState.settings));
    }
    return Promise.resolve(null);
  },
  listen(ev, fn) {
    if (!mock) return T.event.listen(ev, (e) => fn(e.payload));
    (listeners[ev] ||= []).push(fn);
    return Promise.resolve();
  },
};
// Lets a host page (the website demo) drive the mock: brink.emit("brink://edge", {...}).
window.brink = {
  emit: (ev, p) => (listeners[ev] || []).forEach((f) => f(p)),
  setLevels: (l) => {
    Object.assign(mockState.levels, l);
    if (state) Object.assign(state.levels, l), renderMirror();
  },
};

// Still glass: the panel's light stays put and edges never bulge toward the pointer. The
// component tracks the pointer in one method; switch it off for scenes marked `still`.
customElements.whenDefined("liquid-glass").then(() => {
  const proto = customElements.get("liquid-glass").prototype;
  const track = proto.movePointer;
  if (typeof track !== "function" || proto.movePointer.__still) return;
  proto.movePointer = function (event) {
    if (!this.hasAttribute("still")) track.call(this, event);
  };
  proto.movePointer.__still = true;
});

const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];
let state = null;

// What a double-tap in each corner can do (matches the app's CornerAction).
const CORNER_ACTIONS = {
  off: "Nothing", mute: "Mute sound", muteMic: "Mute microphone", playPause: "Play or pause", nextTrack: "Next track",
  missionControl: "Mission Control", appWindows: "App windows", showDesktop: "Show desktop", spotlight: "Spotlight",
  screenshot: "Screenshot", lockScreen: "Lock screen", sleepDisplay: "Sleep display",
};
const CORNER_NAMES = { topLeft: "Top left", topRight: "Top right", bottomLeft: "Bottom left", bottomRight: "Bottom right" };

let live = null; // { control, y } while a finger is on an edge
let visible = true;
let readoutTimer = 0;

const pct = (v) => (v == null ? "–" : Math.round(v * 100));

function sides() {
  const s = state.settings;
  return s.swapSides ? { left: "volume", right: "brightness" } : { left: "brightness", right: "volume" };
}

function renderMirror() {
  const s = state.settings;
  const map = sides();
  for (const side of ["left", "right"]) {
    const ctl = map[side];
    const isVol = ctl === "volume";
    const v = isVol ? state.levels.volume : state.levels.brightness;
    const cap = $(`#cap-${side}`);
    cap.className = `cap ${isVol ? "cool" : "warm"}${live && live.control === ctl ? " live" : ""}${isVol && state.levels.muted ? " muted" : ""}`;
    cap.querySelector(".fill").style.height = `${Math.max(0, (v ?? 0) * 100)}%`;
    // Keep the number and glyph on a surface they contrast with: the number sits at the top
    // (on the fill once the level passes ~85%), the glyph at the bottom (on glass below ~12%).
    cap.classList.toggle("pct-on-fill", (v ?? 0) > 0.85);
    cap.classList.toggle("glyph-on-glass", (v ?? 0) < 0.12 || (isVol && state.levels.muted));
    cap.querySelector(".pct").textContent = isVol && state.levels.muted ? "Off" : pct(v);
    const g = cap.querySelector(".glyph");
    if (g.dataset.k !== ctl) (g.innerHTML = isVol ? SPK : SUN), (g.dataset.k = ctl);
    if (live && live.control === ctl) cap.querySelector(".finger").style.top = `${(1 - live.y) * 100}%`;
  }
  for (const dot of $$(".corner[data-corner]")) {
    const a = s.quickMute ? (s.corners || {})[dot.dataset.corner] || "off" : "off";
    dot.hidden = a === "off";
    dot.title = `Double-tap: ${CORNER_ACTIONS[a] || ""}`;
  }
  $("#strip-top").classList.toggle("has-l", !$('.corner[data-corner="topLeft"]').hidden);
  for (const [id, on, ctl] of [["#strip-top", s.topStrip, "keyboard"], ["#strip-media", s.mediaStrip, "media"]]) {
    const el = $(id);
    const isLive = live && live.control === ctl;
    el.classList.toggle("off", !on);
    el.classList.toggle("live", !!isLive);
    if (isLive) el.querySelector(".finger").style.left = `${live.y * 100}%`;
  }
  $("#swap-desc").textContent = s.swapSides ? "Volume left, brightness right" : "Brightness left, volume right";
}

function showReadout(control) {
  if (control === "keyboard" || control === "media") {
    $("#readout-value").textContent = "";
    $("#readout-label").textContent = control === "keyboard" ? "Keyboard light" : "Swipe to skip";
  } else {
    const v = control === "volume" ? state.levels.volume : state.levels.brightness;
    const muted = control === "volume" && state.levels.muted;
    $("#readout-value").textContent = muted ? "Muted" : `${pct(v)}%`;
    $("#readout-label").textContent = control === "volume" ? "Volume" : "Brightness";
  }
  clearTimeout(readoutTimer);
  readoutTimer = setTimeout(() => {
    if (live) return;
    $("#readout-value").textContent = "";
    $("#readout-label").textContent = idleHint();
  }, 1400);
}

function idleHint() {
  if (!state.settings.enabled) return "Edges are off";
  const s = state.settings;
  return s.requireModifier ? "Hold Fn or ⌥ and slide an edge" : "Slide an edge";
}

function renderControls() {
  const s = state.settings;
  $$(".tile[data-tile]").forEach((t) => {
    const k = t.dataset.tile;
    t.classList.toggle("on", k === "haptic" ? s.haptic !== "off" : !!s[k]);
  });
  $("#haptic-label").textContent = s.haptic[0].toUpperCase() + s.haptic.slice(1);
  $("#corners-label").textContent = editingCorners ? "Pick an action" : cornersLabel(s);
  $$("select[data-corner]").forEach((el) => {
    const a = cornerAt(s, el.dataset.corner);
    el.value = a;
    const chip = el.closest(".cchip");
    chip.classList.toggle("set", a !== "off");
    chip.querySelector("b").textContent = a === "off" ? "Nothing" : CORNER_ACTIONS[a];
  });
  $('.tile[data-tile="quickMute"]').classList.toggle("on", anyCorner(s));
  $('.tile[data-tile="quickMute"]').classList.toggle("open", editingCorners);
  $("#menu-enabled").textContent = s.enabled ? "Pause Brink" : "Resume Brink";
  $("#cheats-title").classList.toggle("off", !s.windowGestures);
  $("#cheats-dock").classList.toggle("off", !s.dockGestures);
  $$(".cc-slider").forEach(paintSlider);
  $$("input.switch[data-key]").forEach((el) => (el.checked = !!s[el.dataset.key]));
  $$("select[data-key]").forEach((el) => (el.value = s[el.dataset.key]));
  $$(".seg[data-key]").forEach((el) =>
    el.querySelectorAll("button").forEach((b) => b.classList.toggle("on", b.dataset.value === s[el.dataset.key])),
  );
  if (document.documentElement.dataset.theme !== s.appearance || renderControls.backdrop !== s.backdrop) {
    document.documentElement.dataset.theme = s.appearance;
    renderControls.backdrop = s.backdrop;
    paintBackdrop();
  }
  if (!live && !$("#readout-value").textContent) $("#readout-label").textContent = idleHint();
}

function renderStatus() {
  const chip = $("#status");
  chip.className = `chip ${!state.trusted ? "warn" : !state.settings.enabled ? "off" : ""}`;
  chip.querySelector("span").textContent = !state.trusted ? "Needs access" : state.settings.enabled ? "Ready" : "Paused";
  $("#guide").hidden = state.trusted && state.settings.onboarded;
  $("#step-access").classList.toggle("done", state.trusted);
  $("#step-slide").classList.toggle("waiting", !state.trusted);
  $("#update-pill").hidden = !state.update;
  $("#version").textContent = `Version ${state.version}`;
}

function render() {
  renderMirror();
  renderControls();
  renderStatus();
  fit();
}

let lastHeight = 0;
// The panel hangs from the menu bar; keep it on screen (the app caps it at 760pt).
const PANEL_MAX = Math.min(760, (screen.availHeight || 800) - 40);
function fit() {
  const head = $("#view-settings .top");
  document.documentElement.style.setProperty("--scroll-max", `${PANEL_MAX - 24 - (head.offsetHeight || 42)}px`);
  requestAnimationFrame(() => {
    const h = $("#panel").offsetHeight;
    if (!mock) api.invoke("set_panel_height", { height: h });
    if (Math.abs(h - lastHeight) > 1) (lastHeight = h), paintBackdrop();
  });
}

// ---------- the glass backdrop: the wallpaper, cropped to what is behind the panel ----------

let wallpaper = null; // HTMLImageElement
let placementInfo = null; // { x, y, sw, sh } in points from the screen's top-left
let backdropUrl = null;

async function loadWallpaper() {
  try {
    const buf = await api.invoke("wallpaper");
    if (!buf || !buf.byteLength) return;
    const url = URL.createObjectURL(new Blob([buf], { type: "image/jpeg" }));
    const img = new Image();
    img.src = url;
    await img.decode();
    wallpaper = img;
  } catch (e) {
    console.warn("brink: no wallpaper backdrop", e);
  }
}

// Backdrop processing, all per pixel (WebKit's canvas has no ctx.filter, so nothing here may
// rely on it). Two steps:
//  1. Tone: mute the source (less saturation and contrast, pulled toward graphite in Dark or warm
//     paper in Light) so the backdrop stays quieter than the controls.
//  2. Legibility floor, in linear (relative) luminance, only where text sits: Light pixels at
//     least `target` bright, so #18191b text keeps ~10:1; Dark pixels at most `target`, so white
//     text does. Pixels that already pass are untouched, so there are no patches.
const TONE = {
  ribbons: { dark: { sat: 0.32, contrast: 0.66, base: [0.07, 0.075, 0.085], mix: 0.52 }, light: { sat: 0.3, contrast: 0.6, base: [0.955, 0.95, 0.935], mix: 0.5 } },
  wallpaper: { dark: { sat: 0.8, contrast: 0.85, base: [0.07, 0.075, 0.085], mix: 0.3 }, light: { sat: 0.8, contrast: 0.85, base: [0.955, 0.95, 0.935], mix: 0.25 } },
};
const FLOOR = { light: { target: 0.5, toward: 1.0 }, dark: { target: 0.05, toward: 0.006 } };
const toLin = new Float32Array(256).map((_, i) => {
  const v = i / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
});
const fromLin = new Uint8ClampedArray(4096).map((_, i) => {
  const v = i / 4095;
  return Math.round(255 * (v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055));
});
const lin = (v) => toLin[Math.max(0, Math.min(255, Math.round(v * 255)))];

function processBackdrop(g, width, height, dark, mask, tone) {
  const f = dark ? FLOOR.dark : FLOOR.light;
  const img = g.getImageData(0, 0, width, height);
  const d = img.data;
  const t = f.toward;
  const [br, bg, bb] = tone.base;
  for (let i = 0; i < d.length; i += 4) {
    // 1. tone (sRGB 0..1)
    let R = d[i] / 255, G = d[i + 1] / 255, B = d[i + 2] / 255;
    const y = 0.299 * R + 0.587 * G + 0.114 * B;
    R = y + (R - y) * tone.sat; G = y + (G - y) * tone.sat; B = y + (B - y) * tone.sat;
    R = (R - 0.5) * tone.contrast + 0.5; G = (G - 0.5) * tone.contrast + 0.5; B = (B - 0.5) * tone.contrast + 0.5;
    R += (br - R) * tone.mix; G += (bg - G) * tone.mix; B += (bb - B) * tone.mix;
    // 2. floor, only under text
    let r = lin(R), gg = lin(G), b = lin(B);
    const m = mask[i + 3] / 255;
    if (m > 0) {
      const L = 0.2126 * r + 0.7152 * gg + 0.0722 * b;
      let a = dark ? (L > f.target ? (L - f.target) / (L - t) : 0) : L < f.target ? (f.target - L) / (t - L) : 0;
      a = Math.min(1, a) * m;
      r += (t - r) * a; gg += (t - gg) * a; b += (t - b) * a;
    }
    d[i] = fromLin[(r * 4095) | 0];
    d[i + 1] = fromLin[(gg * 4095) | 0];
    d[i + 2] = fromLin[(b * 4095) | 0];
  }
  g.putImageData(img, 0, 0);
}

// Without any image (unreadable file): a soft graphite field.
function paintFallback(g, w, h) {
  const base = g.createLinearGradient(0, 0, w, h);
  base.addColorStop(0, "#ffb547");
  base.addColorStop(0.5, "#5a5b5f");
  base.addColorStop(1, "#2bd4c4");
  g.fillStyle = base;
  g.fillRect(0, 0, w, h);
}

const ribbons = {};
function ribbon(theme) {
  // backdrops/ribbons-light.jpg and -dark.jpg; dark falls back to light if it is missing.
  if (!ribbons[theme]) {
    ribbons[theme] = new Promise((resolve) => {
      const img = new Image();
      img.onload = () => resolve(img);
      img.onerror = () => resolve(theme === "dark" ? ribbon("light") : null);
      img.src = `backdrops/ribbons-${theme}.jpg`;
    });
  }
  return ribbons[theme];
}

// Text and icons, and how far around each the backdrop must clear the contrast floor.
const LEGIBLE = [
  [".view:not([hidden]) .word, .view:not([hidden]) .title, .view:not([hidden]) .ver, .view:not([hidden]) h6", 6],
  [".view:not([hidden]) .readout", 8],
  // Inside glass the face refracts a little from around the words, so reach further.
  [".view:not([hidden]) .chip, .view:not([hidden]) .strip, .view:not([hidden]) .cc-slider, .view:not([hidden]) .list, .view:not([hidden]) .tile, .view:not([hidden]) .round, .view:not([hidden]) .cap .pct, .view:not([hidden]) .cap .glyph, .view:not([hidden]) .guide", 12],
];

function legibilityMask(scene, cw, ch, dpr) {
  const m = document.createElement("canvas");
  m.width = cw;
  m.height = ch;
  const g = m.getContext("2d", { willReadFrequently: true });
  const host = scene.getBoundingClientRect();
  // Feathered edges via a shadow cast from shapes drawn off-canvas (shadowBlur works everywhere).
  const off = cw + 1000;
  g.fillStyle = "#fff";
  g.shadowColor = "#fff";
  g.shadowBlur = 14 * dpr;
  g.shadowOffsetX = off;
  for (const [sel, pad] of LEGIBLE) {
    for (const el of scene.querySelectorAll(sel)) {
      const r = el.getBoundingClientRect();
      if (!r.width || !r.height) continue;
      const x = (r.left - host.left - pad) * dpr, y = (r.top - host.top - pad) * dpr;
      const w = (r.width + pad * 2) * dpr, h = (r.height + pad * 2) * dpr;
      g.beginPath();
      if (g.roundRect) g.roundRect(x - off, y, w, h, Math.min(h / 2, 18 * dpr));
      else g.rect(x - off, y, w, h);
      g.fill();
    }
  }
  return g.getImageData(0, 0, cw, ch).data;
}

function cover(g, img, cw, ch, focusX = 0.5, focusY = 0.5) {
  const k = Math.max(cw / img.naturalWidth, ch / img.naturalHeight);
  const sw = cw / k, sh = ch / k;
  g.drawImage(img, (img.naturalWidth - sw) * focusX, (img.naturalHeight - sh) * focusY, sw, sh, 0, 0, cw, ch);
}

/** Soft blur without ctx.filter: shrink, then scale back up with smoothing. */
function softBlur(g, cw, ch, factor = 4) {
  const s = document.createElement("canvas");
  s.width = Math.max(1, Math.round(cw / factor));
  s.height = Math.max(1, Math.round(ch / factor));
  const sg = s.getContext("2d");
  sg.imageSmoothingQuality = "high";
  sg.drawImage(g.canvas, 0, 0, s.width, s.height);
  g.imageSmoothingQuality = "high";
  g.clearRect(0, 0, cw, ch);
  g.drawImage(s, 0, 0, cw, ch);
}

let paintSeq = 0;
async function paintBackdrop() {
  const seq = ++paintSeq;
  const scene = $("#panel");
  const w = scene.offsetWidth, h = scene.offsetHeight;
  if (!w || !h) return;
  const dark = document.documentElement.dataset.theme === "dark";
  const useWall = state?.settings?.backdrop === "wallpaper" && wallpaper;
  const rib = useWall ? null : await ribbon(dark ? "dark" : "light");
  if (seq !== paintSeq) return;
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const c = document.createElement("canvas");
  c.width = Math.round(w * dpr);
  c.height = Math.round(h * dpr);
  const g = c.getContext("2d", { willReadFrequently: true });
  if (useWall) {
    // The wallpaper fills the screen (cover); find the panel's rectangle in image pixels.
    const iw = wallpaper.naturalWidth, ih = wallpaper.naturalHeight;
    const p = placementInfo || { x: (iw / ih) * 900 - w - 40, y: 40, sw: (iw / ih) * 900, sh: 900 };
    const k = Math.max(p.sw / iw, p.sh / ih);
    const ox = (p.sw - iw * k) / 2, oy = (p.sh - ih * k) / 2;
    const sx = (p.x - ox) / k, sy = (p.y - oy) / k;
    g.drawImage(wallpaper, sx, sy, w / k, h / k, 0, 0, c.width, c.height);
    softBlur(g, c.width, c.height, 3);
  } else if (rib) {
    cover(g, rib, c.width, c.height, 0.5, 0.35);
  } else {
    paintFallback(g, c.width, c.height);
  }
  const tone = TONE[useWall ? "wallpaper" : "ribbons"][dark ? "dark" : "light"];
  processBackdrop(g, c.width, c.height, dark, legibilityMask(scene, c.width, c.height, dpr), tone);
  c.toBlob((blob) => {
    if (!blob || seq !== paintSeq) return;
    const url = URL.createObjectURL(blob);
    scene.backdrop = url;
    if (backdropUrl) setTimeout((old) => URL.revokeObjectURL(old), 2000, backdropUrl);
    backdropUrl = url;
  }, "image/jpeg", 0.92);
}

// Only the showing view's elements are glass: a scene renders at most 16.
function syncGlass() {
  $$("[data-g]").forEach((el) => {
    const shown = !el.closest(".view[hidden]");
    if (shown) el.setAttribute("glass", "");
    else el.removeAttribute("glass");
  });
}

async function patch(p) {
  Object.assign(state.settings, p);
  render();
  state.settings = await api.invoke("set_settings", { patch: p });
  render();
}

function toast(msg, ms = 2200) {
  const t = $("#toast");
  t.textContent = msg;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (t.hidden = true), ms);
}

// ---------- capsule sliders ----------

function sliderFormat(key, v) {
  return key === "sensitivity" ? `${+v.toFixed(2)}×` : `${Math.round(v * 100)}%`;
}

function paintSlider(el) {
  const k = el.dataset.key;
  const min = +el.dataset.min, max = +el.dataset.max;
  const v = state.settings[k];
  const p = (v - min) / (max - min);
  el.querySelector(".cc-fill").style.width = `calc(36px + ${p} * (100% - 36px))`;
  el.querySelector(".cc-val").textContent = sliderFormat(k, v);
}

// Scrolled settings put different text over the backdrop: redo its legibility floor.
let scrollPaint = 0;
$("#settings-scroll").addEventListener("scroll", () => {
  clearTimeout(scrollPaint);
  scrollPaint = setTimeout(paintBackdrop, 150);
});

$$(".cc-slider").forEach((el) => {
  const k = el.dataset.key;
  const min = +el.dataset.min, max = +el.dataset.max, step = +el.dataset.step;
  const valueAt = (x) => {
    const r = el.getBoundingClientRect();
    const p = Math.min(1, Math.max(0, (x - r.left - 18) / (r.width - 36)));
    return Math.round((min + p * (max - min)) / step) * step;
  };
  el.addEventListener("pointerdown", (e) => {
    el.setPointerCapture(e.pointerId);
    el.classList.add("dragging");
    state.settings[k] = +valueAt(e.clientX).toFixed(3);
    paintSlider(el);
  });
  el.addEventListener("pointermove", (e) => {
    if (!el.hasPointerCapture(e.pointerId)) return;
    state.settings[k] = +valueAt(e.clientX).toFixed(3);
    paintSlider(el);
  });
  const end = (e) => {
    if (!el.classList.contains("dragging")) return;
    el.classList.remove("dragging");
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    patch({ [k]: state.settings[k] });
  };
  el.addEventListener("pointerup", end);
  el.addEventListener("pointercancel", end);
});

// ---------- wiring ----------

$$("input.switch[data-key]").forEach((el) => el.addEventListener("change", () => patch({ [el.dataset.key]: el.checked })));
$$("select[data-key]").forEach((el) => el.addEventListener("change", () => patch({ [el.dataset.key]: el.value })));
$$("select[data-corner]").forEach((el) => {
  el.innerHTML = Object.entries(CORNER_ACTIONS).map(([v, t]) => `<option value="${v}">${t}</option>`).join("");
  el.addEventListener("change", () => {
    const s = state.settings;
    const corners = {};
    for (const k of Object.keys(CORNER_NAMES)) corners[k] = cornerAt(s, k);
    corners[el.dataset.corner] = el.value;
    // Corners are on while any corner has an action; setting them all to Nothing turns them off.
    patch({ corners, quickMute: Object.values(corners).some((a) => a !== "off") });
  });
});
$$(".seg[data-key]").forEach((el) =>
  el.addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (b) patch({ [el.dataset.key]: b.dataset.value });
  }),
);
$$(".tile[data-tile]").forEach((t) =>
  t.addEventListener("click", () => {
    const k = t.dataset.tile;
    if (k === "quickMute") return setEditingCorners(!editingCorners);
    if (k === "haptic") {
      const next = HAPTICS[(HAPTICS.indexOf(state.settings.haptic) + 1) % HAPTICS.length];
      patch({ haptic: next });
    } else patch({ [k]: !state.settings[k] });
  }),
);

function showView(name) {
  $("#view-main").hidden = name !== "main";
  $("#view-settings").hidden = name !== "settings";
  syncGlass();
  fit();
  requestAnimationFrame(paintBackdrop); // the legible areas moved
}

document.addEventListener("click", async (e) => {
  const a = e.target.closest("[data-action]");
  const menu = $("#menu");
  if (!a || a.dataset.action !== "more") menu.hidden = true;
  if (!a) return;
  switch (a.dataset.action) {
    case "settings": return showView("settings");
    case "back": return showView("main");
    case "corners-done": return setEditingCorners(false);
    case "more": menu.hidden = !menu.hidden; return;
    case "quit": return api.invoke("quit");
    case "access": return api.invoke("request_access");
    case "site": return api.invoke("open_url", { url: "https://zerowing.in/brink/" });
    case "restart": return api.invoke("restart");
    case "enabled": return patch({ enabled: !state.settings.enabled });
    case "update": {
      toast("Checking for updates…", 15000);
      try {
        const v = await api.invoke("check_update");
        if (v) {
          state.update = v;
          renderStatus();
          toast(`Brink ${v} is ready. Restart to update.`);
        } else toast("You’re up to date.");
      } catch (err) {
        console.error(err);
        toast("Couldn’t check for updates.");
      }
    }
  }
});

function flashCorner(k) {
  for (const d of [$(`.corner[data-corner="${k}"]`), $(`.cchip[data-at="${k}"]`)]) {
    if (!d) continue;
    d.classList.add("flash");
    setTimeout(() => d.classList.remove("flash"), 700);
  }
}

// What a corner does right now: nothing at all while corners are switched off.
const cornerAt = (s, k) => (s.quickMute ? (s.corners || {})[k] || "off" : "off");
const anyCorner = (s) => Object.keys(CORNER_NAMES).some((k) => cornerAt(s, k) !== "off");

// The Corners tile opens corner setup on the trackpad mirror itself.
let editingCorners = false;
function setEditingCorners(on) {
  editingCorners = on;
  $("#corner-edit").hidden = !on;
  $("#mirror").classList.toggle("editing", on);
  render();
}
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && editingCorners) setEditingCorners(false);
});

function cornersLabel(s) {
  const set = Object.entries(s.corners || {}).filter(([, a]) => a !== "off");
  if (!s.quickMute || !set.length) return "Double-tap";
  if (set.length === 1) return `${CORNER_NAMES[set[0][0]]}: ${CORNER_ACTIONS[set[0][1]]}`;
  return `${set.length} corners set`;
}

let mutedBefore = false;
function onLevels(l) {
  const muteFlip = l.muted !== mutedBefore;
  mutedBefore = l.muted;
  state.levels = l;
  renderMirror();
  if (live) showReadout(live.control);
  else if (muteFlip && state.settings.quickMute) {
    showReadout("volume");
  }
}

async function boot() {
  state = await api.invoke("get_state");
  mutedBefore = state.levels.muted;
  const q = new URLSearchParams(location.search);
  if (q.get("view") === "settings") showView("settings");
  syncGlass();
  render();
  placementInfo = await api.invoke("placement");
  await loadWallpaper();
  paintBackdrop();
  api.listen("brink://placement", (p) => {
    const moved = !placementInfo || Math.abs(p.x - placementInfo.x) > 1 || Math.abs(p.y - placementInfo.y) > 1;
    placementInfo = p;
    if (moved) paintBackdrop();
  });

  api.listen("brink://levels", onLevels);
  api.listen("brink://corner", flashCorner);
  api.listen("brink://edge", (c) => {
    live = c;
    renderMirror();
    if (c) showReadout(c.control);
    else {
      // Finger lifted: hold the final value a moment, then go back to the hint.
      clearTimeout(readoutTimer);
      readoutTimer = setTimeout(() => {
        $("#readout-value").textContent = "";
        $("#readout-label").textContent = idleHint();
      }, 1200);
    }
    if (c && state.trusted && !state.settings.onboarded) {
      patch({ onboarded: true });
      toast("That’s it. Every edge works the same way.");
    }
  });
  api.listen("brink://trusted", () => {
    state.trusted = true;
    renderStatus();
    fit();
  });
  api.listen("brink://update", (v) => {
    state.update = v;
    renderStatus();
  });
  api.listen("brink://visible", async (v) => {
    visible = v;
    if (v) {
      state = await api.invoke("get_state");
      render();
      // The wallpaper may have changed since last time.
      if (Date.now() - (loadWallpaper.at || 0) > 60000) {
        loadWallpaper.at = Date.now();
        await loadWallpaper();
        paintBackdrop();
      }
    } else $("#menu").hidden = true;
  });

  // Keys, Control Center and other apps change levels too; stay honest while open.
  setInterval(async () => {
    if (!visible || mock) return;
    onLevels(await api.invoke("get_levels"));
  }, 400);
}

boot();
