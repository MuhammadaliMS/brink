// The trackpad grid (#corners): drag the edge faders, flick the media knob to skip, and
// double-click a corner to fire it. Plain DOM, no libraries.
(() => {
  // The grid itself: on wide screens it lives inside the pinned story, not its own section.
  const root = document.querySelector(".bento");
  if (!root) return;
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

  // ---------- faders ----------
  root.querySelectorAll("[data-fader]").forEach((tile) => {
    const track = tile.querySelector(".b-track");
    const out = tile.querySelector(".b-val");
    const horizontal = tile.dataset.axis === "x";
    let v = Number(tile.getAttribute("aria-valuenow")) / 100;

    const set = (next) => {
      v = Math.min(1, Math.max(0, next));
      const pct = Math.round(v * 100);
      tile.style.setProperty("--v", v);
      tile.setAttribute("aria-valuenow", pct);
      out.textContent = pct;
    };
    set(v);

    const at = (e) => {
      const r = track.getBoundingClientRect();
      return horizontal ? (e.clientX - r.left) / r.width : 1 - (e.clientY - r.top) / r.height;
    };
    // Like the real edge: the finger moves the level from where it is, it doesn't jump.
    let drag = null;
    tile.addEventListener("pointerdown", (e) => {
      tile.setPointerCapture(e.pointerId);
      drag = { from: at(e), v };
    });
    tile.addEventListener("pointermove", (e) => {
      if (drag) set(drag.v + (at(e) - drag.from));
    });
    const end = () => (drag = null);
    tile.addEventListener("pointerup", end);
    tile.addEventListener("pointercancel", end);
    tile.addEventListener("keydown", (e) => {
      const up = horizontal ? ["ArrowRight", "ArrowUp"] : ["ArrowUp", "ArrowRight"];
      const down = horizontal ? ["ArrowLeft", "ArrowDown"] : ["ArrowDown", "ArrowLeft"];
      if (up.includes(e.key)) set(v + 1 / 16);
      else if (down.includes(e.key)) set(v - 1 / 16);
      else return;
      e.preventDefault();
    });
  });

  // ---------- bottom edge: swipe to skip ----------
  const swipe = root.querySelector("[data-swipe]");
  if (swipe) {
    const rail = swipe.querySelector(".b-rail");
    const knob = swipe.querySelector(".b-knob");
    const name = swipe.querySelector(".b-track-name");
    const tracks = ["Night Drive", "Low Tide", "Paper Planes", "Static Bloom", "Long Exposure", "Afterglow"];
    let n = 0;
    const skip = (dir) => {
      n = (n + dir + tracks.length) % tracks.length;
      name.classList.add("swap");
      setTimeout(() => {
        name.textContent = `Track ${String(n + 3).padStart(2, "0")} · ${tracks[n]}`;
        name.classList.remove("swap");
      }, reduced ? 0 : 180);
    };
    let drag = null;
    const room = () => rail.clientWidth / 2 - 24;
    rail.addEventListener("pointerdown", (e) => {
      rail.setPointerCapture(e.pointerId);
      knob.classList.remove("back");
      drag = { x0: e.clientX };
    });
    rail.addEventListener("pointermove", (e) => {
      if (!drag) return;
      const dx = Math.max(-room(), Math.min(room(), e.clientX - drag.x0));
      knob.style.setProperty("--x", `${dx}px`);
      drag.dx = dx;
    });
    const release = () => {
      if (!drag) return;
      // Past a third of the way: a skip, in that direction. Then the knob springs home.
      if (Math.abs(drag.dx || 0) > room() / 3) skip(Math.sign(drag.dx));
      drag = null;
      knob.classList.add("back");
      knob.style.setProperty("--x", "0px");
    };
    rail.addEventListener("pointerup", release);
    rail.addEventListener("pointercancel", release);
    swipe.querySelectorAll(".b-skip").forEach((b) => b.addEventListener("click", () => skip(Number(b.dataset.dir))));
    knob.addEventListener("keydown", (e) => {
      if (e.key === "ArrowRight") skip(1);
      else if (e.key === "ArrowLeft") skip(-1);
      else return;
      e.preventDefault();
    });
  }

  // ---------- corners: double-click (or Enter) fires ----------
  const picks = ["Yours", "Mute mic", "Play / pause", "Next track", "App windows", "Show desktop", "Spotlight", "Screenshot", "Sleep display"];
  root.querySelectorAll(".b-corner").forEach((c) => {
    const name = c.querySelector(".b-name");
    let p = 0;
    const fire = () => {
      c.classList.remove("tap");
      void c.offsetWidth;
      c.classList.add("tap");
      if (c.classList.contains("b-pick")) {
        // The free corner shows what it could be.
        p = (p + 1) % picks.length;
        name.classList.add("swap");
        setTimeout(() => {
          name.textContent = picks[p];
          name.classList.remove("swap");
        }, reduced ? 0 : 160);
        return;
      }
      const on = !c.classList.contains("on");
      // Mission Control has no "on": it just flashes.
      if (c.dataset.corner === "tl") {
        c.classList.add("on");
        setTimeout(() => c.classList.remove("on"), 700);
        return;
      }
      c.classList.toggle("on", on);
      if (name.dataset.on) name.textContent = on ? name.dataset.on : name.dataset.off;
    };
    c.addEventListener("dblclick", fire);
    // A single click shows the ring, so it's clear the corner wants a second one.
    c.addEventListener("click", (e) => {
      if (e.detail === 1) {
        c.classList.remove("tap");
        void c.offsetWidth;
        c.classList.add("tap");
      }
    });
    c.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        fire();
      }
    });
  });
})();
