// Website-only hooks for the Brink panel embedded on zerowing.in/brink (not part of brink/ui).
// Loaded after app.js by one extra <script> line in this copy's index.html, so a fresh copy of
// brink/ui only needs that line re-added.
//   ?embed=1        transparent page and panel, so the page's liquid glass shows through
//   ?view=settings  open the settings view on boot
//   ?theme=dark     start in Dark appearance (mock backend only)
// In an iframe the panel also reports its size so the page can fit the frame to it.
(() => {
  const q = new URLSearchParams(location.search);
  if (q.has("embed")) {
    document.documentElement.classList.add("embed");
    const s = document.createElement("style");
    s.textContent =
      "html.embed,html.embed body{background:transparent!important}";
    document.head.appendChild(s);
  }
  const panel = () => document.getElementById("panel") || document.body;
  const booted = () => typeof state !== "undefined" && state; // app.js's top-level `state`
  const whenBooted = (fn, n = 0) => (booted() || n > 100 ? fn() : setTimeout(() => whenBooted(fn, n + 1), 30));
  whenBooted(() => {
    if (q.get("theme") === "dark" && typeof patch === "function") patch({ appearance: "dark" });
    const settingsView = document.getElementById("view-settings");
    if (q.get("view") === "settings" && settingsView?.hidden) document.querySelector('[data-action="settings"]')?.click();
    if (window.parent !== window) {
      const report = () => {
        const p = panel();
        window.parent.postMessage({ type: "brink-panel-size", width: p.offsetWidth, height: p.offsetHeight }, "*");
      };
      new ResizeObserver(report).observe(panel());
      report();
    }
  });
})();
