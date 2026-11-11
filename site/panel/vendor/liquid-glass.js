var Te = Object.defineProperty;
var Re = (s, e, t) => e in s ? Te(s, e, { enumerable: !0, configurable: !0, writable: !0, value: t }) : s[e] = t;
var p = (s, e, t) => Re(s, typeof e != "symbol" ? e + "" : e, t);
const Fe = `precision highp float;
// The backdrop the glass bends: an image or video cropped to cover, or the built-in aurora.
uniform vec2 u_size;      // CSS pixels
uniform float u_dpr;
uniform float u_mode;     // 0: image or video, 1: aurora, 2: plain base colour (while an image loads)
uniform sampler2D u_image;
uniform vec4 u_crop;      // scale x, scale y, offset x, offset y (cover crop)
uniform float u_time;
uniform vec3 u_colors[4];
uniform vec3 u_base;
uniform float u_dim;       // 0..1, darkens the backdrop like a scrim

float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}

float fbm(vec2 p) {
  float value = 0.0;
  float amplitude = 0.5;
  for (int i = 0; i < 4; i++) { value += amplitude * noise(p); p = p * 2.03 + 11.7; amplitude *= 0.5; }
  return value;
}

// Four slow colour fields, folded by a noise flow, with fine contour lines so the glass
// always has edges to bend.
vec3 aurora(vec2 pixel) {
  vec2 q = pixel / max(u_size.x, u_size.y) * 2.4;
  float t = u_time * 0.045;
  vec2 warp = vec2(fbm(q + vec2(t, -t * 0.7)), fbm(q + vec2(-t * 0.6, t) + 5.2));
  vec2 p = q + warp * 1.35;
  vec3 color = u_base;
  for (int i = 0; i < 4; i++) {
    float fi = float(i);
    vec2 center = vec2(0.6 + 1.1 * sin(t * (1.0 + fi * 0.37) + fi * 1.9), 0.55 + 0.75 * cos(t * (0.8 + fi * 0.29) + fi * 2.6));
    center *= vec2(max(u_size.x / u_size.y, 1.0), max(u_size.y / u_size.x, 1.0)) * 1.2;
    float reach = exp(-dot(p - center, p - center) * 1.15);
    color = mix(color, u_colors[i], clamp(reach * 1.25, 0.0, 1.0));
  }
  float height = fbm(p * 1.6 + t);
  float lines = abs(fract(height * 14.0) - 0.5);
  float line = 1.0 - smoothstep(0.0, 0.07, lines);
  color = mix(color, color * 1.35 + 0.06, line * 0.35);
  color += (hash(pixel + fract(u_time)) - 0.5) * 0.025;
  return color;
}

void main() {
  vec2 pixel = vec2(gl_FragCoord.x, u_size.y * u_dpr - gl_FragCoord.y) / u_dpr;
  vec2 uv = pixel / u_size;
  vec3 color;
  if (u_mode < 0.5) color = texture2D(u_image, vec2(u_crop.z + uv.x * u_crop.x, 1.0 - (u_crop.w + uv.y * u_crop.y))).rgb;
  else if (u_mode < 1.5) color = aurora(pixel);
  else color = u_base;
  gl_FragColor = vec4(color * (1.0 - u_dim), 1.0);
}
`, Ce = `precision highp float;
uniform sampler2D u_background;
uniform vec2 u_size;
uniform vec2 u_step;

vec3 sampleLinear(vec2 uv) {
  return pow(texture2D(u_background, clamp(uv, 0.001, 0.999)).rgb, vec3(2.2));
}

void main() {
  vec2 uv = gl_FragCoord.xy / u_size;
  vec3 color = vec3(0.0);
  float total = 0.0;
  // The cached wider Gaussian prevents tiny granules from becoming ridges
  // when the bevel compresses the transmitted image's sampling footprint.
  for (int y = -2; y <= 2; y++) {
    for (int x = -2; x <= 2; x++) {
      vec2 offset = vec2(float(x), float(y));
      float weight = exp(-dot(offset, offset) * 0.5);
      color += sampleLinear(uv + offset * u_step) * weight;
      total += weight;
    }
  }
  color /= total;
  gl_FragColor = vec4(pow(color, vec3(1.0 / 2.2)), 1.0);
}
`, Pe = `precision highp float;
// Every glass panel on the scene as a real solid of glass. Each panel is a rounded slab with
// a domed crown and rounded edges, ray-traced from a camera straight in front of it: light
// refracts into the slab, travels through it (bouncing inside when it can't get out), refracts
// out and lands on the backdrop behind. That's what makes the backdrop magnify, flip and
// smear at the rim. The same model as the liquid-glass playground's pebble.
// MAX_PANES is defined by the renderer from what the device's GPU allows.
uniform sampler2D u_scene;
uniform sampler2D u_frost;
uniform vec2 u_size;      // CSS pixels
uniform float u_dpr;
uniform int u_count;
uniform vec4 u_rect[MAX_PANES];    // centre x, centre y, half width, half height (CSS px)
uniform vec4 u_look[MAX_PANES];    // corner radius, frost 0..1, chroma, pointer pull (px)
uniform vec4 u_tint[MAX_PANES];    // rgb, strength
uniform vec4 u_profile[MAX_PANES]; // half depth, edge rounding, dome, index of refraction
uniform vec4 u_lens[MAX_PANES];    // a raised disc on the face: x, y (from centre), radius, amount
uniform vec4 u_extra[MAX_PANES];   // sound glow 0..1, shadow 0..1, press highlight 0..1, unused
uniform vec2 u_light;              // light position, CSS px
uniform vec2 u_pointer;            // pointer position, CSS px
uniform float u_bands[6];          // sound, low to high
uniform float u_level;
uniform mat3 u_rotation;           // the glass's tilt toward the camera, and its inverse
uniform mat3 u_inverse;
uniform float u_camera;            // camera distance, CSS px (the playground's 780)
uniform float u_margin;            // how far a tilted slab can reach past its outline, px
uniform float u_sheen;             // a studio rim light on the glass edges, 0..1
uniform sampler2D u_shape;         // glass text: a signed distance field over the scene (0.5 = edge)
uniform float u_shapeRange;        // the field's reach either side of the edge, px
uniform float u_shapeStep;         // one field texel in scene px (the gradient's finite difference)

const float BACKGROUND = -190.0;

// The selected panel, copied out of the arrays (GLSL ES 1.0 can't index them by a variable).
vec4 R; vec4 L; vec4 T; vec4 P; vec4 N; vec4 X;
vec2 pointerLocal;

vec3 sceneColor(vec2 pixel, float frost) {
  vec2 uv = clamp(vec2(pixel.x / u_size.x, 1.0 - pixel.y / u_size.y), 0.001, 0.999);
  vec3 clear = pow(texture2D(u_scene, uv).rgb, vec3(2.2));
  vec3 blurred = pow(texture2D(u_frost, uv).rgb, vec3(2.2));
  return mix(clear, blurred, clamp(frost, 0.0, 1.0));
}

float box(vec2 p, vec2 halfSize, float radius) {
  vec2 q = abs(p) - halfSize + radius;
  return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - radius;
}

// A glass-text pane's outline: the shape field, kept inside the pane's own box so a pane never
// claims another word's letters.
float shapeField(vec2 scenePixel) {
  vec2 uv = clamp(scenePixel / u_size, 0.0, 1.0);
  return (texture2D(u_shape, uv).r - 0.5) * 2.0 * u_shapeRange;
}
float paneShape(vec2 local, vec4 rect) {
  return max(shapeField(rect.xy + local), box(local, rect.zw + 6.0, 0.0));
}

// The panel's outline: its rounded rectangle (or its letters), bulging a little toward a nearby pointer.
float outline(vec2 p) {
  float radius = min(L.x, min(R.z, R.w));
  float d = X.w > 0.5 ? paneShape(p, R) : box(p, R.zw, radius);
  vec2 toPointer = p - pointerLocal;
  return d - L.w * exp(-dot(toPointer, toPointer) / 1800.0);
}

vec3 field(vec2 p) {
  float d = outline(p);
  vec2 e = vec2(X.w > 0.5 ? u_shapeStep * 1.5 : 0.5, 0.0);
  vec2 n = vec2(outline(p + e.xy) - outline(p - e.xy), outline(p + e.yx) - outline(p - e.yx));
  return vec3(d, normalize(n + 1e-6));
}

// Rounded extrusion with a smooth convex crown; the gradient belongs to the same solid.
vec4 solid(vec3 p) {
  vec3 boundary = field(p.xy);
  vec2 radius = max(R.zw, vec2(20.0));
  float dome = P.z * exp(-1.5 * dot(p.xy / radius, p.xy / radius));
  vec2 domeGradient = -3.0 * dome * p.xy / (radius * radius);
  float rounding = P.y;
  vec2 q = vec2(boundary.x + rounding, abs(p.z) - P.x - dome + rounding);
  vec2 outside = max(q, 0.0);
  float outsideLength = length(outside);
  vec2 weights = outsideLength > 0.0001 ? outside / outsideLength : (q.x > q.y ? vec2(1.0, 0.0) : vec2(0.0, 1.0));
  float distance = min(max(q.x, q.y), 0.0) + outsideLength - rounding;
  vec3 gradient = weights.x * vec3(boundary.yz, 0.0) + weights.y * vec3(-domeGradient, sign(p.z));
  if (N.w > 0.02) {
    vec2 delta = p.xy - N.xy;
    vec2 lensQ = vec2(length(delta) - N.z + 8.0, abs(p.z - P.x - 5.0) - 2.0);
    vec2 lensOutside = max(lensQ, 0.0);
    float lensLength = length(lensOutside);
    vec2 lensWeights = lensLength > 0.0001 ? lensOutside / lensLength : (lensQ.x > lensQ.y ? vec2(1.0, 0.0) : vec2(0.0, 1.0));
    float lensDistance = min(max(lensQ.x, lensQ.y), 0.0) + lensLength - 8.0;
    vec3 lensGradient = vec3(delta / max(length(delta), 0.001) * lensWeights.x, sign(p.z - P.x - 5.0) * lensWeights.y);
    float blend = clamp(0.5 + 0.5 * (distance - lensDistance) / 2.5, 0.0, 1.0) * N.w;
    distance = mix(distance, lensDistance, blend) - 2.5 * blend * (1.0 - blend);
    gradient = mix(gradient, lensGradient, blend);
  }
  return vec4(distance, gradient);
}

bool enterSolid(vec3 origin, vec3 direction, out vec3 hit) {
  vec3 bounds = vec3(R.zw + L.w + 6.0, P.x + P.z + 10.0);
  vec3 inverseRay = 1.0 / (direction + vec3(0.000001));
  vec3 a = (-bounds - origin) * inverseRay;
  vec3 b = (bounds - origin) * inverseRay;
  vec3 nearBox = min(a, b);
  vec3 farBox = max(a, b);
  float nearDistance = max(max(nearBox.x, nearBox.y), nearBox.z);
  float farDistance = min(min(farBox.x, farBox.y), farBox.z);
  float travel = max(0.0, nearDistance);
  if (farDistance < travel) return false;
  for (int i = 0; i < 56; i++) {
    hit = origin + direction * travel;
    float distance = solid(hit).x;
    if (distance < 0.12) return true;
    travel += max(0.06, distance * 0.74);
    if (travel > farDistance) break;
  }
  return false;
}

vec3 exitSolid(vec3 origin, vec3 direction, out float traveled) {
  float travel = 0.6;
  vec3 point = origin + direction * travel;
  float limit = 2.0 * (R.z + R.w) + 2.0 * P.x;
  for (int i = 0; i < 48; i++) {
    point = origin + direction * travel;
    float distance = -solid(point).x;
    if (i > 1 && distance < 0.12) break;
    travel += max(0.10, distance * 0.74);
    if (travel > limit) break;
  }
  traveled = travel;
  return point;
}

vec3 environment(vec3 direction, vec2 pixel) {
  vec2 reflectedPixel = u_size * 0.5 + direction.xy * u_size * 0.42;
  vec3 reflectedScene = sceneColor(reflectedPixel, 0.9);
  float softbox = exp(-pow((direction.x + 0.42) / 0.16, 2.0) - pow((direction.y + 0.58) / 0.28, 4.0));
  float fill = exp(-pow((direction.x - 0.7) / 0.10, 2.0) - pow(direction.y / 0.55, 4.0));
  return mix(reflectedScene, sceneColor(pixel, 0.65), 0.20) + vec3(0.96, 0.98, 1.0) * (softbox * 1.65 + fill * 0.32);
}

float energy(vec2 p, vec2 origin, vec2 spread) {
  vec2 d = (p - origin) / spread;
  return exp(-dot(d, d));
}

void main() {
  vec2 pixel = vec2(gl_FragCoord.x, u_size.y * u_dpr - gl_FragCoord.y) / u_dpr;
  vec3 base = sceneColor(pixel, 0.0);

  // Later panels sit on top (a knob on its track); outside every panel, the nearest one wins.
  float nearest = 1e5;
  float shade = 0.0;
  for (int i = 0; i < MAX_PANES; i++) {
    if (i >= u_count) break;
    vec4 r = u_rect[i];
    float radius = min(u_look[i].x, min(r.z, r.w));
    vec2 local = pixel - r.xy;
    bool shaped = u_extra[i].w > 0.5;
    float d = shaped ? paneShape(local, r) : box(local, r.zw, radius) - u_look[i].w;
    float lift = min(r.w, 40.0);
    float s = shaped ? paneShape(local - vec2(2.0, lift * 0.22), r) : box(local - vec2(2.0, lift * 0.22), r.zw, radius);
    shade = max(shade, (1.0 - smoothstep(-lift * 0.3, lift * 0.8, s)) * u_extra[i].y);
    // inside an outline: the topmost panel there owns the pixel. Outside every outline: the nearest
    // panel within reach (a tilted slab's side can show past its outline).
    bool take = d < 2.0 || (nearest >= 2.0 && d < nearest && d < u_margin);
    if (take) { nearest = d; R = r; L = u_look[i]; T = u_tint[i]; P = u_profile[i]; N = u_lens[i]; X = u_extra[i]; }
  }
  vec3 outside = base * (1.0 - shade * 0.16);
  if (nearest > u_margin || u_count == 0) { gl_FragColor = vec4(pow(outside, vec3(1.0 / 2.2)), 1.0); return; }

  vec2 local = pixel - R.xy;
  pointerLocal = u_pointer - R.xy;
  vec3 origin = u_inverse * vec3(0.0, 0.0, u_camera);
  vec3 viewRay = normalize(vec3(local, -u_camera));
  vec3 direction = u_inverse * viewRay;
  vec3 front;
  if (!enterSolid(origin, direction, front)) {
    float contact = (1.0 - smoothstep(-2.0, 13.0, outline(local - vec2(2.0, 7.0)))) * 0.045;
    gl_FragColor = vec4(pow(outside * (1.0 - contact), vec3(1.0 / 2.2)), 1.0);
    return;
  }
  vec3 frontNormal = normalize(solid(front).yzw);
  float ior = P.w;
  vec3 inside = refract(direction, frontNormal, 1.0 / ior);
  float pathLength;
  vec3 rear = exitSolid(front, inside, pathLength);
  vec3 rearNormal = normalize(solid(rear).yzw);
  vec3 outgoing = refract(inside, -rearNormal, ior);
  // Follow one internal bounce instead of a dark band where the first exit totally reflects.
  if (dot(outgoing, outgoing) < 0.01) {
    inside = reflect(inside, rearNormal);
    float secondLength;
    rear = exitSolid(rear, inside, secondLength);
    pathLength += secondLength;
    rearNormal = normalize(solid(rear).yzw);
    outgoing = refract(inside, -rearNormal, ior);
    if (dot(outgoing, outgoing) < 0.01) outgoing = reflect(inside, rearNormal);
  }
  // back to the camera's frame: the slab may be tilted
  vec3 normal = normalize(u_rotation * frontNormal);
  vec3 worldRear = u_rotation * rear;
  vec3 worldOutgoing = normalize(u_rotation * outgoing);
  vec3 worldFront = u_rotation * front;
  vec3 reflected = environment(reflect(viewRay, normal), pixel);
  vec3 transmitted;
  float frost = L.y;
  if (worldOutgoing.z < -0.045) {
    float distance = (BACKGROUND - worldRear.z) / worldOutgoing.z;
    vec2 backgroundPoint = (worldRear.xy + worldOutgoing.xy * distance) * u_camera / (u_camera - BACKGROUND);
    vec2 rayPixel = R.xy + backgroundPoint;
    vec2 displacement = clamp(rayPixel - pixel, vec2(-180.0), vec2(180.0));
    float grazing = 1.0 - max(dot(normal, -viewRay), 0.0);
    float filtering = clamp(grazing * grazing * 1.6 + length(displacement) / 300.0 + frost, 0.02, 0.92);
    float chroma = L.z;
    vec3 red = sceneColor(pixel + displacement * (1.0 - chroma), filtering);
    vec3 green = sceneColor(pixel + displacement, filtering);
    vec3 blue = sceneColor(pixel + displacement * (1.0 + chroma), filtering);
    transmitted = vec3(red.r, green.g, blue.b);
  } else transmitted = environment(worldOutgoing, pixel);
  transmitted *= exp(-vec3(0.00032, 0.00015, 0.00007) * min(pathLength, 250.0));
  transmitted = mix(transmitted, pow(T.rgb, vec3(2.2)), T.a);

  float f0 = pow((ior - 1.0) / (ior + 1.0), 2.0);
  float fresnel = f0 + (1.0 - f0) * pow(1.0 - max(dot(normal, -viewRay), 0.0), 5.0);
  vec3 color = mix(transmitted, reflected, fresnel);
  vec3 toLight = normalize(vec3(u_light - R.xy - worldFront.xy, 500.0));
  vec3 halfVector = normalize(toLight - viewRay);
  color += vec3(0.97, 0.985, 1.0) * pow(max(dot(normal, halfVector), 0.0), 160.0) * (0.04 + X.z * 0.10);
  // studio lighting for dark scenes: a soft rim where the slab turns away, and a broad top softbox
  float facing = max(dot(normal, -viewRay), 0.0);
  color += vec3(1.0, 0.98, 0.95) * u_sheen * (pow(1.0 - facing, 2.5) * 0.55 + pow(max(dot(normal, normalize(vec3(-0.25, -0.75, 0.6))), 0.0), 24.0) * 0.35);
  color *= 1.0 + X.z * 0.06;

  // Sound lights the glass from inside in six colours, low notes left to high notes right.
  if (X.x > 0.001) {
    vec2 q = front.xy / max(R.zw, vec2(1.0));
    vec3 glow = vec3(1.0, 0.45, 0.10) * u_bands[0] * energy(q, vec2(-0.39, 0.53), vec2(0.26, 0.53));
    glow += vec3(1.0, 0.19, 0.08) * u_bands[1] * energy(q, vec2(-0.20, 0.64), vec2(0.24, 0.48));
    glow += vec3(1.0, 0.025, 0.30) * u_bands[2] * energy(q, vec2(0.03, 0.61), vec2(0.28, 0.52));
    glow += vec3(0.47, 0.04, 1.0) * u_bands[3] * energy(q, vec2(0.28, 0.55), vec2(0.26, 0.53));
    glow += vec3(0.08, 0.23, 1.0) * u_bands[4] * energy(q, vec2(0.52, 0.38), vec2(0.23, 0.52));
    glow += vec3(0.035, 0.75, 1.0) * u_bands[5] * energy(q, vec2(-0.64, 0.17), vec2(0.17, 0.53));
    color += glow * u_level * 0.75 * X.x;
  }
  if (N.w > 0.5 && length(front.xy - N.xy) < N.z - 2.0) color = mix(color, sceneColor(pixel, 0.85), 0.15);

  // flat: soften the outline by its distance; tilted: the ray hit decides the silhouette
  float coverage = u_margin > 2.5 ? 1.0 : clamp(0.5 - nearest * u_dpr, 0.0, 1.0);
  color = mix(outside, color, max(coverage, step(nearest, -1.0)));
  gl_FragColor = vec4(pow(max(color, vec3(0.0)), vec3(1.0 / 2.2)), 1.0);
}
`;
function De(s, e, t, n) {
  if (![s, e, t, n].every((l) => Number.isFinite(l) && l > 0))
    throw new RangeError("Backdrop dimensions must be finite and positive.");
  const i = s / e, r = t / n, o = Math.min(1, i / r), m = Math.min(1, r / i);
  return [o, m, (1 - o) / 2, (1 - m) / 2];
}
function Ne(s, e = () => new Image()) {
  return new Promise((t, n) => {
    const i = e();
    i.decoding = "async", i.crossOrigin = "anonymous", i.onload = () => {
      if (i.onload = null, i.onerror = null, i.naturalWidth <= 0 || i.naturalHeight <= 0) {
        n(new Error("The backdrop image has invalid image dimensions."));
        return;
      }
      t({ source: i, width: i.naturalWidth, height: i.naturalHeight });
    }, i.onerror = () => {
      i.onload = null, i.onerror = null, n(new Error("The backdrop image could not load; the procedural scene remains available."));
    }, i.src = s;
  });
}
const me = 16, ke = "attribute vec2 a_position; void main() { gl_Position = vec4(a_position, 0.0, 1.0); }";
function Le(s) {
  const e = s.getContext("webgl", { alpha: !1, antialias: !1, premultipliedAlpha: !1, powerPreference: "low-power" });
  if (!e) throw new Error("WebGL is unavailable.");
  const t = [], n = [];
  function i(f, v) {
    const M = e.createShader(f);
    if (!M) throw new Error("Unable to allocate a glass shader.");
    if (e.shaderSource(M, v), e.compileShader(M), !e.getShaderParameter(M, e.COMPILE_STATUS)) {
      const I = e.getShaderInfoLog(M);
      throw e.deleteShader(M), new Error(`Glass shader failed: ${I}`);
    }
    return t.push(M), M;
  }
  function r(f) {
    const v = e.createProgram();
    if (!v) throw new Error("Unable to allocate the glass renderer.");
    if (e.attachShader(v, i(e.VERTEX_SHADER, ke)), e.attachShader(v, i(e.FRAGMENT_SHADER, f)), e.linkProgram(v), !e.getProgramParameter(v, e.LINK_STATUS)) throw new Error(`Glass renderer failed to link: ${e.getProgramInfoLog(v)}`);
    return n.push(v), v;
  }
  function o() {
    const f = e.createTexture();
    if (!f) throw new Error("Unable to allocate a glass texture.");
    return e.bindTexture(e.TEXTURE_2D, f), e.texParameteri(e.TEXTURE_2D, e.TEXTURE_MIN_FILTER, e.LINEAR), e.texParameteri(e.TEXTURE_2D, e.TEXTURE_MAG_FILTER, e.LINEAR), e.texParameteri(e.TEXTURE_2D, e.TEXTURE_WRAP_S, e.CLAMP_TO_EDGE), e.texParameteri(e.TEXTURE_2D, e.TEXTURE_WRAP_T, e.CLAMP_TO_EDGE), e.texImage2D(e.TEXTURE_2D, 0, e.RGBA, 1, 1, 0, e.RGBA, e.UNSIGNED_BYTE, new Uint8Array([20, 24, 40, 255])), f;
  }
  const m = r(Fe), l = r(Ce), h = Number(e.getParameter(e.MAX_FRAGMENT_UNIFORM_VECTORS)) || 64, d = Math.max(2, Math.min(me, Math.floor((h - 16) / 6))), a = r(`#define MAX_PANES ${d}
${Pe}`), g = e.createBuffer(), E = o(), c = o(), x = o(), F = o();
  e.texImage2D(e.TEXTURE_2D, 0, e.LUMINANCE, 1, 1, 0, e.LUMINANCE, e.UNSIGNED_BYTE, new Uint8Array([255]));
  const A = e.createFramebuffer(), C = e.createFramebuffer();
  if (!g || !A || !C) throw new Error("Unable to allocate glass buffers.");
  e.bindBuffer(e.ARRAY_BUFFER, g), e.bufferData(e.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), e.STATIC_DRAW);
  const T = (f, v) => Object.fromEntries(v.map((M) => [M, e.getUniformLocation(f, M)])), w = T(m, ["u_size", "u_dpr", "u_mode", "u_image", "u_crop", "u_time", "u_colors[0]", "u_base", "u_dim"]), k = T(l, ["u_background", "u_size", "u_step"]), y = T(a, ["u_scene", "u_frost", "u_size", "u_dpr", "u_count", "u_rect[0]", "u_look[0]", "u_tint[0]", "u_profile[0]", "u_lens[0]", "u_extra[0]", "u_light", "u_pointer", "u_bands[0]", "u_level", "u_rotation", "u_inverse", "u_camera", "u_margin", "u_sheen", "u_shape", "u_shapeRange", "u_shapeStep"]), U = new Float32Array(d * 4), b = new Float32Array(d * 4), _ = new Float32Array(d * 4), u = new Float32Array(d * 4), R = new Float32Array(d * 4), O = new Float32Array(d * 4), W = new Float32Array(6);
  let L = 1, Y = 1, B = 1, $ = !1, G = !1, N = 1, oe = 0, Q = { width: 1, height: 1 }, se = new Float32Array([1, 0.35, 0.12, 1, 0.18, 0.54, 0.42, 0.23, 1, 0, 0.76, 1]), ae = new Float32Array([0.04, 0.03, 0.12]), le = 40, ce = 1;
  function j(f) {
    e.useProgram(f), e.bindBuffer(e.ARRAY_BUFFER, g);
    const v = e.getAttribLocation(f, "a_position");
    e.enableVertexAttribArray(v), e.vertexAttribPointer(v, 2, e.FLOAT, !1, 0, 0);
  }
  function ue(f, v, M, I) {
    if (e.bindTexture(e.TEXTURE_2D, v), e.texImage2D(e.TEXTURE_2D, 0, e.RGBA, M, I, 0, e.RGBA, e.UNSIGNED_BYTE, null), e.bindFramebuffer(e.FRAMEBUFFER, f), e.framebufferTexture2D(e.FRAMEBUFFER, e.COLOR_ATTACHMENT0, e.TEXTURE_2D, v, 0), e.checkFramebufferStatus(e.FRAMEBUFFER) !== e.FRAMEBUFFER_COMPLETE) throw new Error("A glass framebuffer is incomplete.");
  }
  function xe(f, v) {
    L = Math.max(1, f), Y = Math.max(1, v), B = Math.min(window.devicePixelRatio || 1, 1.5), s.width = Math.round(L * B), s.height = Math.round(Y * B), ue(A, c, s.width, s.height), ue(C, x, Math.max(1, Math.ceil(s.width / 4)), Math.max(1, Math.ceil(s.height / 4))), e.bindFramebuffer(e.FRAMEBUFFER, null), $ = !0;
  }
  function ye(f, v = 1, M = 1) {
    N = f === "wait" ? 2 : f ? 0 : 1, !(!f || f === "wait") && (Q = { width: Math.max(1, v), height: Math.max(1, M) }, e.bindTexture(e.TEXTURE_2D, E), e.pixelStorei(e.UNPACK_FLIP_Y_WEBGL, !0), e.texImage2D(e.TEXTURE_2D, 0, e.RGBA, e.RGBA, e.UNSIGNED_BYTE, f), e.pixelStorei(e.UNPACK_FLIP_Y_WEBGL, !1));
  }
  function be(f, v, M, I) {
    e.bindTexture(e.TEXTURE_2D, F), e.pixelStorei(e.UNPACK_ALIGNMENT, 1), f ? e.texImage2D(e.TEXTURE_2D, 0, e.LUMINANCE, v, M, 0, e.LUMINANCE, e.UNSIGNED_BYTE, f) : e.texImage2D(e.TEXTURE_2D, 0, e.LUMINANCE, 1, 1, 0, e.LUMINANCE, e.UNSIGNED_BYTE, new Uint8Array([255])), e.pixelStorei(e.UNPACK_ALIGNMENT, 4), le = I, ce = f ? Math.max(0.5, L / Math.max(1, v)) : 1;
  }
  function we(f, v) {
    se = new Float32Array(Array.from({ length: 4 }, (M, I) => f[I % f.length]).flat()), ae = new Float32Array(v);
  }
  function _e(f) {
    oe = Math.min(0.9, Math.max(0, Number.isFinite(f) ? f : 0));
  }
  function Ee(f) {
    if (G || !$) return;
    e.bindFramebuffer(e.FRAMEBUFFER, A), e.viewport(0, 0, s.width, s.height), j(m), e.activeTexture(e.TEXTURE0), e.bindTexture(e.TEXTURE_2D, E), e.uniform1i(w.u_image, 0), e.uniform2f(w.u_size, L, Y), e.uniform1f(w.u_dpr, B), e.uniform1f(w.u_mode, N), e.uniform4f(w.u_crop, ...De(L, Y, Q.width, Q.height)), e.uniform1f(w.u_time, f), e.uniform3fv(w["u_colors[0]"], se), e.uniform3fv(w.u_base, ae), e.uniform1f(w.u_dim, oe), e.drawArrays(e.TRIANGLES, 0, 3);
    const v = Math.max(1, Math.ceil(s.width / 4)), M = Math.max(1, Math.ceil(s.height / 4));
    e.bindFramebuffer(e.FRAMEBUFFER, C), e.viewport(0, 0, v, M), j(l), e.bindTexture(e.TEXTURE_2D, c), e.uniform1i(k.u_background, 0), e.uniform2f(k.u_size, v, M), e.uniform2f(k.u_step, 5 * B / s.width, 5 * B / s.height), e.drawArrays(e.TRIANGLES, 0, 3), e.bindFramebuffer(e.FRAMEBUFFER, null);
  }
  function Ae(f, v, M, I, z = { tiltX: 0, tiltY: 0, camera: 780 }) {
    if (G || !$) return;
    const K = Math.min(d, f.length);
    for (let P = 0; P < K; P += 1) {
      const S = f[P];
      U.set([S.x, S.y, S.halfWidth, S.halfHeight], P * 4), b.set([S.radius, S.frost, S.chroma, S.pull], P * 4), _.set(S.tint, P * 4), u.set([S.depth, S.rounding, S.dome, S.ior], P * 4), R.set(S.lens, P * 4), O.set([S.glow, S.shadow, S.press, S.shape ?? 0], P * 4);
    }
    for (let P = 0; P < 6; P += 1) W[P] = Number(I.bands[P]) || 0;
    e.bindFramebuffer(e.FRAMEBUFFER, null), e.viewport(0, 0, s.width, s.height), j(a), e.activeTexture(e.TEXTURE0), e.bindTexture(e.TEXTURE_2D, c), e.activeTexture(e.TEXTURE1), e.bindTexture(e.TEXTURE_2D, x), e.uniform1i(y.u_scene, 0), e.uniform1i(y.u_frost, 1), e.activeTexture(e.TEXTURE2), e.bindTexture(e.TEXTURE_2D, F), e.uniform1i(y.u_shape, 2), e.uniform1f(y.u_shapeRange, le), e.uniform1f(y.u_shapeStep, ce), e.uniform2f(y.u_size, L, Y), e.uniform1f(y.u_dpr, B), e.uniform1i(y.u_count, K), e.uniform4fv(y["u_rect[0]"], U), e.uniform4fv(y["u_look[0]"], b), e.uniform4fv(y["u_tint[0]"], _), e.uniform4fv(y["u_profile[0]"], u), e.uniform4fv(y["u_lens[0]"], R), e.uniform4fv(y["u_extra[0]"], O), e.uniform2f(y.u_light, v.x, v.y), e.uniform2f(y.u_pointer, M.x, M.y), e.uniform1fv(y["u_bands[0]"], W), e.uniform1f(y.u_level, Math.min(1, Math.max(0, I.level)));
    const J = Math.cos(z.tiltY), Z = Math.sin(z.tiltY), ee = Math.cos(z.tiltX), te = Math.sin(z.tiltX), q = [J, 0, -Z, Z * te, ee, J * te, Z * ee, -te, J * ee];
    e.uniformMatrix3fv(y.u_rotation, !1, q), e.uniformMatrix3fv(y.u_inverse, !1, [q[0], q[3], q[6], q[1], q[4], q[7], q[2], q[5], q[8]]), e.uniform1f(y.u_camera, z.camera), e.uniform1f(y.u_sheen, Math.min(1, Math.max(0, z.sheen ?? 0)));
    const H = Math.max(Math.abs(z.tiltX), Math.abs(z.tiltY));
    let ne = 0;
    if (H > 5e-4) for (let P = 0; P < K; P += 1) {
      const S = f[P], de = S.depth + S.dome + 10, ie = Math.max(S.halfWidth, S.halfHeight), Se = ie * Math.cos(H) * z.camera / Math.max(z.camera * 0.15, z.camera - ie * Math.sin(H) - de);
      ne = Math.max(ne, Se - ie + de * Math.sin(H));
    }
    e.uniform1f(y.u_margin, H > 5e-4 ? ne + 6 : 2), e.drawArrays(e.TRIANGLES, 0, 3), e.activeTexture(e.TEXTURE0);
  }
  function Me() {
    G || (G = !0, t.forEach((f) => e.deleteShader(f)), n.forEach((f) => e.deleteProgram(f)), [E, c, x, F].forEach((f) => e.deleteTexture(f)), e.deleteFramebuffer(A), e.deleteFramebuffer(C), e.deleteBuffer(g), e.getExtension("WEBGL_lose_context")?.loseContext());
  }
  return { resize: xe, setSource: ye, setAurora: we, setDim: _e, setShape: be, drawScene: Ee, drawGlass: Ae, destroy: Me, maxPanes: d };
}
const Ue = [[30, 120], [120, 300], [300, 700], [700, 1600], [1600, 4e3], [4e3, 1e4]];
function ze(s, e, t, n = new Float32Array(6)) {
  if (!Number.isFinite(e) || e <= 0 || !Number.isInteger(t) || t < 32 || t > 32768 || (t & t - 1) !== 0 || s.length !== t / 2 || n.length < 6)
    throw new RangeError("Audio analysis needs a valid sample rate, power-of-two FFT and matching spectrum.");
  const i = e / t;
  return Ue.forEach(([r, o], m) => {
    const l = Math.max(1, Math.ceil(r / i)), h = Math.min(s.length, Math.ceil(o / i));
    let d = 0, a = 0;
    for (let E = l; E < h; E += 1) {
      const c = s[E] / 255;
      d += c * c, a = Math.max(a, c);
    }
    const g = h > l ? Math.sqrt(d / (h - l)) : 0;
    n[m] = Math.min(1, (g * 0.4 + a * 0.6) * 1.3);
  }), n;
}
function Xe(s) {
  const e = !!s.getUserMedia && !!s.createAudioContext;
  let t = { status: e ? "idle" : "unavailable", message: e ? "" : "Microphone analysis is not supported in this browser. The visual demo still works." }, n = null, i = null, r = null, o = null, m = new Uint8Array(0), l = new Float32Array(0);
  const h = new Float32Array(6), d = { bands: new Float32Array(6), level: 0 };
  let a = 0, g = null;
  async function E() {
    const A = n, C = i, T = r, w = o;
    n = null, i = null, r = null, o = null, T?.disconnect(), w?.disconnect(), C?.getTracks().forEach((k) => k.stop()), d.bands.fill(0), d.level = 0, A && A.state !== "closed" && await A.close().catch(() => {
    });
  }
  async function c() {
    a += 1, t = { status: e ? "idle" : "unavailable", message: e ? "" : t.message }, await E();
  }
  function x() {
    if (!e) return Promise.resolve(!1);
    if (t.status === "live") return Promise.resolve(!0);
    if (t.status === "requesting" && g) return g;
    const A = ++a;
    t = { status: "requesting", message: "Allow microphone access in your browser, or cancel to keep exploring." };
    const C = (async () => {
      try {
        const T = s.createAudioContext();
        n = T;
        const w = T.resume().then(() => null, (U) => U), k = await s.getUserMedia({ audio: { echoCancellation: !0, noiseSuppression: !0, autoGainControl: !0 }, video: !1 });
        if (A !== a)
          return k.getTracks().forEach((U) => U.stop()), !1;
        i = k;
        const y = await w;
        if (y) throw y;
        return A !== a ? !1 : (r = T.createMediaStreamSource(k), o = T.createAnalyser(), o.fftSize = 4096, o.minDecibels = -90, o.maxDecibels = -15, o.smoothingTimeConstant = 0.55, m = new Uint8Array(o.frequencyBinCount), l = new Float32Array(o.fftSize), r.connect(o), k.getAudioTracks().forEach((U) => U.addEventListener("ended", () => {
          c();
        }, { once: !0 })), t = { status: "live", message: "Microphone on. Audio is analysed on this device; nothing is saved or uploaded." }, !0);
      } catch (T) {
        if (A !== a || (await E(), A !== a)) return !1;
        const w = T && typeof T == "object" && "name" in T && typeof T.name == "string" ? T.name : "";
        return t = w === "NotAllowedError" || w === "SecurityError" ? { status: "denied", message: "Microphone permission was not allowed. You can allow it in your browser and try again; the demo still works." } : { status: "error", message: w === "NotFoundError" ? "No microphone was found. Connect one and try again, or use the visual demo." : "The microphone could not be started. Check your device and browser permissions, then try again." }, !1;
      }
    })();
    return g = C, C.finally(() => {
      g === C && (g = null);
    }), C;
  }
  function F(A) {
    return t.status !== "live" || !o || !n ? d : pe(o, n.sampleRate, A, d, h, m, l);
  }
  return { start: x, stop: c, read: F, getState: () => ({ ...t }) };
}
function pe(s, e, t, n, i, r, o) {
  s.getByteFrequencyData(r), s.getFloatTimeDomainData(o), ze(r, e, s.fftSize, i);
  let m = 0;
  for (const g of o) m += g * g;
  const l = Math.sqrt(m / o.length), h = Math.min(1, Math.max(0, (l - 6e-3) / 0.012)), d = Math.min(0.1, Math.max(0, Number.isFinite(t) ? t : 0));
  i.forEach((g, E) => {
    const c = g * h, x = c > n.bands[E] ? 0.045 : 0.23;
    n.bands[E] += (c - n.bands[E]) * (1 - Math.exp(-d / x));
  });
  const a = Math.min(1, Math.max(0, (l - 6e-3) * 9));
  return n.level += (a - n.level) * (1 - Math.exp(-d / (a > n.level ? 0.04 : 0.2))), n;
}
function Be(s) {
  const e = s.context.createAnalyser();
  e.fftSize = 4096, e.minDecibels = -90, e.maxDecibels = -15, e.smoothingTimeConstant = 0.55, s.connect(e);
  const t = { bands: new Float32Array(6), level: 0 }, n = new Float32Array(6), i = new Uint8Array(e.frequencyBinCount), r = new Float32Array(e.fftSize);
  let o = !0;
  return {
    read: (m) => o ? pe(e, s.context.sampleRate, m, t, n, i, r) : t,
    disconnect: () => {
      if (o) {
        o = !1;
        try {
          s.disconnect(e);
        } catch {
        }
        t.bands.fill(0), t.level = 0;
      }
    }
  };
}
function X(s, e, t) {
  return Math.min(t, Math.max(e, s));
}
function V(s, e, t, n, i = 420, r = 18) {
  const o = X(Number.isFinite(n) ? n : 0, 0, 0.06666666666666667), m = Math.max(1, Math.ceil(o / (1 / 240))), l = o / m;
  let h = s, d = e;
  for (let a = 0; a < m; a += 1)
    d += ((t - h) * i - d * r) * l, h += d * l;
  return [h, d];
}
function Ie(s, e, t) {
  const n = (s || "").trim().split(/\s+/)[0] ?? "", i = parseFloat(n);
  if (!Number.isFinite(i) || i <= 0) return 0;
  const r = Math.min(e, t), o = n.endsWith("%") ? i / 100 * r : i;
  return X(o, 0, r / 2);
}
function D(s, e, t = -1 / 0, n = 1 / 0) {
  const i = parseFloat(s);
  return Number.isFinite(i) ? X(i, t, n) : e;
}
function qe(s) {
  const e = /rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:[,\s/]+([\d.]+%?))?\s*\)/.exec(s || "");
  if (!e) return null;
  const t = e[4], n = t === void 0 ? 1 : t.endsWith("%") ? parseFloat(t) / 100 : parseFloat(t);
  return [Number(e[1]) / 255, Number(e[2]) / 255, Number(e[3]) / 255, X(n, 0, 1)];
}
function Ve(s) {
  return /\.(mp4|webm|mov|m4v)(\?|#|$)/i.test(s);
}
function he(s, e, t, n, i) {
  let r = 0;
  n[0] = 0, i[0] = -1 / 0, i[1] = 1 / 0;
  for (let o = 1; o < e; o += 1) {
    let m = (s[o] + o * o - (s[n[r]] + n[r] * n[r])) / (2 * o - 2 * n[r]);
    for (; m <= i[r]; )
      r -= 1, m = (s[o] + o * o - (s[n[r]] + n[r] * n[r])) / (2 * o - 2 * n[r]);
    r += 1, n[r] = o, i[r] = m, i[r + 1] = 1 / 0;
  }
  r = 0;
  for (let o = 0; o < e; o += 1) {
    for (; i[r + 1] < o; ) r += 1;
    t[o] = (o - n[r]) * (o - n[r]) + s[n[r]];
  }
}
function fe(s, e, t) {
  const n = new Float64Array(e * t);
  for (let h = 0; h < n.length; h += 1) n[h] = s(h) ? 0 : 1e20;
  const i = Math.max(e, t), r = new Float64Array(i), o = new Float64Array(i), m = new Float64Array(i + 1), l = new Int32Array(i);
  for (let h = 0; h < e; h += 1) {
    for (let d = 0; d < t; d += 1) r[d] = n[d * e + h];
    he(r, t, o, l, m);
    for (let d = 0; d < t; d += 1) n[d * e + h] = o[d];
  }
  for (let h = 0; h < t; h += 1) {
    const d = h * e;
    for (let a = 0; a < e; a += 1) r[a] = n[d + a];
    he(r, e, o, l, m);
    for (let a = 0; a < e; a += 1) n[d + a] = o[a];
  }
  return n;
}
function Ye(s, e, t, n, i = 1) {
  const r = fe((l) => s[l] >= 128, e, t), o = fe((l) => s[l] < 128, e, t), m = new Uint8Array(e * t);
  for (let l = 0; l < m.length; l += 1) {
    const h = s[l] / 255;
    let d = s[l] >= 128 ? -Math.sqrt(o[l]) + 0.5 : Math.sqrt(r[l]) - 0.5;
    h > 0.02 && h < 0.98 && (d = 0.5 - h);
    const a = d * i;
    m[l] = Math.max(0, Math.min(255, Math.round(127.5 + a / n * 127.5)));
  }
  return m;
}
const re = 48, Ge = 1920, Oe = [[1, 0.35, 0.12], [1, 0.18, 0.54], [0.42, 0.23, 1], [0, 0.76, 1]], $e = [0.04, 0.03, 0.12], ge = document.createElement("template");
ge.innerHTML = `<style>
:host { display: block; position: relative; isolation: isolate; overflow: hidden; min-height: 120px; color: #fff; background: #0a0818; }
:host([hidden]) { display: none; }
canvas { position: absolute; inset: 0; width: 100%; height: 100%; z-index: 0; pointer-events: none; opacity: 0; transition: opacity .5s; }
:host([data-ready]) canvas { opacity: 1; }
.fallback { position: absolute; inset: 0; z-index: 0; background: var(--image, linear-gradient(120deg, #ff5a1f, #ff2d8a 35%, #6a3bff 65%, #00c2ff)) center / cover no-repeat; }
:host([data-renderer='webgl'][data-ready]) .fallback { display: none; }
.content { position: relative; z-index: 1; height: 100%; }
</style><div class="fallback" part="fallback"></div><canvas part="canvas" aria-hidden="true"></canvas><div class="content" part="content"><slot></slot></div>`;
const We = `
liquid-glass [glass] { -webkit-tap-highlight-color: transparent; }
liquid-glass [glass-drag] { touch-action: none; cursor: grab; user-select: none; }
liquid-glass [glass-drag]:active { cursor: grabbing; }
liquid-glass[data-renderer='webgl'][data-ready] [glass-text] { color: transparent !important; }
liquid-glass:not([data-renderer='webgl']) [glass] { background: rgba(255,255,255,.16); border: 1px solid rgba(255,255,255,.38); box-shadow: inset 0 1px 1px rgba(255,255,255,.6), 0 10px 30px rgba(0,0,0,.18); backdrop-filter: blur(14px) saturate(1.7); -webkit-backdrop-filter: blur(14px) saturate(1.7); }`;
class ve extends HTMLElement {
  constructor() {
    super();
    p(this, "canvas");
    p(this, "fallback");
    p(this, "renderer", null);
    p(this, "states", /* @__PURE__ */ new WeakMap());
    p(this, "colorProbe", document.createElement("span"));
    p(this, "colorCache", /* @__PURE__ */ new Map());
    p(this, "motionQuery", window.matchMedia("(prefers-reduced-motion: reduce)"));
    p(this, "events", null);
    p(this, "resizeObserver", null);
    p(this, "visibility", null);
    p(this, "mutations", null);
    p(this, "video", null);
    p(this, "animatedScene", !0);
    p(this, "sceneDirty", !0);
    p(this, "stylesDirty", !0);
    p(this, "onScreen", !0);
    p(this, "frameId", 0);
    p(this, "frameCount", 0);
    p(this, "lastFrame", 0);
    p(this, "time", 0);
    p(this, "width", 1);
    p(this, "height", 1);
    p(this, "lastSignature", "");
    p(this, "backdropRequest", 0);
    p(this, "live", null);
    p(this, "shapeSignature", "");
    p(this, "shapeCanvas", document.createElement("canvas"));
    p(this, "pointer", { x: 0, y: 0, inside: !1, down: !1, target: null });
    p(this, "light", { x: 0, y: 0, vx: 0, vy: 0 });
    p(this, "bulge", { x: -1e4, y: -1e4, vx: 0, vy: 0 });
    p(this, "microphone", null);
    p(this, "node", null);
    p(this, "manualAudio", null);
    p(this, "silent", { bands: new Float32Array(6), level: 0 });
    p(this, "demo", { bands: new Float32Array(6), level: 0 });
    p(this, "level", 0);
    p(this, "lastAudioStatus", "");
    p(this, "liveSince", 0);
    const t = this.attachShadow({ mode: "open" });
    if (t.append(ge.content.cloneNode(!0)), this.canvas = t.querySelector("canvas"), this.fallback = t.querySelector(".fallback"), this.colorProbe.hidden = !0, t.append(this.colorProbe), !document.getElementById("liquid-glass-style")) {
      const n = document.createElement("style");
      n.id = "liquid-glass-style", n.textContent = We, document.head.append(n);
    }
  }
  /** 'aurora' (default), or the URL of an image or video to refract. */
  get backdrop() {
    return this.getAttribute("backdrop") || "aurora";
  }
  set backdrop(t) {
    this.setAttribute("backdrop", t);
  }
  /**
   * Refract a canvas you draw yourself instead of the backdrop attribute: it is read again every
   * frame, so whatever you animate on it moves behind the glass. null goes back to the attribute.
   */
  setBackdropCanvas(t) {
    if (this.live = t, this.backdropRequest += 1, !t) {
      this.applyBackdrop();
      return;
    }
    this.video?.pause(), this.video = null, this.animatedScene = !0, this.sceneDirty = !0, this.dataset.ready = "";
  }
  /** The glass elements this scene renders (at most 16). */
  get panes() {
    return Array.from(this.querySelectorAll("[glass]")).slice(0, this.renderer?.maxPanes ?? me);
  }
  /** The microphone's state: idle, requesting, live, denied, unavailable or error. */
  get microphoneState() {
    return this.mic().getState();
  }
  /** When the microphone went live (performance.now() time), for a recording timer. */
  get listeningSince() {
    return this.liveSince;
  }
  /** Ask for the microphone and let the glass glow with it. Call from a click or a key press. */
  async startMicrophone() {
    const t = this.mic().start();
    this.syncAudio();
    const n = await t;
    return n && (this.liveSince = performance.now()), this.syncAudio(), n;
  }
  /** Stop the microphone and release the device. */
  stopMicrophone() {
    this.microphone && (this.microphone.stop(), this.syncAudio());
  }
  /** Let any Web Audio node light the glass (a player, a synth). Returns a function that stops listening. */
  listen(t) {
    this.node?.disconnect();
    const n = Be(t);
    return this.node = n, () => {
      n.disconnect(), this.node === n && (this.node = null);
    };
  }
  /** Drive the glow with your own levels: six bands (low to high) and an overall level, 0 to 1. null stops. */
  setAudio(t) {
    if (!t) {
      this.manualAudio = null;
      return;
    }
    const n = new Float32Array(6);
    for (let i = 0; i < 6; i += 1) n[i] = X(Number(t.bands[i]) || 0, 0, 1);
    this.manualAudio = { bands: n, level: X(Number(t.level) || 0, 0, 1) };
  }
  mic() {
    return this.microphone ?? (this.microphone = Xe({
      getUserMedia: navigator.mediaDevices?.getUserMedia ? (t) => navigator.mediaDevices.getUserMedia(t) : void 0,
      createAudioContext: typeof window.AudioContext == "function" ? () => new AudioContext() : void 0
    })), this.microphone;
  }
  syncAudio() {
    const t = this.mic().getState();
    t.status !== this.lastAudioStatus && (this.lastAudioStatus = t.status, this.dataset.audio = t.status, this.dispatchEvent(new CustomEvent("microphone-change", { detail: t, bubbles: !0, composed: !0 })));
  }
  connectedCallback() {
    this.events = new AbortController();
    const t = this.events.signal;
    try {
      this.renderer = Le(this.canvas), this.dataset.renderer = "webgl";
    } catch (n) {
      console.warn("liquid-glass: WebGL is unavailable, using frosted CSS glass.", n), this.dataset.renderer = "css";
    }
    this.resizeObserver = new ResizeObserver(() => this.resize()), this.resizeObserver.observe(this), this.visibility = new IntersectionObserver(([n]) => {
      this.onScreen = n.isIntersecting, this.onScreen ? this.start() : this.stop();
    }), this.visibility.observe(this), this.mutations = new MutationObserver(() => {
      this.stylesDirty = !0;
    }), this.mutations.observe(this, { subtree: !0, childList: !0, attributes: !0 }), this.resize(), this.applyDim(), this.applyBackdrop(), this.addEventListener("pointermove", (n) => this.movePointer(n), { passive: !0, signal: t }), this.addEventListener("pointerenter", (n) => this.movePointer(n), { passive: !0, signal: t }), this.addEventListener("pointerleave", () => {
      this.pointer.inside = !1;
    }, { signal: t }), this.addEventListener("pointerdown", (n) => this.press(n), { signal: t }), window.addEventListener("pointerup", (n) => this.release(n), { signal: t }), window.addEventListener("pointercancel", (n) => this.release(n), { signal: t }), window.addEventListener("pointermove", (n) => this.dragMove(n), { passive: !0, signal: t }), document.addEventListener("visibilitychange", () => {
      document.hidden ? (this.stop(), this.stopMicrophone()) : this.start();
    }, { signal: t }), this.canvas.addEventListener("webglcontextlost", (n) => {
      n.preventDefault(), this.renderer?.destroy(), this.renderer = null, this.dataset.renderer = "css";
    }, { signal: t }), this.start();
  }
  disconnectedCallback() {
    this.stop(), this.events?.abort(), this.resizeObserver?.disconnect(), this.visibility?.disconnect(), this.mutations?.disconnect(), this.video?.pause(), this.video = null, this.microphone?.stop(), this.node?.disconnect(), this.node = null, this.renderer?.destroy(), this.renderer = null, delete this.dataset.ready;
  }
  attributeChangedCallback(t, n, i) {
    n === i || !this.isConnected || ((t === "backdrop" || t === "colors") && this.applyBackdrop(), t === "dim" && this.applyDim());
  }
  // ---- the backdrop
  applyBackdrop() {
    if (this.live) return;
    const t = ++this.backdropRequest, n = this.backdrop.trim();
    if (this.video?.pause(), this.video = null, n === "aurora" || !n) {
      this.renderer?.setAurora(this.auroraColors(), $e), this.renderer?.setSource(null), this.fallback.style.removeProperty("--image"), this.animatedScene = !0, this.sceneDirty = !0, this.dataset.ready = "";
      return;
    }
    if (this.fallback.style.setProperty("--image", `url("${n.replace(/"/g, "%22")}")`), this.renderer?.setSource("wait"), this.sceneDirty = !0, Ve(n)) {
      const i = document.createElement("video");
      Object.assign(i, { muted: !0, loop: !0, playsInline: !0, autoplay: !0, crossOrigin: "anonymous", src: n }), i.setAttribute("muted", ""), this.video = i, this.animatedScene = !0, i.addEventListener("playing", () => {
        t === this.backdropRequest && (this.dataset.ready = "");
      }, { once: !0 }), i.play().catch(() => {
      });
      return;
    }
    this.animatedScene = !1, Ne(n).then((i) => {
      t === this.backdropRequest && (this.renderer?.setSource(i.source, i.width, i.height), this.sceneDirty = !0, this.dataset.ready = "");
    }).catch((i) => {
      t === this.backdropRequest && (console.warn(`liquid-glass: the backdrop "${n}" could not load (images on other sites need CORS headers). Showing the aurora.`, i), this.renderer?.setSource(null), this.animatedScene = !0, this.sceneDirty = !0, this.dataset.ready = "");
    });
  }
  applyDim() {
    const t = D(this.getAttribute("dim") ?? "", 0, 0, 0.9);
    this.renderer?.setDim(t), this.fallback.style.filter = t ? `brightness(${1 - t})` : "", this.sceneDirty = !0;
  }
  auroraColors() {
    const t = (this.getAttribute("colors") || "").split(/[\s,]+/).filter(Boolean).map((n) => this.resolveColor(n)).filter((n) => n[3] > 0);
    return t.length ? t.map(([n, i, r]) => [n, i, r]) : Oe;
  }
  // ---- reading the panes
  resolveColor(t) {
    const n = this.colorCache.get(t);
    if (n) return n;
    this.colorProbe.style.color = "", this.colorProbe.style.color = t;
    const i = (this.colorProbe.style.color ? qe(getComputedStyle(this.colorProbe).color) : null) ?? [0, 0, 0, 0];
    return this.colorCache.set(t, i), i;
  }
  readLook(t, n, i) {
    const r = getComputedStyle(t), o = Math.min(n, i) / 2, m = r.getPropertyValue("--glass-tint").trim(), l = r.getPropertyValue("--glass-lens").trim().split(/[\s,]+/).map(Number), h = D(r.getPropertyValue("--glass-depth"), 1, 0.2, 3);
    if (t.hasAttribute("glass-text")) {
      const a = t.offsetWidth > 0 ? n / t.offsetWidth : 1, g = (parseFloat(r.fontSize) || 16) * a, E = g * 0.1 * h;
      return {
        radius: 0,
        frost: D(r.getPropertyValue("--glass-frost"), 0, 0, 1),
        chroma: D(r.getPropertyValue("--glass-chroma"), 0.012, 0, 0.1),
        pull: 0,
        tint: m ? this.resolveColor(m) : [1, 1, 1, 0],
        depth: E,
        rounding: Math.min(E * 0.85, g * 0.05),
        dome: E * 0.12 * D(r.getPropertyValue("--glass-dome"), 1, 0, 4),
        ior: D(r.getPropertyValue("--glass-ior"), 1.45, 1.05, 2.4),
        lens: null,
        glow: D(r.getPropertyValue("--glass-glow"), 1, 0, 2),
        shadow: D(r.getPropertyValue("--glass-shadow"), 0.6, 0, 1),
        press: 1
      };
    }
    const d = Math.min(44, Math.max(o * 0.36, Math.min(o * 0.7, 14))) * h;
    return {
      radius: Ie(r.borderTopLeftRadius, n, i),
      frost: D(r.getPropertyValue("--glass-frost"), 0, 0, 1),
      chroma: D(r.getPropertyValue("--glass-chroma"), 8e-3, 0, 0.1),
      pull: D(r.getPropertyValue("--glass-pull"), Math.min(9, o * 0.3), 0, 40),
      tint: m ? this.resolveColor(m) : [1, 1, 1, 0],
      depth: d,
      rounding: Math.min(d * 0.73, o * 0.9),
      dome: d * 0.48 * D(r.getPropertyValue("--glass-dome"), 1, 0, 4),
      ior: D(r.getPropertyValue("--glass-ior"), 1.38, 1.05, 2.4),
      lens: l.length === 3 && l.every(Number.isFinite) ? [l[0], l[1], l[2]] : null,
      glow: D(r.getPropertyValue("--glass-glow"), 1, 0, 2),
      shadow: D(r.getPropertyValue("--glass-shadow"), 1, 0, 1),
      press: D(r.getPropertyValue("--glass-press"), 1.045, 0.8, 1.3)
    };
  }
  state(t) {
    let n = this.states.get(t);
    return n || (n = { scale: 1, scaleVelocity: 0, glow: 0, glowVelocity: 0, stretchX: 1, stretchXVelocity: 0, stretchY: 1, stretchYVelocity: 0, look: null, drag: null, offsetX: 0, offsetY: 0, lens: [0, 0, 0, 0], lensVelocity: [0, 0, 0, 0] }, this.states.set(t, n)), n;
  }
  // ---- pointer: light, press, drag
  movePointer(t) {
    const n = this.getBoundingClientRect();
    this.pointer.x = t.clientX - n.left, this.pointer.y = t.clientY - n.top, this.pointer.inside = !0;
  }
  press(t) {
    this.movePointer(t);
    const n = t.target?.closest?.("[glass]");
    if (!(!n || !this.contains(n)) && (this.pointer.down = !0, this.pointer.target = n, n.hasAttribute("glass-drag"))) {
      const i = this.state(n);
      i.drag = { pointer: t.pointerId, startX: t.clientX, startY: t.clientY, baseX: i.offsetX, baseY: i.offsetY, lastX: t.clientX, lastY: t.clientY, lastT: t.timeStamp, vx: 0, vy: 0 }, n.setPointerCapture?.(t.pointerId);
    }
  }
  dragMove(t) {
    const n = this.pointer.target, i = n ? this.states.get(n)?.drag : null;
    if (!n || !i || i.pointer !== t.pointerId) return;
    const r = this.state(n), o = Math.max(1, t.timeStamp - i.lastT) / 1e3;
    i.vx = i.vx * 0.6 + (t.clientX - i.lastX) / o * 0.4, i.vy = i.vy * 0.6 + (t.clientY - i.lastY) / o * 0.4, i.lastX = t.clientX, i.lastY = t.clientY, i.lastT = t.timeStamp, r.offsetX = i.baseX + t.clientX - i.startX, r.offsetY = i.baseY + t.clientY - i.startY, n.style.translate = `${r.offsetX}px ${r.offsetY}px`;
  }
  release(t) {
    const n = this.pointer.target;
    if (n) {
      const i = this.states.get(n);
      i?.drag && i.drag.pointer === t.pointerId && (i.drag = null);
    }
    this.pointer.down = !1, this.pointer.target = null;
  }
  // ---- the frame loop
  resize() {
    const t = this.getBoundingClientRect();
    this.width = Math.max(1, t.width), this.height = Math.max(1, t.height);
    try {
      this.renderer?.resize(this.width, this.height);
    } catch (n) {
      console.warn(n), this.renderer?.destroy(), this.renderer = null, this.dataset.renderer = "css";
    }
    this.sceneDirty = !0, this.stylesDirty = !0, this.lastSignature = "";
  }
  start() {
    this.frameId || !this.isConnected || !this.onScreen || document.hidden || (this.lastFrame = performance.now(), this.frameId = requestAnimationFrame((t) => this.frame(t)));
  }
  stop() {
    cancelAnimationFrame(this.frameId), this.frameId = 0;
  }
  /**
   * Draw every glass-text element's words into a mask at its place on the scene, and turn the
   * mask into the distance field the shader traces. Rebuilt only when a word, font or box changes.
   */
  updateShapes(t, n) {
    const i = this.renderer;
    if (!i) return "";
    const r = t.map(({ element: c, box: x }) => {
      const F = getComputedStyle(c), A = c.offsetWidth > 0 ? x.width / c.offsetWidth : 1, C = (c.textContent || "").replace(/\s+/g, " ").trim(), T = `${F.fontStyle} ${F.fontWeight} ${((parseFloat(F.fontSize) || 16) * A).toFixed(2)}px ${F.fontFamily}`, w = (parseFloat(F.letterSpacing) || 0) * A, k = F.textAlign, y = (parseFloat(F.paddingLeft) || 0) * A, U = (parseFloat(F.paddingRight) || 0) * A;
      return { text: C, font: T, spacing: w, align: k, padLeft: y, padRight: U, left: x.left - n.left, top: x.top - n.top, width: x.width, height: x.height };
    }).filter((c) => c.text), o = r.map((c) => `${c.text}|${c.font}|${c.spacing.toFixed(2)}|${c.align}|${c.left.toFixed(1)},${c.top.toFixed(1)},${c.width.toFixed(1)},${c.height.toFixed(1)}`).join("#") + `@${this.width}x${this.height}`;
    if (o === this.shapeSignature) return o;
    if (this.shapeSignature = o, !r.length)
      return i.setShape(null, 1, 1, re), o;
    const m = Math.max(1, Math.max(this.width, this.height) / Ge), l = Math.max(1, Math.ceil(this.width / m)), h = Math.max(1, Math.ceil(this.height / m)), d = this.shapeCanvas;
    (d.width !== l || d.height !== h) && (d.width = l, d.height = h);
    const a = d.getContext("2d", { willReadFrequently: !0 });
    a.setTransform(1, 0, 0, 1, 0, 0), a.clearRect(0, 0, l, h), a.setTransform(1 / m, 0, 0, 1 / m, 0, 0), a.fillStyle = "#fff";
    for (const c of r) {
      a.font = c.font, a.letterSpacing = `${c.spacing}px`, a.textBaseline = "alphabetic";
      const x = a.measureText(c.text), F = x.fontBoundingBoxAscent ?? x.actualBoundingBoxAscent, A = x.fontBoundingBoxDescent ?? x.actualBoundingBoxDescent, C = c.top + c.height / 2 + (F - A) / 2, T = x.width - c.spacing;
      let w = c.left + (c.width - T) / 2;
      (c.align === "left" || c.align === "start") && (w = c.left + c.padLeft), (c.align === "right" || c.align === "end") && (w = c.left + c.width - c.padRight - T), a.textAlign = "left", a.fillText(c.text, w, C);
    }
    const g = a.getImageData(0, 0, l, h).data, E = new Uint8ClampedArray(l * h);
    for (let c = 0; c < E.length; c += 1) E[c] = g[c * 4 + 3];
    return i.setShape(Ye(E, l, h, re, m), l, h, re), o;
  }
  frame(t) {
    this.frameId = requestAnimationFrame((b) => this.frame(b));
    const n = this.renderer;
    if (!n) return;
    const i = X((t - this.lastFrame) / 1e3, 0, 1 / 15);
    this.lastFrame = t, this.frameCount += 1;
    const r = this.motionQuery.matches || this.hasAttribute("reduce-motion"), o = this.video;
    o && o.readyState >= 2 && (n.setSource(o, o.videoWidth, o.videoHeight), this.sceneDirty = !0), this.live && (n.setSource(this.live, this.live.width, this.live.height), this.sceneDirty = !0), this.animatedScene && !r && (this.time += i, this.sceneDirty = !0);
    const m = this.sceneDirty;
    this.sceneDirty && (n.drawScene(this.time), this.sceneDirty = !1);
    const l = this.light, h = this.pointer.inside ? this.pointer.x - 100 : this.width * 0.18, d = this.pointer.inside ? this.pointer.y - 160 : -this.height * 0.25;
    [l.x, l.vx] = V(l.x, l.vx, h, i, 330, 24), [l.y, l.vy] = V(l.y, l.vy, d, i, 330, 24);
    const a = this.bulge;
    this.pointer.inside && a.x < -5e3 && Object.assign(a, { x: this.pointer.x, y: this.pointer.y, vx: 0, vy: 0 }), !this.pointer.inside || r ? Object.assign(a, { x: -1e4, y: -1e4, vx: 0, vy: 0 }) : ([a.x, a.vx] = V(a.x, a.vx, this.pointer.x, i, 140, 9), [a.y, a.vy] = V(a.y, a.vy, this.pointer.y, i, 140, 9)), this.microphone && this.syncAudio();
    let g = this.silent;
    this.microphone?.getState().status === "live" ? g = this.microphone.read(i) : this.node ? g = this.node.read(i) : this.manualAudio ? g = this.manualAudio : this.hasAttribute("inner-light") && (this.demo.bands.forEach((b, _) => {
      this.demo.bands[_] = 0.26 + Math.sin(this.time * (1.4 + _ * 0.28) + _) * 0.17;
    }), this.demo.level = Math.max(...this.demo.bands) * 0.6, g = this.demo, !this.animatedScene && !r && (this.time += i));
    const E = g === this.demo ? 0.85 : g === this.silent ? 0 : X(g.level * 1.5, 0, 1);
    this.level = r ? E : this.level + (E - this.level) * Math.min(i * 7, 1);
    const c = g !== this.silent || this.level > 2e-3;
    (this.stylesDirty || this.frameCount % 30 === 0) && this.panes.forEach((b) => {
      this.state(b).look = null;
    }), this.stylesDirty = !1;
    const x = this.getBoundingClientRect(), F = [], A = [];
    let C = Math.abs(l.vx) + Math.abs(l.vy) + Math.abs(a.vx) + Math.abs(a.vy) > 0.5 || c;
    for (const b of this.panes) {
      const _ = b.getBoundingClientRect();
      if (_.width < 1 || _.height < 1 || _.right < x.left || _.left > x.right || _.bottom < x.top || _.top > x.bottom) continue;
      const u = this.state(b);
      u.look ?? (u.look = this.readLook(b, _.width, _.height));
      const R = u.look, O = this.pointer.down && this.pointer.target === b, W = this.pointer.inside && this.pointer.x >= _.left - x.left && this.pointer.x <= _.right - x.left && this.pointer.y >= _.top - x.top && this.pointer.y <= _.bottom - x.top, L = u.drag, Y = O ? R.press : W ? 1 + (R.press - 1) * 0.3 : 1, B = L ? X(Math.abs(L.vx) / 2600, 0, 0.22) : 0, $ = L ? X(Math.abs(L.vy) / 2600, 0, 0.22) : 0;
      r ? Object.assign(u, { scale: Y, scaleVelocity: 0, stretchX: 1, stretchY: 1, stretchXVelocity: 0, stretchYVelocity: 0, glow: O ? 1 : 0, glowVelocity: 0 }) : ([u.scale, u.scaleVelocity] = V(u.scale, u.scaleVelocity, Y, i, 520, 14), [u.stretchX, u.stretchXVelocity] = V(u.stretchX, u.stretchXVelocity, 1 + B - $ * 0.5, i, 300, 12), [u.stretchY, u.stretchYVelocity] = V(u.stretchY, u.stretchYVelocity, 1 + $ - B * 0.5, i, 300, 12), [u.glow, u.glowVelocity] = V(u.glow, u.glowVelocity, O ? 1 : W ? 0.35 : 0, i, 260, 22)), L && (L.vx *= 0.9, L.vy *= 0.9), C || (C = Math.abs(u.scaleVelocity) + Math.abs(u.stretchXVelocity) + Math.abs(u.stretchYVelocity) + Math.abs(u.glowVelocity) > 2e-3);
      const G = R.lens ? [R.lens[0], R.lens[1], R.lens[2], 1] : [u.lens[0], u.lens[1], u.lens[2], 0];
      u.lens[3] === 0 && R.lens && !u.lensVelocity.some(Boolean) && (u.lens = [...G]);
      for (let N = 0; N < 4; N += 1) {
        if (r) {
          u.lens[N] = G[N], u.lensVelocity[N] = 0;
          continue;
        }
        [u.lens[N], u.lensVelocity[N]] = V(u.lens[N], u.lensVelocity[N], G[N], i, 330, 24);
      }
      C || (C = u.lensVelocity.some((N) => Math.abs(N) > 0.01)), F.push({
        x: _.left - x.left + _.width / 2,
        y: _.top - x.top + _.height / 2,
        halfWidth: _.width / 2 * u.scale * u.stretchX,
        halfHeight: _.height / 2 * u.scale * u.stretchY,
        radius: R.radius * u.scale,
        frost: R.frost,
        chroma: R.chroma,
        pull: R.pull,
        tint: R.tint,
        depth: R.depth,
        rounding: R.rounding,
        dome: R.dome,
        ior: R.ior,
        lens: [u.lens[0] * u.scale, u.lens[1] * u.scale, u.lens[2] * u.scale, u.lens[3]],
        glow: R.glow,
        shadow: R.shadow,
        press: u.glow,
        shape: b.hasAttribute("glass-text") ? 1 : 0
      }), b.hasAttribute("glass-text") && A.push({ element: b, box: _ });
    }
    const w = `${this.updateShapes(A, x)}|${this.getAttribute("tilt")}|${this.getAttribute("camera")}|${this.getAttribute("sheen")}|` + F.map((b) => `${b.x.toFixed(1)},${b.y.toFixed(1)},${b.halfWidth.toFixed(1)},${b.halfHeight.toFixed(1)},${b.radius},${b.frost},${b.depth},${b.dome},${b.tint.join(",")}`).join("|");
    if (!m && !C && w === this.lastSignature) return;
    this.lastSignature = w;
    const [k = 0, y = 0] = (this.getAttribute("tilt") || "").split(/[\s,]+/).map(Number).map((b) => Number.isFinite(b) ? X(b, -0.6, 0.6) : 0), U = D(this.getAttribute("camera") ?? "", 780, 200, 5e3);
    n.drawGlass(F, { x: l.x, y: l.y }, { x: a.x, y: a.y }, g === this.silent ? { bands: this.silent.bands, level: this.level } : { bands: g.bands, level: this.level }, { tiltX: k, tiltY: y, camera: U, sheen: D(this.getAttribute("sheen") ?? "", 0, 0, 1) });
  }
}
p(ve, "observedAttributes", ["backdrop", "colors", "dim", "inner-light", "tilt", "camera", "sheen"]);
typeof customElements < "u" && !customElements.get("liquid-glass") && customElements.define("liquid-glass", ve);
export {
  ve as LiquidGlassElement
};
