// Brink's level HUD. Brink sends { kind, level 0..1, muted, secs } on every step.
const $ = (s) => document.querySelector(s);

const ICONS = {
  volume: '<svg viewBox="0 0 24 24"><path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z"/><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"/></svg>',
  muted: '<svg viewBox="0 0 24 24"><path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z"/><path d="m16 9.5 5 5m0-5-5 5"/></svg>',
  brightness: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3.6"/><path d="M12 2.8v2M12 19.2v2M2.8 12h2M19.2 12h2M5.5 5.5l1.4 1.4M17.1 17.1l1.4 1.4M5.5 18.5l1.4-1.4M17.1 6.9l1.4-1.4"/></svg>',
  keyboard: '<svg viewBox="0 0 24 24"><rect x="3" y="8" width="18" height="11" rx="2.5"/><path d="M8 15.5h8"/><path d="M12 2.8v2.4M7 3.8l1 1.6M17 3.8l-1 1.6"/></svg>',
  monitor: '<svg viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="12.5" rx="2"/><path d="M9 20h6M12 16.5V20"/></svg>',
  back: '<svg viewBox="0 0 24 24"><path d="M11.5 7 6 12l5.5 5zM19 7l-5.5 5 5.5 5z"/></svg>',
  forward: '<svg viewBox="0 0 24 24"><path d="M12.5 7 18 12l-5.5 5zM5 7l5.5 5L5 17z"/></svg>',
  scrub: '<svg viewBox="0 0 24 24"><path d="M8.5 6.5v11L18 12z"/></svg>',
};

// What the glass bends: a band of Brink's ribbons, picked per level (the image is 1024 x 1536).
// Teal waves for sound, amber folds for light, orange for the keyboard; grey when muted.
const BANDS = {
  volume: { y: 1270, grey: false },
  muted: { y: 1270, grey: true },
  brightness: { y: 560, grey: false },
  monitor: { y: 560, grey: false },
  keyboard: { y: 250, grey: false },
  back: { y: 1270, grey: false },
  forward: { y: 1270, grey: false },
  scrub: { y: 1270, grey: false },
};
const ribbon = new Image();
ribbon.src = "backdrops/ribbons-dark.jpg";
const backdrops = {};

function backdropFor(key) {
  if (backdrops[key]) return backdrops[key];
  backdrops[key] = ribbon.decode().then(
    () =>
      new Promise((resolve) => {
        const band = BANDS[key] || BANDS.volume;
        const c = document.createElement("canvas");
        c.width = 528;
        c.height = 108;
        const g = c.getContext("2d", { willReadFrequently: band.grey });
        const sw = ribbon.naturalWidth, sh = (sw * c.height) / c.width;
        g.drawImage(ribbon, 0, band.y - sh / 2, sw, sh, 0, 0, c.width, c.height);
        if (band.grey) {
          // No ctx.filter in WebKit: desaturate by hand.
          const px = g.getImageData(0, 0, c.width, c.height);
          const d = px.data;
          for (let i = 0; i < d.length; i += 4) {
            const y = 0.3 * d[i] + 0.59 * d[i + 1] + 0.11 * d[i + 2];
            d[i] = d[i + 1] = d[i + 2] = y * 0.8;
          }
          g.putImageData(px, 0, 0);
        }
        c.toBlob((blob) => resolve(blob ? URL.createObjectURL(blob) : null), "image/jpeg", 0.92);
      }),
  );
  return backdrops[key];
}

// Still glass: the light doesn't follow the pointer (the HUD ignores the mouse anyway).
customElements.whenDefined("liquid-glass").then(() => {
  const proto = customElements.get("liquid-glass").prototype;
  const track = proto.movePointer;
  if (typeof track !== "function" || proto.movePointer.__still) return;
  proto.movePointer = function (event) {
    if (!this.hasAttribute("still")) track.call(this, event);
  };
  proto.movePointer.__still = true;
});

const scene = $("#scene");
const SPAN = 146 - 6; // the thumb fills the track, 3px in from each side
let shown = "";

// Seconds as the HUD shows them: "+15 s", "−1:30".
function jump(t) {
  const a = Math.round(Math.abs(t));
  const sign = t < 0 ? "\u2212" : "+";
  return a < 60 ? `${sign}${a} s` : `${sign}${Math.floor(a / 60)}:${String(a % 60).padStart(2, "0")}`;
}

// A playhead time: "4:05", "1:02:09".
function clock(t) {
  const a = Math.max(0, Math.round(t));
  const h = Math.floor(a / 3600), m = Math.floor((a % 3600) / 60), s = String(a % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

function render(h) {
  if (h.kind === "seek" && h.level >= 0) {
    // A seek bar: the fill is the playhead's place in the video, the number its time.
    const key = "scrub";
    if (key !== shown) {
      $("#icon").innerHTML = ICONS[key];
      backdropFor(key).then((url) => url && shown === key && (scene.backdrop = url));
      shown = key;
    }
    $("#thumb").style.width = `${Math.round(Math.min(1, h.level) * SPAN)}px`;
    document.body.className = "seek scrub";
    const time = clock(h.secs);
    $("#pct").textContent = time;
    $("#pct").style.fontSize = time.length > 5 ? "12px" : ""; // hours: keep it off the track
    return;
  }
  $("#pct").style.fontSize = "";
  if (h.kind === "seek") {
    const key = h.secs < 0 ? "back" : "forward";
    if (key !== shown) {
      $("#icon").innerHTML = ICONS[key];
      backdropFor(key).then((url) => url && shown === key && (scene.backdrop = url));
      shown = key;
    }
    // Length unknown: the fill grows with the jump, full at a minute.
    $("#thumb").style.width = `${Math.round(Math.min(1, Math.abs(h.secs) / 60) * SPAN)}px`;
    document.body.className = `seek ${key}`;
    $("#pct").textContent = jump(h.secs);
    return;
  }
  const muted = h.kind === "volume" && h.muted;
  const key = muted ? "muted" : h.kind;
  if (key !== shown) {
    $("#icon").innerHTML = ICONS[key] || ICONS.volume;
    backdropFor(key).then((url) => url && shown === key && (scene.backdrop = url));
    shown = key;
  }
  const level = muted ? 0 : Math.max(0, Math.min(1, h.level));
  $("#thumb").style.width = `${Math.round(level * SPAN)}px`;
  document.body.className = key;
  $("#pct").textContent = muted ? "Muted" : `${Math.round(level * 100)}%`;
}

// Ready every level's backdrop now, so the first slide never shows a blank pill.
Object.keys(BANDS).forEach(backdropFor);

window.__TAURI__?.event.listen("brink://hud", (e) => render(e.payload));

// Browser preview: hud.html?demo=volume&level=0.62[&muted]
const q = new URLSearchParams(location.search);
render({ kind: q.get("demo") || "volume", level: Number(q.get("level") ?? 0.5), muted: q.has("muted"), secs: Number(q.get("secs") ?? 0) });
