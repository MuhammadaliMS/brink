// Shows where a window will land while two fingers swipe on it. Brink sends the spot (in
// points from this screen's top-left) and a label, or null to hide.
const shape = document.getElementById("shape");
const label = document.getElementById("label");
let visible = false;

function render(v) {
  if (!v) {
    shape.classList.remove("on");
    visible = false;
    return;
  }
  // Appearing: jump to the spot first, so it fades in place instead of flying in.
  if (!visible) shape.style.transition = "opacity 0.14s ease-out, transform 0.2s cubic-bezier(0.25, 1.1, 0.4, 1)";
  const inset = 6;
  shape.style.left = `${v.x + inset}px`;
  shape.style.top = `${v.y + inset}px`;
  shape.style.width = `${Math.max(0, v.w - inset * 2)}px`;
  shape.style.height = `${Math.max(0, v.h - inset * 2)}px`;
  shape.classList.toggle("minimize", v.kind === "minimize");
  label.textContent = v.label;
  if (!visible) {
    shape.getBoundingClientRect();
    shape.style.transition = "";
  }
  shape.classList.add("on");
  visible = true;
}

window.__TAURI__?.event.listen("brink://preview", (e) => render(e.payload));

// Browser preview: ?demo cycles through a few spots.
if (location.search.includes("demo")) {
  const W = innerWidth, H = innerHeight;
  const spots = [
    { x: 0, y: 0, w: W / 2, h: H, label: "Left half", kind: "place" },
    { x: 0, y: 0, w: W, h: H, label: "Fill screen", kind: "place" },
    { x: W / 2, y: H / 2, w: W / 2, h: H / 2, label: "Bottom right", kind: "place" },
    { x: W * 0.2, y: H * 0.2, w: W * 0.6, h: H * 0.6, label: "Minimize", kind: "minimize" },
  ];
  const i = Number(new URLSearchParams(location.search).get("demo")) || 0;
  render(spots[i % spots.length]);
}
