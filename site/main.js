// Brink landing page. Vanilla JS + GSAP/ScrollTrigger + Lenis, no build step.
(() => {
  "use strict";

  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const clamp = (v, a = 0, b = 1) => Math.min(b, Math.max(a, v));
  const lerp = (a, b, t) => a + (b - a) * t;
  const smooth = (e0, e1, x) => { const t = clamp((x - e0) / (e1 - e0)); return t * t * (3 - 2 * t); };
  const inOut = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
  const bump = (a, b, t, w = 0.02) => smooth(a - w, a, t) * (1 - smooth(b, b + w, t));

  const root = document.documentElement;
  const RM = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const FINE = matchMedia("(hover: hover) and (pointer: fine)").matches;
  const HAS_GSAP = !!(window.gsap && window.ScrollTrigger);
  const MOTION = HAS_GSAP && !RM;
  if (!MOTION) root.classList.add("static");

  const DMG = "https://github.com/MuhammadaliMS/brink-releases/releases/latest/download/Brink.dmg";

  // Download clicks go to GA4 as file_download through Cloudflare Zaraz (injected on all of zerowing.in).
  document.addEventListener("click", (e) => {
    const a = e.target.closest(`a[href="${DMG}"]`);
    if (!a || !window.zaraz) return;
    const placement = a.classList.contains("nav-cta") ? "nav"
      : a.closest("footer") ? "footer"
      : a.closest("#top") ? "hero"
      : a.closest("section[id]")?.id || "page";
    zaraz.track("file_download", { file_name: "Brink.dmg", link_url: DMG, link_text: a.textContent.trim(), placement });
  });

  // ---------------------------------------------------------------------------
  // Trackpad component
  // ---------------------------------------------------------------------------
  const EDGE_TONE = { right: "teal", left: "amber", top: "ivory", bottom: "signal", corner: "signal" };

  function Trackpad(host) {
    host.innerHTML = `
      <i class="tp-spill s-left"></i><i class="tp-spill s-right"></i><i class="tp-spill s-top"></i><i class="tp-spill s-bottom"></i>
      <div class="tp-body">
        <i class="tp-sheen"></i>
        <i class="tp-glow g-left"></i><i class="tp-glow g-right"></i><i class="tp-glow g-top"></i><i class="tp-glow g-bottom"></i>
        <i class="tp-rail r-left"><b></b></i><i class="tp-rail r-right"><b></b></i><i class="tp-rail r-top"><b></b></i><i class="tp-rail r-bottom"><b></b></i>
        <i class="tp-corner"></i><i class="tp-rip"></i><i class="tp-rip"></i>
        <span class="tp-skip mono">› › ›</span>
        <i class="tp-pool"></i>
        <i class="tp-finger"><i></i></i>
      </div>
      <i class="tp-pt" style="left:0;top:0"></i><i class="tp-pt" style="left:100%;top:0"></i>
      <i class="tp-pt" style="left:100%;top:100%"></i><i class="tp-pt" style="left:0;top:100%"></i>`;
    const q = (s) => host.querySelector(s);
    const glow = { left: q(".g-left"), right: q(".g-right"), top: q(".g-top"), bottom: q(".g-bottom") };
    const fill = { left: q(".r-left b"), right: q(".r-right b"), top: q(".r-top b"), bottom: q(".r-bottom b") };
    const spill = { left: q(".s-left"), right: q(".s-right"), top: q(".s-top"), bottom: q(".s-bottom") };
    const finger = q(".tp-finger"), tick = q(".tp-finger i"), pool = q(".tp-pool");
    const corner = q(".tp-corner"), rips = [...host.querySelectorAll(".tp-rip")], skip = q(".tp-skip");
    const pts = [...host.querySelectorAll(".tp-pt")];
    let W = host.offsetWidth, H = host.offsetHeight, last = null;
    new ResizeObserver(() => { W = host.offsetWidth; H = host.offsetHeight; if (last) set(last); }).observe(host);

    function set(s) {
      last = s;
      if (s.tone && host.dataset.tone !== s.tone) host.dataset.tone = s.tone;
      const x = s.fx * W, y = s.fy * H, t = s.touch;
      finger.style.transform = `translate3d(${x.toFixed(1)}px,${y.toFixed(1)}px,0) scale(${(0.8 + 0.2 * t).toFixed(3)})`;
      finger.style.opacity = t.toFixed(3);
      pool.style.transform = `translate3d(${x.toFixed(1)}px,${y.toFixed(1)}px,0)`;
      pool.style.opacity = (t * 0.95).toFixed(3);
      for (const k in glow) { const g = (s.glow[k] || 0).toFixed(3); glow[k].style.opacity = g; spill[k].style.opacity = g; }
      fill.left.style.transform = `scaleY(${s.level.left.toFixed(4)})`;
      fill.right.style.transform = `scaleY(${s.level.right.toFixed(4)})`;
      fill.top.style.transform = `scaleX(${s.level.top.toFixed(4)})`;
      fill.bottom.style.transform = `scaleX(${(s.glow.bottom || 0).toFixed(3)})`;
      corner.classList.toggle("on", !!s.cornerOn);
      const r = s.rips || [0, 0];
      rips.forEach((el, i) => {
        const p = r[i] || 0;
        el.style.opacity = p > 0 && p < 1 ? ((1 - p) * 0.9).toFixed(3) : "0";
        el.style.transform = `scale(${(0.5 + p * 1.6).toFixed(3)})`;
      });
      skip.style.opacity = (s.skip || 0).toFixed(3);
      skip.style.transform = `translateX(${((s.skip || 0) * 2).toFixed(2)}cqw)`;
    }
    function pulse() {
      if (RM || !tick.animate) return;
      tick.animate([{ transform: "scale(.7)", opacity: 0.9 }, { transform: "scale(1.7)", opacity: 0 }], { duration: 420, easing: "cubic-bezier(.16,1,.3,1)" });
    }
    return { host, set, pulse, pts, get state() { return last; } };
  }

  // ---------------------------------------------------------------------------
  // HUD component (macOS on-screen display)
  // ---------------------------------------------------------------------------
  function Hud(host) {
    let rays = "", krays = "";
    for (let i = 0; i < 8; i++) {
      const a = (i / 8) * Math.PI * 2, c = Math.cos(a), s = Math.sin(a);
      rays += `<line x1="${(32 + c * 14).toFixed(2)}" y1="${(32 + s * 14).toFixed(2)}" x2="${(32 + c * 19.5).toFixed(2)}" y2="${(32 + s * 19.5).toFixed(2)}"/>`;
    }
    for (let i = 0; i < 5; i++) {
      const a = Math.PI + (i / 4) * Math.PI, c = Math.cos(a), s = Math.sin(a);
      krays += `<line x1="${(32 + c * 12).toFixed(2)}" y1="${(34 + s * 12).toFixed(2)}" x2="${(32 + c * 17).toFixed(2)}" y2="${(34 + s * 17).toFixed(2)}"/>`;
    }
    host.innerHTML = `
      <div class="hud-body mode-vol">
        <svg class="hud-glyph" viewBox="0 0 64 64" aria-hidden="true">
          <g class="gl gl-vol">
            <path class="fp" d="M7 25h9.5L29.5 14v36L16.5 39H7z"/>
            <path class="wv" d="M36 25.5a9 9 0 0 1 0 13"/><path class="wv" d="M41.5 20a16.5 16.5 0 0 1 0 24"/><path class="wv" d="M47 14.5a24 24 0 0 1 0 35"/>
            <path class="x" d="M37 26l12 12m0-12-12 12"/>
          </g>
          <g class="gl gl-sun"><circle class="fp" cx="32" cy="32" r="9"/><g class="rays">${rays}</g></g>
          <g class="gl gl-key">
            <rect class="fp" x="12" y="40" width="40" height="12" rx="3"/>
            <path class="fp" d="M25 34a7 7 0 0 1 14 0z"/>
            <g class="krays">${krays}</g>
          </g>
        </svg>
        <svg class="hud-media" viewBox="0 0 132 40" aria-hidden="true">
          <g class="m m-prev"><path class="st" d="M6 9v22"/><path d="M33 9 12 20l21 11z"/></g>
          <g class="m m-play"><path d="M50 8v24l18-12z"/><path class="st" d="M76 10v20M84 10v20"/></g>
          <g class="m m-next"><path d="M99 9l21 11-21 11z"/><path class="st" d="M126 9v22"/></g>
        </svg>
        <div class="hud-bar">${"<i><b></b></i>".repeat(16)}</div>
      </div>`;
    const body = host.firstElementChild;
    const cells = [...host.querySelectorAll(".hud-bar b")];
    const waves = [...host.querySelectorAll(".wv")];
    const raysG = host.querySelector(".rays"), kraysG = host.querySelector(".krays");
    const media = { prev: host.querySelector(".m-prev"), play: host.querySelector(".m-play"), next: host.querySelector(".m-next") };
    let mode = "vol", lastStep = -1;

    function set(s, onStep) {
      if (s.mode !== mode) { body.classList.remove(`mode-${mode}`); mode = s.mode; body.classList.add(`mode-${mode}`); }
      const show = s.show;
      host.style.opacity = show.toFixed(3);
      host.style.transform = `translate3d(0,${((1 - show) * 10).toFixed(2)}px,0) scale(${(0.94 + 0.06 * show).toFixed(4)})`;
      const L = mode === "mute" ? 0 : clamp(s.level);
      const step = Math.round(L * 16);
      if (mode !== "media") {
        cells.forEach((c, i) => (c.style.transform = `scaleX(${clamp(step - i)})`));
        if (step !== lastStep) { if (lastStep >= 0 && onStep && show > 0.5) onStep(); lastStep = step; }
      }
      if (mode === "vol") {
        const on = L <= 0.001 ? 0 : L < 0.34 ? 1 : L < 0.67 ? 2 : 3;
        waves.forEach((w, i) => (w.style.opacity = i < on ? 1 : 0.22));
      }
      if (mode === "sun") { raysG.style.transform = `scale(${(0.62 + 0.5 * L).toFixed(3)})`; raysG.style.opacity = (0.35 + 0.65 * L).toFixed(3); }
      if (mode === "key") { kraysG.style.transform = `scale(${(0.7 + 0.42 * L).toFixed(3)})`; kraysG.style.opacity = (0.25 + 0.75 * L).toFixed(3); }
      if (mode === "media") {
        const h = s.media || 0, dir = s.mediaDir || 1;
        const hot = dir > 0 ? media.next : media.prev, cold = dir > 0 ? media.prev : media.next;
        hot.style.transform = `scale(${(1 + 0.22 * h).toFixed(3)})`;
        hot.style.opacity = (0.5 + 0.5 * h).toFixed(3);
        cold.style.transform = "none"; cold.style.opacity = "0.5";
        media.play.style.opacity = s.playing === false ? "1" : (0.85 - 0.35 * h).toFixed(3);
      }
    }
    return { set, host };
  }

  // A trackpad + HUD pair that pulses a haptic tick on each 1/16 step.
  function Rig(tpHost, hudHost) {
    const tp = Trackpad(tpHost), hud = Hud(hudHost);
    return { tp, hud, set(st) { tp.set(st.tp); hud.set(st.hud, tp.pulse); } };
  }

  const baseTp = () => ({ fx: 0.965, fy: 0.7, touch: 0, tone: "teal", glow: {}, level: { left: 0.55, right: 0.5, top: 0.4 }, rips: [0, 0], skip: 0 });

  // ---------------------------------------------------------------------------
  // Split headings into masked words
  // ---------------------------------------------------------------------------
  function split(el) {
    if (el.dataset.splitDone) return;
    el.dataset.splitDone = "1";
    el.setAttribute("aria-label", el.textContent.replace(/\s+/g, " ").trim());
    let i = 0;
    const wrap = (node) => {
      const w = document.createElement("span"); w.className = "w"; w.setAttribute("aria-hidden", "true");
      const wi = document.createElement("span"); wi.className = "wi"; wi.style.setProperty("--i", i++);
      wi.appendChild(node); w.appendChild(wi); return w;
    };
    const kids = [...el.childNodes];
    el.textContent = "";
    for (const n of kids) {
      if (n.nodeType === 3) {
        n.textContent.split(/(\s+)/).forEach((part) => {
          if (!part) return;
          if (/^\s+$/.test(part)) el.appendChild(document.createTextNode(" "));
          else el.appendChild(wrap(document.createTextNode(part)));
        });
      } else if (n.nodeName === "BR") el.appendChild(n);
      else el.appendChild(wrap(n));
    }
  }
  $$("[data-split]").forEach(split);

  // ---------------------------------------------------------------------------
  // Lenis smooth scroll wired to ScrollTrigger
  // ---------------------------------------------------------------------------
  let lenis = null;
  if (MOTION) {
    gsap.registerPlugin(ScrollTrigger);
    ScrollTrigger.config({ ignoreMobileResize: true });
    if (window.CustomEase) {
      gsap.registerPlugin(CustomEase);
      CustomEase.create("out", "0.16,1,0.3,1");
      CustomEase.create("inOut", "0.65,0,0.35,1");
    }
    if (window.Lenis) {
      lenis = new Lenis({ lerp: 0.1, smoothWheel: true, syncTouch: false });
      lenis.on("scroll", ScrollTrigger.update);
      gsap.ticker.add((time) => lenis.raf(time * 1000));
      gsap.ticker.lagSmoothing(0);
    }
  }
  const EASE_OUT = MOTION && window.CustomEase ? "out" : "expo.out";

  // anchor links
  $$('a[href^="#"]').forEach((a) => {
    a.addEventListener("click", (e) => {
      const id = a.getAttribute("href");
      const t = id === "#top" ? document.body : $(id);
      if (!t) return;
      e.preventDefault();
      if (lenis) lenis.scrollTo(id === "#top" ? 0 : t, { duration: 1.4, easing: (x) => 1 - Math.pow(1 - x, 4) });
      else t.scrollIntoView({ behavior: RM ? "auto" : "smooth" });
      if (t.focus && id !== "#top") { t.setAttribute("tabindex", "-1"); t.focus({ preventScroll: true }); }
    });
  });

  // ---------------------------------------------------------------------------
  // Nav: compresses into a pill once you leave the top
  // ---------------------------------------------------------------------------
  const nav = $("#nav");
  // The pill is as wide as what it holds: the centred links, plus room each side for the
  // wider of the logo and the Download button (so neither ever runs into a link).
  const fitNav = () => {
    const w = nav.offsetWidth;
    const side = Math.max($(".brand").offsetWidth + 12, $(".nav-cta").offsetWidth + 8) + 20;
    const target = innerWidth > 860 ? Math.min(w, $(".nav-links").offsetWidth + 2 * side) : w;
    nav.style.setProperty("--sq", `${Math.max(0, (w - target) / 2)}px`);
  };
  fitNav();
  addEventListener("resize", fitNav, { passive: true });
  const onScrollNav = () => nav.classList.toggle("compact", scrollY > 60);
  addEventListener("scroll", onScrollNav, { passive: true });
  onScrollNav();

  // ---------------------------------------------------------------------------
  // HERO: entry + autonomous fingertip loop
  // ---------------------------------------------------------------------------
  const heroRig = Rig($("#tp-hero"), $("#hud-hero"));
  const HERO_T = 9.2;
  function heroState(t) {
    t %= HERO_T;
    const A = t < 4.5, st = A ? t : t - 4.6;
    const touch = smooth(0, 0.35, st) * (1 - smooth(2.9, 3.3, st));
    const g1 = inOut(clamp((st - 0.35) / 2.05)), g2 = inOut(clamp((st - 2.4) / 0.5));
    const fy = lerp(0.6, 0.13, g1) + 0.1 * g2;
    const lv = A ? lerp(0.31, 0.88, g1) - 0.13 * g2 : lerp(0.25, 0.94, g1) - 0.14 * g2;
    const glowV = 0.2 + 0.8 * smooth(0, 0.4, st) * (1 - smooth(3.0, 3.8, st));
    const show = smooth(0.15, 0.45, st) * (1 - smooth(3.7, 4.2, st));
    const tp = baseTp();
    tp.fx = A ? 0.965 : 0.035; tp.fy = fy; tp.touch = touch; tp.tone = A ? "teal" : "amber";
    tp.glow = A ? { right: glowV, left: 0.12 } : { left: glowV, right: 0.12 };
    tp.level = { right: A ? lv : 0.75, left: A ? 0.8 : lv, top: 0 };
    return { tp, hud: { mode: A ? "vol" : "sun", level: lv, show } };
  }
  let heroT = 0.4, heroRun = false, heroVisible = true, heroLast = 0, heroRaf = 0;
  const heroTick = (now) => {
    if (!heroRun) return;
    const dt = Math.min(0.05, (now - heroLast) / 1000 || 0); heroLast = now;
    heroT += dt; heroRig.set(heroState(heroT));
    heroRaf = requestAnimationFrame(heroTick);
  };
  const heroPlay = () => {
    const want = MOTION && heroVisible && !document.hidden;
    if (want && !heroRun) { heroRun = true; heroLast = performance.now(); heroRaf = requestAnimationFrame(heroTick); }
    if (!want && heroRun) { heroRun = false; cancelAnimationFrame(heroRaf); }
  };
  heroRig.set(heroState(MOTION ? 0.4 : 2.2));
  new IntersectionObserver((es) => { heroVisible = es[0].isIntersecting; heroPlay(); }).observe($("#hero-rig"));
  document.addEventListener("visibilitychange", heroPlay);

  if (MOTION) {
    const tl = gsap.timeline({ defaults: { ease: EASE_OUT } });
    gsap.set("[data-hero], [data-hero-title], [data-hero-rig], #nav", { opacity: 0 });
    tl.fromTo(nav, { y: -18, opacity: 0 }, { y: 0, opacity: 1, duration: 0.6, clearProps: "transform" }, 0)
      .fromTo("[data-hero-title]", { opacity: 0, filter: "blur(8px)" }, { opacity: 1, filter: "blur(0px)", duration: 0.6, clearProps: "filter" }, 0.04)
      .add(() => $("[data-hero-title]").classList.add("in"), 0.02)
      .fromTo(".hero .eyebrow", { opacity: 0, y: 14, filter: "blur(6px)" }, { opacity: 1, y: 0, filter: "blur(0px)", duration: 0.6, clearProps: "filter" }, 0)
      .fromTo(".hero-sub", { opacity: 0, y: 18, filter: "blur(8px)" }, { opacity: 1, y: 0, filter: "blur(0px)", duration: 0.6, clearProps: "filter" }, 0.18)
      .fromTo(".hero-cta", { opacity: 0, y: 18, filter: "blur(8px)" }, { opacity: 1, y: 0, filter: "blur(0px)", duration: 0.55, clearProps: "filter" }, 0.25)
      .fromTo("[data-hero-rig]", { y: 120, opacity: 0 }, { y: 0, opacity: 1, duration: 0.7 }, 0.1)
      .add(heroPlay, 0.3);
    // trackpad drifts up and settles as you leave the hero
    gsap.to("#hero-rig", { y: -60, ease: "none", scrollTrigger: { trigger: ".hero", start: "top top", end: "bottom top", scrub: true } });
    gsap.to(".hero-copy", { y: -40, opacity: 0.2, ease: "none", scrollTrigger: { trigger: ".hero", start: "20% top", end: "bottom top", scrub: true } });
  } else {
    $("[data-hero-title]").classList.add("in");
  }

  // ---------------------------------------------------------------------------
  // STORY: a flat, front-on MacBook. The finger works the trackpad, the Mac answers.
  // ---------------------------------------------------------------------------
  const F = (n) => [`F${n}`, 1];
  const KEYS = [
    ["fn", [["esc", 1.45, "mod"], ...[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12].map(F), ["", 1, "touch"]]],
    ["", [..."`1234567890-=".split("").map((c) => [c]), ["delete", 1.5, "mod r"]]],
    ["", [["tab", 1.5, "mod"], ..."QWERTYUIOP[]\\".split("").map((c) => [c])]],
    ["", [["caps lock", 1.8, "mod"], ..."ASDFGHJKL;'".split("").map((c) => [c]), ["return", 1.7, "mod r"]]],
    ["", [["shift", 2.3, "mod"], ..."ZXCVBNM,./".split("").map((c) => [c]), ["shift", 2.2, "mod r"]]],
    ["", [["fn", 1, "mod"], ["control", 1, "mod"], ["option", 1, "mod"], ["command", 1.25, "mod"], ["", 5], ["command", 1.25, "mod r"], ["option", 1, "mod r"], ["", 3, "arrows"]]],
  ];
  const esc = (c) => c.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  const kbLayer = (cls) => `<div class="${cls}">${KEYS.map(([rc, ks]) => `<div class="kb-row ${rc}">${ks.map(([l, w = 1, kc = ""]) =>
    kc === "arrows" ? `<span class="key arrows" style="--w:${w}"><i>◀</i><i>▲</i><i>▼</i><i>▶</i></span>` : `<span class="key ${kc}" style="--w:${w}">${esc(l)}</span>`).join("")}</div>`).join("")}</div>`;

  function MacRig(mac) {
    const kb = mac.querySelector(".kb");
    kb.innerHTML = `<i class="kb-glow"></i>${kbLayer("kb-layer")}${kbLayer("kb-layer kb-lit")}`;
    const tp = Trackpad(mac.querySelector(".tp")), hud = Hud(mac.querySelector(".hud"));
    const lit = kb.querySelector(".kb-lit"), glow = kb.querySelector(".kb-glow");
    const dim = mac.querySelector(".scr-dim"), brink = mac.querySelector(".scr-brink");
    const npA = mac.querySelector(".np-a"), npB = mac.querySelector(".np-b");
    const artA = mac.querySelector(".np-art-a"), artB = mac.querySelector(".np-art-b");
    const nextIc = mac.querySelector(".np-ctl path:nth-child(3)");
    let track = 7, lastMask = "";
    const pad = (n) => String(n).padStart(2, "0");
    const setTracks = (n) => {
      track = n;
      npA.firstChild.textContent = `Track ${pad(n)}`;
      npB.firstChild.textContent = `Track ${pad(n + 1)}`;
    };
    let last = null;
    function set(st) {
      last = st;
      tp.set(st.tp);
      hud.set(st.hud, tp.pulse);
      const s = st.scr;
      dim.style.opacity = ((1 - s.bright) * 0.84).toFixed(3);
      const k = clamp(s.keys);
      lit.style.opacity = clamp(k * 1.15).toFixed(3);
      glow.style.opacity = (k * 0.9).toFixed(3);
      const sweep = clamp(k / 0.85);
      const m = sweep >= 0.999 ? "none" : `linear-gradient(90deg,#000 ${(sweep * 110 - 12).toFixed(1)}%,transparent ${(sweep * 110).toFixed(1)}%)`;
      if (m !== lastMask) { lit.style.webkitMaskImage = m; lit.style.maskImage = m; lastMask = m; }
      const n = clamp(s.np);
      npA.style.transform = `translate3d(${(-n * 70).toFixed(1)}%,0,0)`; npA.style.opacity = (1 - n).toFixed(3);
      npB.style.transform = `translate3d(${((1 - n) * 70).toFixed(1)}%,0,0)`; npB.style.opacity = n.toFixed(3);
      artA.style.opacity = (1 - n).toFixed(3); artB.style.opacity = n.toFixed(3);
      nextIc.style.transform = `scale(${(1 + 0.35 * (s.npHot || 0)).toFixed(3)})`;
      brink.classList.toggle("on", st.tp.touch > 0.3);
    }
    return { tp, hud, set, setTracks, get track() { return track; }, get state() { return last; } };
  }

  const chapters = $$("#chapters .chapter");
  const prog = $$("#story-progress li");
  const mobile = () => innerWidth <= 860;
  const macEl = $("#mac");

  // static stills (reduced motion, or no GSAP): one Mac per chapter, built before the live one
  if (!MOTION) {
    const stills = [
      { tp: { fx: 0.965, fy: 0.89 - 0.78 * 0.8, touch: 1, tone: "teal", glow: { right: 1 }, level: { left: 0.55, right: 0.8, top: 0 } }, hud: { mode: "vol", level: 0.8, show: 1 }, scr: { bright: 0.8, keys: 0, np: 0 } },
      { tp: { fx: 0.035, fy: 0.89 - 0.78 * 0.35, touch: 1, tone: "amber", glow: { left: 1 }, level: { left: 0.35, right: 0.5, top: 0 } }, hud: { mode: "sun", level: 0.35, show: 1 }, scr: { bright: 0.35, keys: 0, np: 0 } },
      { tp: { fx: 0.8, fy: 0.05, touch: 1, tone: "ivory", glow: { top: 1 }, level: { left: 0.55, right: 0.5, top: 0.9 } }, hud: { mode: "key", level: 0.9, show: 1 }, scr: { bright: 1, keys: 0.9, np: 0 } },
      { tp: { fx: 0.72, fy: 0.955, touch: 1, tone: "signal", glow: { bottom: 1 }, skip: 1, level: { left: 0.55, right: 0.5, top: 0.9 } }, hud: { mode: "media", show: 0 }, scr: { bright: 1, keys: 0.9, np: 1, npHot: 1 } },
    ];
    chapters.forEach((el, i) => {
      const v = el.querySelector(".ch-visual");
      const clone = macEl.cloneNode(true);
      clone.removeAttribute("id"); clone.querySelectorAll("[id]").forEach((n) => n.removeAttribute("id"));
      clone.setAttribute("aria-hidden", "true"); clone.removeAttribute("role");
      v.appendChild(clone);
      MacRig(clone).set({ tp: { ...baseTp(), ...stills[i].tp }, hud: stills[i].hud, scr: stills[i].scr });
    });
  }

  const storyRig = MacRig(macEl);
  const ch = (p, i) => clamp(p * 4 - i);
  const slideG = (t) => inOut(clamp((t - 0.2) / 0.6));
  const fyOf = (lv) => 0.89 - 0.78 * lv;
  const volOf = (t) => lerp(0.25, 0.88, slideG(t));
  const brightOf = (t) => (t < 0.5 ? lerp(0.8, 0.2, inOut(clamp((t - 0.16) / 0.32))) : lerp(0.2, 1, inOut(clamp((t - 0.52) / 0.32))));
  const keysOf = (t) => lerp(0, 0.95, slideG(t));

  function storyState(p) {
    const c = Math.min(3, Math.floor(p * 4)), t = p * 4 - c;
    const tp = baseTp();
    const vol = volOf(ch(p, 0)), bright = brightOf(ch(p, 1)), keys = keysOf(ch(p, 2));
    tp.level = { right: vol, left: bright, top: keys };
    const glowIn = smooth(0.02, 0.14, t) * (c === 3 ? 1 : 1 - smooth(0.9, 1, t));
    const touch = smooth(0.12, 0.2, t) * (1 - smooth(0.82, 0.9, t));
    const dip = c > 0 ? smooth(0.04, 0.14, t) : 1;
    const out = c < 3 ? 1 - smooth(0.9, 1, t) : 1;
    const scr = { bright, keys, np: 0, npHot: 0 };
    let hud;
    if (c === 0) {
      Object.assign(tp, { fx: 0.965, fy: fyOf(vol), touch, tone: "teal", glow: { right: glowIn } });
      hud = { mode: "vol", level: vol, show: smooth(0.1, 0.2, t) * out };
    } else if (c === 1) {
      Object.assign(tp, { fx: 0.035, fy: fyOf(bright), touch, tone: "amber", glow: { left: glowIn } });
      hud = { mode: "sun", level: bright, show: dip * out };
    } else if (c === 2) {
      Object.assign(tp, { fx: 0.14 + 0.72 * keys / 0.95, fy: 0.05, touch, tone: "ivory", glow: { top: glowIn } });
      hud = { mode: "key", level: keys, show: dip * out };
    } else {
      scr.np = inOut(clamp((t - 0.28) / 0.16));
      scr.npHot = bump(0.3, 0.42, t, 0.04);
      if (t < 0.58) {
        const sw = inOut(clamp((t - 0.16) / 0.28));
        Object.assign(tp, {
          fx: lerp(0.28, 0.74, sw), fy: 0.955, tone: "signal",
          touch: smooth(0.08, 0.16, t) * (1 - smooth(0.46, 0.52, t)),
          glow: { bottom: smooth(0.04, 0.14, t) * (1 - smooth(0.5, 0.58, t)) },
          skip: smooth(0.2, 0.36, t) * (1 - smooth(0.44, 0.52, t)),
        });
        hud = { mode: "vol", level: vol, show: 0 };
      } else {
        const r1 = clamp((t - 0.66) / 0.14), r2 = clamp((t - 0.74) / 0.14);
        Object.assign(tp, {
          fx: 0.962, fy: 0.055, tone: "signal",
          touch: bump(0.66, 0.69, t) + bump(0.74, 0.77, t),
          glow: {}, cornerOn: t > 0.62, rips: [r1, r2],
        });
        hud = { mode: t > 0.76 ? "mute" : "vol", level: vol, show: smooth(0.75, 0.8, t) };
      }
    }
    return { tp, hud, scr };
  }

  function renderStory(p) {
    if (manual.active) return;
    storyRig.set(storyState(p));
    chapters.forEach((el, i) => {
      let d = p * 4 - (i + 0.5);
      if (i === 0 && d < 0) d = 0;
      if (i === 3 && d > 0) d = 0;
      const a = clamp(1 - (Math.abs(d) - 0.3) / 0.2);
      el.style.opacity = a.toFixed(3);
      el.style.transform = `translate3d(0,${(-d * 60).toFixed(1)}px,0)`;
      el.style.visibility = a < 0.01 ? "hidden" : "visible";
    });
    prog.forEach((li, i) => {
      li.querySelector("b").style.transform = `scaleX(${ch(p, i).toFixed(4)})`;
      li.classList.toggle("on", Math.min(3, Math.floor(p * 4)) === i);
    });
  }

  // drag on the trackpad: maps the pointer back onto the pad (works for any projection)
  const manual = { active: false, timer: 0 };
  function homography(P) {
    const [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = P;
    const dx1 = x1 - x2, dx2 = x3 - x2, dy1 = y1 - y2, dy2 = y3 - y2;
    const sx = x0 - x1 + x2 - x3, sy = y0 - y1 + y2 - y3;
    let g = 0, h = 0;
    if (Math.abs(sx) > 1e-6 || Math.abs(sy) > 1e-6) {
      const den = dx1 * dy2 - dx2 * dy1;
      g = (sx * dy2 - dx2 * sy) / den; h = (dx1 * sy - sx * dy1) / den;
    }
    const a = x1 - x0 + g * x1, b = x3 - x0 + h * x3, c = x0;
    const d = y1 - y0 + g * y1, e = y3 - y0 + h * y3, f = y0;
    const A = e - f * h, B = c * h - b, C = b * f - c * e;
    const D = f * g - d, E = a - c * g, Fq = c * d - a * f;
    const G = d * h - e * g, Hh = b * g - a * h, I = a * e - b * d;
    return (x, y) => { const w = G * x + Hh * y + I; return [(A * x + B * y + C) / w, (D * x + E * y + Fq) / w]; };
  }
  (function initDrag() {
    const host = storyRig.tp.host;
    let map = null, drag = null, taps = [], muted = false, st = null;
    const copy = () => {
      const s = storyRig.state || storyState(storyP);
      return { tp: { ...s.tp, glow: {}, level: { ...s.tp.level }, rips: [0, 0], skip: 0 }, hud: { ...s.hud }, scr: { ...s.scr, npHot: 0 } };
    };
    const release = () => {
      clearTimeout(manual.timer);
      manual.timer = setTimeout(() => { manual.active = false; renderStory(storyP); }, 1800);
    };
    const tween = (ms, fn, done) => {
      const t0 = performance.now();
      const f = (n) => { const k = clamp((n - t0) / ms); fn(k); storyRig.set(st); if (k < 1) requestAnimationFrame(f); else if (done) done(); };
      requestAnimationFrame(f);
    };
    host.addEventListener("pointerdown", (e) => {
      if (e.button > 0) return;
      map = homography(storyRig.tp.pts.map((p) => { const r = p.getBoundingClientRect(); return [r.left, r.top]; }));
      const [u, v] = map(e.clientX, e.clientY);
      if (u < -0.05 || u > 1.05 || v < -0.05 || v > 1.05) return;
      clearTimeout(manual.timer);
      if (!manual.active) st = copy();
      manual.active = true;
      let edge;
      if (u > 0.84 && v < 0.22) edge = "corner";
      else {
        const dl = u, dr = 1 - u, dt = v * 1.6, db = (1 - v) * 1.6;
        const m = Math.min(dl, dr, dt, db);
        edge = m === dr ? "right" : m === dl ? "left" : m === dt ? "top" : "bottom";
      }
      drag = { edge, u0: u, skipped: false };
      if (e.pointerType !== "touch") host.setPointerCapture(e.pointerId);
      move(e);
    });
    const move = (e) => {
      if (!drag) return;
      const [u, v] = map(e.clientX, e.clientY);
      const { edge } = drag, tp = st.tp;
      tp.touch = 1; tp.tone = EDGE_TONE[edge]; tp.glow = {}; tp.skip = 0; tp.cornerOn = edge === "corner";
      if (edge === "right" || edge === "left") {
        const lv = clamp((0.89 - v) / 0.78);
        tp.fx = edge === "right" ? 0.965 : 0.035; tp.fy = fyOf(lv);
        tp.level[edge] = lv; tp.glow[edge] = 1;
        if (edge === "right") { muted = false; st.hud = { mode: "vol", level: lv, show: 1 }; }
        else { st.scr.bright = lv; st.hud = { mode: "sun", level: lv, show: 1 }; }
      } else if (edge === "top") {
        const lv = clamp((u - 0.14) / 0.72);
        tp.fx = 0.14 + 0.72 * lv; tp.fy = 0.05; tp.level.top = lv; tp.glow.top = 1;
        st.scr.keys = lv * 0.95;
        st.hud = { mode: "key", level: lv, show: 1 };
      } else if (edge === "bottom") {
        tp.fx = clamp(u, 0.06, 0.94); tp.fy = 0.955; tp.glow.bottom = 1;
        const du = u - drag.u0;
        tp.skip = clamp(Math.abs(du) / 0.18);
        st.hud = { ...st.hud, show: 0 };
        if (!drag.skipped && Math.abs(du) > 0.18) {
          drag.skipped = true; storyRig.tp.pulse();
          const next = du > 0 ? storyRig.track + 1 : Math.max(1, storyRig.track - 1);
          if (du > 0) {
            tween(520, (k) => { st.scr.np = inOut(k); st.scr.npHot = Math.sin(k * Math.PI); }, () => { storyRig.setTracks(next); st.scr.np = 0; storyRig.set(st); });
          } else { storyRig.setTracks(next); }
        }
      } else {
        tp.fx = 0.962; tp.fy = 0.055;
      }
      storyRig.set(st);
    };
    host.addEventListener("pointermove", move);
    const up = () => {
      if (!drag) return;
      if (drag.edge === "corner") {
        const now = performance.now();
        taps = taps.filter((t) => now - t < 450); taps.push(now);
        if (taps.length >= 2) { muted = !muted; taps = []; storyRig.tp.pulse(); }
        st.hud = { mode: muted ? "mute" : "vol", level: st.tp.level.right, show: 1 };
        tween(520, (k) => { st.tp.rips = [k, 0]; });
      }
      st.tp.touch = 0;
      storyRig.set(st);
      drag = null;
      release();
    };
    host.addEventListener("pointerup", up);
    host.addEventListener("pointercancel", () => { if (drag) { st.tp.touch = 0; storyRig.set(st); drag = null; release(); } });
    if (!FINE) $("#story-hint").lastChild.textContent = "Tap or swipe an edge";
  })();

  let storyP = 0;
  storyRig.set(storyState(0));
  if (MOTION) {
    renderStory(0);
    const mm = gsap.matchMedia();

    // Phones: the four chapters, and the grid stays its own section below.
    mm.add("(max-width: 860px)", () => {
      const proxy = { p: 0 };
      gsap.to(proxy, {
        p: 1, ease: "none",
        scrollTrigger: {
          trigger: "#edges", start: "top top", end: () => `+=${innerHeight * 3.4}`,
          pin: "#story-stage", scrub: 0.8, anticipatePin: 1, invalidateOnRefresh: true,
        },
        onUpdate: () => { storyP = proxy.p; renderStory(proxy.p); },
      });
    });

    // Wider screens: after the four chapters the Mac glides to the middle, the camera dives into
    // its trackpad, and the trackpad opens out into the grid: its face becomes the middle tile,
    // and each edge and corner arrives from the side it lives on.
    mm.add("(min-width: 861px)", () => {
      const STORY = 4, OUTRO = 2.6; // in screen heights
      const stage = $("#story-stage"), text = $(".story-text"), hint = $("#story-hint");
      const section = $("#corners"), bento = $("#corners .bento");
      const face = $(".b-face", bento), title = $(".b-title", face);
      const tpEl = storyRig.tp.host;
      const fadeOnZoom = $$(".mac-lid, .mac-hinge, .kb", macEl);
      const layer = document.createElement("div");
      layer.className = "story-bento";
      layer.appendChild(bento);
      stage.appendChild(layer);
      section.classList.add("moved");
      // The face's words arrive with the handoff, not when the pinned stage first shows.
      $$("[data-reveal]", face).forEach((el) => el.removeAttribute("data-reveal"));
      title.classList.remove("in");
      // Where each tile comes from: the side of the trackpad it stands for.
      const FROM = { tl: [-1, -1], top: [0, -1], tr: [1, -1], left: [-1, 0], right: [1, 0], bl: [-1, 1], bottom: [0, 1], br: [1, 1] };
      const AREAS = ["tl", "top", "tr", "left", null, "right", "bl", "bottom", "br"];
      const tiles = [...bento.children].map((el, i) => ({ el, area: AREAS[i] })).filter((t) => t.area);

      let G = null;
      const measure = () => {
        macEl.style.transform = "";
        bento.style.transform = "";
        const st = stage.getBoundingClientRect();
        const rel = (r) => ({ x: r.left - st.left, y: r.top - st.top, w: r.width, h: r.height });
        // Fit the grid under the nav.
        const room = st.height - 56 - 64;
        const k = Math.min(1, room / bento.offsetHeight);
        bento.style.transform = `translate(-50%, -50%) scale(${k})`;
        G = { st: rel(st), mac: rel(macEl.getBoundingClientRect()), tp: rel(tpEl.getBoundingClientRect()), face: rel(face.getBoundingClientRect()) };
      };

      const centre = (r) => [r.x + r.w / 2, r.y + r.h / 2];
      function outro(q) {
        if (!G) measure();
        const o = [G.mac.x, G.mac.y];
        const mc = centre(G.mac), cc = centre(G.tp), fc = centre(G.face);
        const target = [G.st.w / 2, (G.st.h + 56) / 2];
        const sA = 1.06, sB = (G.face.w / G.tp.w) * 1.02;
        let s, pos, track;
        if (q < 0.25) {
          // Glide to the middle.
          const a = inOut(q / 0.25);
          s = lerp(1, sA, a); track = mc; pos = [lerp(mc[0], target[0], a), lerp(mc[1], target[1], a)];
        } else {
          // Dive into the trackpad: it ends exactly where the grid's middle tile is.
          const z = inOut(clamp((q - 0.25) / 0.4));
          const cA = [target[0] + sA * (cc[0] - mc[0]), target[1] + sA * (cc[1] - mc[1])];
          s = sA * Math.pow(sB / sA, z); track = cc; pos = [lerp(cA[0], fc[0], z), lerp(cA[1], fc[1], z)];
        }
        const tx = pos[0] - o[0] - s * (track[0] - o[0]);
        const ty = pos[1] - o[1] - s * (track[1] - o[1]);
        macEl.style.transform = q > 0 ? `translate3d(${tx.toFixed(1)}px,${ty.toFixed(1)}px,0) scale(${s.toFixed(4)})` : "";
        macEl.style.opacity = (1 - smooth(0.6, 0.72, q)).toFixed(3);
        fadeOnZoom.forEach((el) => (el.style.opacity = (1 - smooth(0.28, 0.5, q)).toFixed(3)));
        const away = (1 - smooth(0, 0.14, q)).toFixed(3);
        text.style.opacity = away;
        hint.style.opacity = away;

        // The grid.
        const f = smooth(0.58, 0.7, q);
        face.style.opacity = f.toFixed(3);
        title.classList.toggle("in", q > 0.62);
        tiles.forEach((t, i) => {
          const corner = t.area.length === 2;
          const start = (corner ? 0.74 : 0.66) + (i % 4) * 0.015;
          const e = smooth(start, start + 0.16, q);
          const [dx, dy] = FROM[t.area];
          const d = (1 - e) * 46;
          t.el.style.opacity = e.toFixed(3);
          t.el.style.transform = e < 1 ? `translate3d(${(dx * d).toFixed(1)}px,${(dy * d).toFixed(1)}px,0) scale(${(0.94 + 0.06 * e).toFixed(4)})` : "";
        });
        layer.classList.toggle("live", q > 0.9);
      }

      const draw = (P) => {
        const at = P * (STORY + OUTRO);
        storyP = clamp(at / STORY);
        renderStory(storyP);
        outro(clamp((at - STORY) / OUTRO));
      };
      const proxy = { P: 0 };
      const tw = gsap.to(proxy, {
        P: 1, ease: "none",
        scrollTrigger: {
          trigger: "#edges", start: "top top", end: () => `+=${innerHeight * (STORY + OUTRO)}`,
          pin: "#story-stage", scrub: 0.8, anticipatePin: 1, invalidateOnRefresh: true,
          // Sizes changed (or the page opened part-way down): measure again and draw where the
          // scroll really is, without waiting for the next scroll.
          onRefresh: (self) => { G = null; proxy.P = self.progress; draw(self.progress); },
        },
        onUpdate: () => draw(proxy.P),
      });
      outro(0);

      // "Corners" in the nav: to where the grid has fully opened, not past the pinned stage.
      const toGrid = (e) => {
        e.preventDefault();
        e.stopImmediatePropagation();
        const y = tw.scrollTrigger.end;
        if (lenis) lenis.scrollTo(y, { duration: 1.8, easing: (x) => 1 - Math.pow(1 - x, 4) });
        else scrollTo(0, y);
      };
      const gridLinks = $$('a[href="#corners"]');
      gridLinks.forEach((a) => a.addEventListener("click", toGrid, true));

      return () => {
        gridLinks.forEach((a) => a.removeEventListener("click", toGrid, true));
        // Back to a phone layout: the grid returns to its own section.
        section.appendChild(bento);
        layer.remove();
        section.classList.remove("moved");
        [macEl, text, hint, face, bento, ...fadeOnZoom, ...tiles.map((t) => t.el)].forEach((el) => { el.style.transform = ""; el.style.opacity = ""; });
        title.classList.add("in");
      };
    });
  }

  // ---------------------------------------------------------------------------
  // KINETIC BAND: moves with scroll, leans with scroll velocity
  // ---------------------------------------------------------------------------
  if (MOTION) {
    const rows = $$(".band-row");
    const half = rows.map((r) => r.scrollWidth / 2);
    let vel = 0, skew = 0, running = false;
    const draw = () => {
      const st = ScrollTrigger.getById("band");
      const base = st ? (scrollY - st.start) : 0;
      skew += (vel - skew) * 0.12;
      rows.forEach((r, i) => {
        const dir = +r.dataset.dir;
        let x = (base * 0.55 + i * 200) % half[i];
        x = dir < 0 ? -x : x - half[i];
        r.style.transform = `translate3d(${x.toFixed(1)}px,0,0) skewX(${(-skew * dir).toFixed(2)}deg)`;
      });
      if (Math.abs(skew - vel) < 0.01 && Math.abs(vel) < 0.01) { gsap.ticker.remove(draw); running = false; }
    };
    const kick = () => { if (!running) { running = true; gsap.ticker.add(draw); } };
    ScrollTrigger.create({
      id: "band", trigger: ".band", start: "top bottom", end: "bottom top",
      onUpdate: (self) => { vel = clamp(self.getVelocity() / 260, -9, 9); kick(); clearTimeout(draw.t); draw.t = setTimeout(() => { vel = 0; kick(); }, 90); },
      onRefresh: () => { rows.forEach((r, i) => (half[i] = r.scrollWidth / 2)); kick(); },
    });
    kick();
  }

  // ---------------------------------------------------------------------------
  // HOW IT FEELS: active row by hover/focus, else by scroll position
  // ---------------------------------------------------------------------------
  {
    const rows = $$("#index .row");
    let hovered = null, scrolled = rows[0];
    const apply = () => { const a = hovered || scrolled; rows.forEach((r) => r.classList.toggle("is-active", r === a)); };
    rows.forEach((r) => {
      if (FINE) { r.addEventListener("pointerenter", () => { hovered = r; apply(); }); }
      r.addEventListener("focus", () => { hovered = r; apply(); });
      r.addEventListener("blur", () => { if (hovered === r) hovered = null; apply(); });
    });
    if (FINE) $("#index").addEventListener("pointerleave", () => { hovered = null; apply(); });
    const io = new IntersectionObserver((es) => {
      for (const e of es) if (e.isIntersecting) scrolled = e.target;
      apply();
    }, { rootMargin: "-45% 0px -45% 0px" });
    rows.forEach((r) => io.observe(r));
    apply();
  }

  // ---------------------------------------------------------------------------
  // MENU BAR: real panels, fitted to their size, rising at two depths
  // ---------------------------------------------------------------------------
  {
    const frames = $$(".mb-frame iframe");
    addEventListener("message", (e) => {
      if (!e.data || e.data.type !== "brink-panel-size") return;
      const f = frames.find((x) => x.contentWindow === e.source);
      if (!f) return;
      const h = Math.round(+e.data.height), w = Math.round(+e.data.width);
      const cap = f.closest(".f-settings") ? 760 : 2000;
      if (h > 100 && h < 2400) f.style.height = `${Math.min(h, cap)}px`;
      if (w > 200 && w < 480) f.style.width = `${w}px`;
      if (MOTION) ScrollTrigger.refresh();
    });
    const clock = $("#mb-clock");
    const tickClock = () => { const d = new Date(); clock.textContent = `${d.getHours() % 12 || 12}:${String(d.getMinutes()).padStart(2, "0")}`; };
    tickClock(); setInterval(tickClock, 30000);

    if (MOTION && !mobile()) {
      const st = { trigger: ".menubar", start: "top bottom", end: "bottom top", scrub: 0.6 };
      gsap.fromTo(".f-main", { y: 180 }, { y: -90, ease: "none", scrollTrigger: st });
      gsap.fromTo(".f-settings", { y: 300, scale: 0.94, rotate: -2 }, { y: 30, scale: 0.94, rotate: -1, ease: "none", scrollTrigger: { ...st } });
      gsap.fromTo(".mb-atmos .a1", { y: -80 }, { y: 120, ease: "none", scrollTrigger: { ...st, scrub: true } });
      gsap.fromTo(".mb-atmos .a2", { y: 60 }, { y: -100, ease: "none", scrollTrigger: { ...st, scrub: true } });
    }
  }

  // ---------------------------------------------------------------------------
  // FINALE: "Free." rises letter by letter with scroll
  // ---------------------------------------------------------------------------
  if (MOTION) {
    gsap.fromTo(".fl > span", { yPercent: 104 }, {
      yPercent: 0, ease: "none", stagger: 0.12,
      scrollTrigger: { trigger: ".free", start: "top 92%", end: "top 40%", scrub: 0.6 },
    });
    gsap.fromTo("#wordmark", { yPercent: 38 }, { yPercent: 0, ease: "none", scrollTrigger: { trigger: ".foot", start: "top bottom", end: "bottom bottom", scrub: 0.6 } });
  }

  // ---------------------------------------------------------------------------
  // Magnetic CTAs (spring-interpolated, uses the `translate` property so
  // :active can still `scale` it)
  // ---------------------------------------------------------------------------
  if (FINE && !RM) {
    $$("[data-magnetic]").forEach((el) => {
      let x = 0, y = 0, tx = 0, ty = 0, raf = 0;
      const label = el.querySelector(".btn-label, span");
      const strength = el.classList.contains("btn-xl") ? 0.38 : 0.28;
      const loop = () => {
        x += (tx - x) * 0.16; y += (ty - y) * 0.16;
        el.style.translate = `${x.toFixed(2)}px ${y.toFixed(2)}px`;
        if (label) label.style.translate = `${(x * 0.35).toFixed(2)}px ${(y * 0.35).toFixed(2)}px`;
        if (Math.abs(tx - x) > 0.05 || Math.abs(ty - y) > 0.05) raf = requestAnimationFrame(loop); else raf = 0;
      };
      const go = () => { if (!raf) raf = requestAnimationFrame(loop); };
      el.addEventListener("pointermove", (e) => {
        const r = el.getBoundingClientRect();
        tx = (e.clientX - (r.left - x + r.width / 2)) * strength;
        ty = (e.clientY - (r.top - y + r.height / 2)) * strength * 1.2;
        go();
      });
      el.addEventListener("pointerleave", () => { tx = 0; ty = 0; go(); });
    });
  }

  // ---------------------------------------------------------------------------
  // FAQ accordion
  // ---------------------------------------------------------------------------
  $$(".q").forEach((q) => {
    const b = q.querySelector(".q-btn");
    b.addEventListener("click", () => {
      const open = !q.classList.contains("open");
      q.classList.toggle("open", open);
      b.setAttribute("aria-expanded", String(open));
      if (MOTION) { clearTimeout(q.t); q.t = setTimeout(() => ScrollTrigger.refresh(), 650); }
    });
  });

  // ---------------------------------------------------------------------------
  // MORE FROM ZEROWING: screenshot follows the cursor with an eased tilt
  // ---------------------------------------------------------------------------
  if (FINE) {
    const float = $("#proj-float"), imgs = $$("#proj-float img"), list = $("#projects");
    let mx = 0, my = 0, x = 0, y = 0, rot = 0, raf = 0, shown = false, placed = false;
    const loop = () => {
      const k = RM ? 1 : 0.13;
      const px = x;
      x += (mx - x) * k; y += (my - y) * k;
      const v = x - px;
      rot += (clamp(v * 0.5, -9, 9) - rot) * 0.12;
      const w = float.offsetWidth, h = float.offsetHeight;
      float.style.transform = `translate3d(${(x - w / 2).toFixed(1)}px,${(y - h / 2).toFixed(1)}px,0) rotate(${RM ? 0 : rot.toFixed(2)}deg)`;
      const moving = Math.abs(mx - x) > 0.3 || Math.abs(my - y) > 0.3 || Math.abs(rot) > 0.05;
      raf = moving || shown ? requestAnimationFrame(loop) : 0;
      if (!moving && shown) { cancelAnimationFrame(raf); raf = 0; }
    };
    const go = () => { if (!raf) raf = requestAnimationFrame(loop); };
    list.addEventListener("pointermove", (e) => {
      mx = e.clientX; my = e.clientY;
      if (!placed) { x = mx; y = my; placed = true; float.classList.add("ready"); }
      go();
    });
    $$(".proj", list).forEach((a) => {
      a.addEventListener("pointerenter", () => {
        const i = +a.dataset.img;
        imgs.forEach((im, j) => im.classList.toggle("on", j === i));
        shown = true; float.classList.add("show"); go();
      });
    });
    list.addEventListener("pointerleave", () => { shown = false; float.classList.remove("show"); go(); });
    addEventListener("scroll", () => { if (shown) go(); }, { passive: true });
  }

  // ---------------------------------------------------------------------------
  // Scroll reveals (one-shot, IntersectionObserver)
  // ---------------------------------------------------------------------------
  {
    // sibling stagger
    [".log-list", ".faq-list", ".foot-grid", ".mb-copy", ".free-side"].forEach((s) => {
      const p = $(s); if (!p) return;
      $$("[data-reveal]", p).forEach((el, i) => el.style.setProperty("--d", `${Math.min(i, 6) * 80}ms`));
    });
    const targets = [...$$("[data-reveal]"), ...$$("[data-split]:not([data-hero-title])"), ...$$("#index .row")];
    if ("IntersectionObserver" in window && !RM) {
      const io = new IntersectionObserver((es) => {
        for (const e of es) if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
      }, { threshold: 0.15, rootMargin: "0px 0px -6% 0px" });
      targets.forEach((t) => io.observe(t));
    } else targets.forEach((t) => t.classList.add("in"));
  }

  // fonts can shift layout; settle triggers once they land
  if (MOTION && document.fonts) document.fonts.ready.then(() => ScrollTrigger.refresh());
})();
