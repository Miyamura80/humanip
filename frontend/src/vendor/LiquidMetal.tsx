// Vendored from https://eito.me/ui ("Liquid metal border" pattern).
// Original: metal.jakubantalik.com. Code unchanged; em dashes in comments swapped for hyphens.
import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type HTMLAttributes,
  type ReactNode,
} from "react";

/**
 * LiquidMetal - wrap any element in an animated, real-time "liquid metal" ring.
 *
 * Drop it around a button, chip or icon and it draws a shimmering metallic
 * border that hugs the child's rounded corners, plus an optional soft glow.
 * The metal is a WebGL plasma shader (simplex-noise FBM warping four sine bands,
 * mapped through a five-stop metallic palette) - the same technique metal.fx
 * uses - so it reads as flowing chrome rather than a static gradient.
 *
 * It is deliberately self-contained:
 *   - No three.js, no filters, no runtime deps beyond React - raw WebGL1.
 *   - One shared GL context and one compiled program back *every* instance on
 *     the page; each wrapper only owns a cheap 2D canvas that receives a
 *     drawImage() copy per frame. Mounting fifty metal buttons still compiles
 *     the shader once and keeps one WebGL context.
 *   - The ring is a pure CSS "gradient-border" mask over the canvas, so it
 *     respects any border-radius and never touches the child's interactivity
 *     (pointer-events: none on every overlay).
 *   - It pauses itself when scrolled off-screen or when the tab is hidden, and
 *     honours prefers-reduced-motion by holding a single frame.
 *
 * Deps: react only.
 */

export type MetalPreset = "chromatic" | "silver" | "gold";

export const METAL_PRESETS: MetalPreset[] = ["chromatic", "silver", "gold"];

export interface LiquidMetalProps
  extends Omit<HTMLAttributes<HTMLSpanElement>, "children"> {
  /** The element the metal ring frames (a button, chip, icon…). */
  children: ReactNode;
  /** Palette. "chromatic" is an iridescent rainbow; the others are as named. */
  preset?: MetalPreset;
  /** "auto" follows the surrounding light/dark theme. */
  theme?: "auto" | "dark" | "light";
  /** Overall effect intensity, 0–1: scales the metal's opacity and its glow. */
  strength?: number;
  /** Corner radius of the child, in px. The ring nests just outside it. */
  radius?: number;
  /** Ring thickness in px. */
  ringWidth?: number;
  /** Draw a soft blurred halo behind the child (best on dark backgrounds). */
  glow?: boolean;
  /** Animation speed multiplier. */
  speed?: number;
  /** Freeze on the current frame. */
  paused?: boolean;
}

/* ---- preset palettes (verbatim from metal.fx) ----------------------------- */

interface Mode {
  colors: string[]; // 7 stops; the shader samples the first five
  alphas: number[];
  direction: number;
  speed: number;
  intensity: number;
  scale: number;
  distortion: number;
  complexity: number;
  blur: number;
  vignette: number;
  shaderOpacity: number;
}

const PRESETS: Record<MetalPreset, { dark: Mode; light: Mode }> = {
  chromatic: {
    dark: {
      colors: ["#000000", "#aae8ff", "#c5fe9e", "#f7888d", "#0d0d0d", "#fffdc3", "#007cff"],
      alphas: [1, 1, 1, 1, 1, 1, 1],
      direction: 80, speed: 1.2, intensity: 2, scale: 1.6, distortion: 0.3,
      complexity: 0.68, blur: 1, vignette: 0.26, shaderOpacity: 1,
    },
    light: {
      colors: ["#ffffff", "#ffffff", "#ffffff", "#ffb3b3", "#adadad", "#f5ff70", "#007cff"],
      alphas: [1, 1, 1, 1, 1, 1, 1],
      direction: 80, speed: 1.2, intensity: 2, scale: 2.5, distortion: 0.3,
      complexity: 0.68, blur: 1, vignette: 0.24, shaderOpacity: 1,
    },
  },
  silver: {
    dark: {
      colors: ["#000000", "#dedede", "#747270", "#e5e5e5", "#0d0d0d", "#ffffff", "#e6e6e6"],
      alphas: [1, 1, 1, 1, 1, 1, 1],
      direction: 80, speed: 1.2, intensity: 2, scale: 2.5, distortion: 0.3,
      complexity: 0.68, blur: 1, vignette: 0.26, shaderOpacity: 0.88,
    },
    light: {
      colors: ["#f6f6f6", "#ffffff", "#ffffff", "#f7f7f7", "#c9c9c9", "#d0d0d0", "#d1d1d1"],
      alphas: [1, 1, 1, 1, 1, 1, 1],
      direction: 80, speed: 1.2, intensity: 2, scale: 2.5, distortion: 0.3,
      complexity: 0.68, blur: 1, vignette: 0.2, shaderOpacity: 1,
    },
  },
  gold: {
    dark: {
      colors: ["#000000", "#ffffff", "#ffffff", "#f7d488", "#0d0d0d", "#fffdc3", "#ffffff"],
      alphas: [1, 1, 1, 1, 1, 1, 1],
      direction: 80, speed: 1.0, intensity: 2, scale: 2.5, distortion: 0.3,
      complexity: 0.68, blur: 1, vignette: 0.26, shaderOpacity: 0.92,
    },
    light: {
      colors: ["#fff8e1", "#fffbe0", "#ffffff", "#fff6d6", "#d2c7a7", "#dcd2bc", "#f9f7e5"],
      alphas: [1, 1, 1, 1, 1, 1, 1],
      direction: 80, speed: 1.2, intensity: 2, scale: 2.5, distortion: 0.3,
      complexity: 0.68, blur: 1, vignette: 0.22, shaderOpacity: 1,
    },
  },
};

const hexToRgb = (hex: string): [number, number, number] => {
  const h = hex.replace("#", "");
  const n = parseInt(h.length === 3 ? h.replace(/(.)/g, "$1$1") : h, 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
};

/* ---- shaders (fragment verbatim from metal.fx) ---------------------------- */

const VERT = `attribute vec2 a_position;
void main() { gl_Position = vec4(a_position, 0.0, 1.0); }`;

const FRAG = `precision highp float;
uniform vec2 u_resolution;
uniform float u_time;
uniform vec3 u_color1, u_color2, u_color3, u_color4, u_color5;
uniform float u_alpha1, u_alpha2, u_alpha3, u_alpha4, u_alpha5;
uniform float u_intensity, u_scale, u_direction;
uniform float u_distortion, u_complexity;
uniform float u_vignette, u_blur, u_shaderOpacity;

vec3 mod289(vec3 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec2 mod289v2(vec2 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec3 permute(vec3 x) { return mod289((x * 34.0 + 1.0) * x); }

float snoise(vec2 v) {
  const vec4 C = vec4(0.211324865405187, 0.366025403784439,
                      -0.577350269189626, 0.024390243902439);
  vec2 i = floor(v + dot(v, C.yy));
  vec2 x0 = v - i + dot(i, C.xx);
  vec2 i1 = (x0.x > x0.y) ? vec2(1.0, 0.0) : vec2(0.0, 1.0);
  vec4 x12 = x0.xyxy + C.xxzz;
  x12.xy -= i1;
  i = mod289v2(i);
  vec3 p = permute(permute(i.y + vec3(0.0, i1.y, 1.0)) + i.x + vec3(0.0, i1.x, 1.0));
  vec3 m = max(0.5 - vec3(dot(x0, x0), dot(x12.xy, x12.xy), dot(x12.zw, x12.zw)), 0.0);
  m = m * m; m = m * m;
  vec3 x_ = 2.0 * fract(p * C.www) - 1.0;
  vec3 h = abs(x_) - 0.5;
  vec3 ox = floor(x_ + 0.5);
  vec3 a0 = x_ - ox;
  m *= 1.79284291400159 - 0.85373472095314 * (a0 * a0 + h * h);
  vec3 g;
  g.x = a0.x * x0.x + h.x * x0.y;
  g.yz = a0.yz * x12.xz + h.yz * x12.yw;
  return 130.0 * dot(m, g);
}

float fbm(vec2 p, float oct) {
  float val = 0.0, amp = 0.5;
  int n = int(oct);
  for (int i = 0; i < 7; i++) {
    if (i >= n) break;
    val += amp * snoise(p);
    p *= 2.0;
    amp *= 0.5;
  }
  return val;
}
float nfbm(vec2 p) { return fbm(p, 3.0 + u_complexity * 4.0); }

vec3 palette(float t) {
  t = clamp(t, 0.0, 1.0);
  t = t * t * (3.0 - 2.0 * t);
  float k = 64.0;
  float w1 = u_alpha1 * exp(-k * t * t);
  float w2 = u_alpha2 * exp(-k * (t - 0.25) * (t - 0.25));
  float w3 = u_alpha3 * exp(-k * (t - 0.5)  * (t - 0.5));
  float w4 = u_alpha4 * exp(-k * (t - 0.75) * (t - 0.75));
  float w5 = u_alpha5 * exp(-k * (t - 1.0)  * (t - 1.0));
  float total = w1 + w2 + w3 + w4 + w5 + 0.0001;
  return (u_color1 * w1 + u_color2 * w2 + u_color3 * w3 +
          u_color4 * w4 + u_color5 * w5) / total;
}

vec2 warp(vec2 p, float t) {
  float str = u_distortion * 2.0;
  return vec2(
    nfbm(p + vec2(t * 0.1, 0.0)),
    nfbm(p + vec2(0.0, t * 0.12) + 5.0)
  ) * str;
}

vec3 computeEffect(vec2 uv, float aspect, float t) {
  vec2 p = (uv - 0.5) * u_scale;
  p.x *= aspect;
  p += vec2(cos(u_direction), sin(u_direction)) * t * 0.15;

  float freq = 3.0 + u_complexity * 8.0;
  float val = 0.0;
  val += sin(p.x * freq + t);
  val += sin(p.y * freq + t * 1.3);
  val += sin((p.x + p.y) * freq * 0.7 + t * 0.7);
  val += sin(length(p) * freq * 0.8 - t * 1.5);
  vec2 w = warp(p, t);
  val += (w.x + w.y) * u_distortion;
  val = val * 0.2 * u_intensity + 0.5;
  return palette(clamp(val, 0.0, 1.0));
}

void main() {
  vec2 uv = gl_FragCoord.xy / u_resolution;
  float aspect = u_resolution.x / u_resolution.y;
  float t = u_time;

  vec3 col;
  if (u_blur < 0.01) {
    col = computeEffect(uv, aspect, t);
  } else {
    float r = u_blur * 0.02;
    col  = computeEffect(uv,                 aspect, t) * 0.4;
    col += computeEffect(uv + vec2( r, 0.0), aspect, t) * 0.15;
    col += computeEffect(uv + vec2(-r, 0.0), aspect, t) * 0.15;
    col += computeEffect(uv + vec2(0.0,  r), aspect, t) * 0.15;
    col += computeEffect(uv + vec2(0.0, -r), aspect, t) * 0.15;
  }

  col = pow(col, vec3(1.3));

  float edgeDist = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
  float vigPx = 40.0 / min(u_resolution.x, u_resolution.y);
  float vigRange = vigPx * (1.0 + u_vignette * 3.0);
  float vig = edgeDist * edgeDist / (vigRange * vigRange);
  vig = smoothstep(0.0, 1.0, vig);
  col *= mix(1.0, vig, u_vignette);

  gl_FragColor = vec4(col, u_shaderOpacity);
}`;

/* ---- shared WebGL renderer ------------------------------------------------ */
/* One context + one program serve every LiquidMetal on the page. Each instance
 * owns only lightweight 2D canvases that receive a drawImage() copy per frame. */

type Uniforms = Record<string, WebGLUniformLocation | null>;

interface Instance {
  targets: HTMLCanvasElement[]; // ring canvas, optional glow canvas (same size)
  w: number; // device px
  h: number;
  mode: Mode;
  strength: number;
  speed: number;
  paused: boolean;
  visible: boolean;
  elapsed: number;
}

interface Shared {
  gl: WebGLRenderingContext;
  off: HTMLCanvasElement;
  program: WebGLProgram;
  u: Uniforms;
}

const registry = new Set<Instance>();
let shared: Shared | null = null;
let raf = 0;
let lastNow = 0;
let glFailed = false; // WebGL unavailable - stop scheduling frames for good.

const compile = (gl: WebGLRenderingContext, type: number, src: string) => {
  const s = gl.createShader(type)!;
  gl.shaderSource(s, src);
  gl.compileShader(s);
  return s;
};

const ensureShared = (): Shared | null => {
  if (shared) return shared;
  if (typeof document === "undefined") return null;
  const off = document.createElement("canvas");
  off.width = 2;
  off.height = 2;
  const gl = off.getContext("webgl", {
    alpha: true,
    premultipliedAlpha: false,
    antialias: true,
    preserveDrawingBuffer: true, // let drawImage() read the buffer after draw
  });
  if (!gl) return null;

  const program = gl.createProgram()!;
  gl.attachShader(program, compile(gl, gl.VERTEX_SHADER, VERT));
  gl.attachShader(program, compile(gl, gl.FRAGMENT_SHADER, FRAG));
  gl.linkProgram(program);
  gl.useProgram(program);

  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  // Fullscreen triangle.
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(program, "a_position");
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);

  const names = [
    "u_resolution", "u_time", "u_intensity", "u_scale", "u_direction",
    "u_distortion", "u_complexity", "u_vignette", "u_blur", "u_shaderOpacity",
    "u_color1", "u_color2", "u_color3", "u_color4", "u_color5",
    "u_alpha1", "u_alpha2", "u_alpha3", "u_alpha4", "u_alpha5",
  ];
  const u: Uniforms = {};
  for (const name of names) u[name] = gl.getUniformLocation(program, name);

  shared = { gl, off, program, u };
  return shared;
};

// Render the shader on a buffer larger than the element and copy back only the
// centre. The metal.fx vignette darkens the frame's edges - which is exactly
// where a thin ring lives - so sampling the brighter interior keeps the ring
// luminous instead of muddy. (metal.fx does the same with its 1.6× button scale.)
const OVERSCAN = 1.6;

const renderInstance = (s: Shared, inst: Instance) => {
  const { gl, off, u } = s;
  const { w, h, mode } = inst;
  const bw = Math.max(2, Math.round(w * OVERSCAN));
  const bh = Math.max(2, Math.round(h * OVERSCAN));
  if (off.width !== bw || off.height !== bh) {
    off.width = bw;
    off.height = bh;
  }
  gl.viewport(0, 0, bw, bh);
  gl.clearColor(0, 0, 0, 0);
  gl.clear(gl.COLOR_BUFFER_BIT);

  gl.uniform2f(u.u_resolution, bw, bh);
  gl.uniform1f(u.u_time, inst.elapsed);
  gl.uniform1f(u.u_intensity, mode.intensity);
  gl.uniform1f(u.u_scale, mode.scale);
  gl.uniform1f(u.u_direction, mode.direction);
  gl.uniform1f(u.u_distortion, mode.distortion);
  gl.uniform1f(u.u_complexity, mode.complexity);
  gl.uniform1f(u.u_vignette, mode.vignette);
  gl.uniform1f(u.u_blur, mode.blur);
  gl.uniform1f(u.u_shaderOpacity, mode.shaderOpacity * inst.strength);
  for (let i = 0; i < 5; i++) {
    gl.uniform3fv(u[`u_color${i + 1}`], hexToRgb(mode.colors[i]));
    gl.uniform1f(u[`u_alpha${i + 1}`], mode.alphas[i]);
  }
  gl.drawArrays(gl.TRIANGLES, 0, 3);

  const sx = (bw - w) / 2;
  const sy = (bh - h) / 2;
  for (const canvas of inst.targets) {
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) continue;
    ctx.clearRect(0, 0, w, h);
    ctx.drawImage(off, sx, sy, w, h, 0, 0, w, h);
  }
};

const tick = (now: number) => {
  const s = ensureShared();
  if (!s) {
    // No WebGL: give up instead of rescheduling forever on an idle registry.
    glFailed = true;
    raf = 0;
    return;
  }
  // Cap dt so a long rAF stall (a backgrounded tab) doesn't jump the shader
  // clock forward in a single frame when it resumes.
  const dt = lastNow ? Math.min((now - lastNow) / 1000, 0.05) : 0;
  lastNow = now;
  for (const inst of registry) {
    if (!inst.visible) continue;
    if (!inst.paused) inst.elapsed += dt * inst.speed;
    renderInstance(s, inst);
  }
  raf = registry.size ? requestAnimationFrame(tick) : 0;
};

const register = (inst: Instance) => {
  registry.add(inst);
  if (!raf && !glFailed) {
    lastNow = 0;
    raf = requestAnimationFrame(tick);
  }
};

const unregister = (inst: Instance) => {
  registry.delete(inst);
  if (registry.size === 0 && raf) {
    cancelAnimationFrame(raf);
    raf = 0;
  }
};

/* ---- theme + reduced-motion helpers --------------------------------------- */

const resolveTheme = (el: HTMLElement | null): "dark" | "light" => {
  for (let node = el; node; node = node.parentElement) {
    const attr = node.getAttribute("data-theme");
    if (attr === "dark" || attr === "light") return attr;
    if (node.classList.contains("dark")) return "dark";
    if (node.classList.contains("light")) return "light";
  }
  if (typeof window !== "undefined" && window.matchMedia) {
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  return "dark";
};

/* ---- component ------------------------------------------------------------ */

const LiquidMetal = ({
  children,
  preset = "chromatic",
  theme = "auto",
  strength = 1,
  radius = 12,
  ringWidth = 2,
  glow = true,
  speed = 1,
  paused = false,
  className,
  style,
  ...rest
}: LiquidMetalProps) => {
  const wrapRef = useRef<HTMLSpanElement>(null);
  const ringRef = useRef<HTMLCanvasElement>(null);
  const glowRef = useRef<HTMLCanvasElement>(null);
  const elapsedRef = useRef(0); // survives instance re-creation so pause freezes
  const [resolved, setResolved] = useState<"dark" | "light">("dark");
  const [reduce, setReduce] = useState(false);

  const clampStrength = Math.max(0, Math.min(1, strength));

  // Follow the surrounding theme when asked to.
  useEffect(() => {
    if (theme !== "auto") {
      setResolved(theme);
      return;
    }
    const el = wrapRef.current;
    const update = () => setResolved(resolveTheme(el));
    update();
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    mq?.addEventListener?.("change", update);
    const mo = new MutationObserver(update);
    for (let node = el?.parentElement; node; node = node.parentElement) {
      mo.observe(node, { attributes: true, attributeFilter: ["class", "data-theme"] });
    }
    return () => {
      mq?.removeEventListener?.("change", update);
      mo.disconnect();
    };
  }, [theme]);

  // Live-track prefers-reduced-motion.
  useEffect(() => {
    const mq = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    if (!mq) return;
    const update = () => setReduce(mq.matches);
    update();
    mq.addEventListener?.("change", update);
    return () => mq.removeEventListener?.("change", update);
  }, []);

  const mode = PRESETS[preset][resolved];

  // One registry entry drives the ring (+ optional glow) canvases.
  useEffect(() => {
    const wrap = wrapRef.current;
    const ring = ringRef.current;
    if (!wrap || !ring) return;

    const dpr = Math.min(2, typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1);
    const targets = glow && glowRef.current ? [ring, glowRef.current] : [ring];

    const inst: Instance = {
      targets,
      w: 2,
      h: 2,
      mode,
      strength: clampStrength,
      speed,
      paused: paused || reduce,
      visible: true,
      elapsed: elapsedRef.current || (reduce ? 0.6 : 0),
    };

    const measure = () => {
      const rect = wrap.getBoundingClientRect();
      inst.w = Math.max(2, Math.round(rect.width * dpr));
      inst.h = Math.max(2, Math.round(rect.height * dpr));
    };
    measure();

    register(inst);

    // Viewport and tab visibility are tracked separately: the instance only
    // renders while on-screen AND the tab is foregrounded, so returning to a
    // tab whose wrapper is scrolled away keeps it paused.
    let onScreen = true;
    const syncVisible = () => {
      inst.visible = onScreen && !document.hidden;
    };
    const ro = new ResizeObserver(measure);
    ro.observe(wrap);
    const io = new IntersectionObserver(
      ([e]) => {
        onScreen = e.isIntersecting;
        syncVisible();
      },
      { threshold: 0 },
    );
    io.observe(wrap);
    document.addEventListener("visibilitychange", syncVisible);

    return () => {
      elapsedRef.current = inst.elapsed; // preserve the clock for the next instance
      unregister(inst);
      ro.disconnect();
      io.disconnect();
      document.removeEventListener("visibilitychange", syncVisible);
    };
    // Re-create the instance whenever anything about how it draws changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [preset, resolved, clampStrength, speed, paused, reduce, glow, ringWidth]);

  const outerRadius = radius + ringWidth;

  // The "gradient-border" mask: show the canvas only in the padding band, so
  // the metal reads as a ring that respects the corner radius.
  const ringMask: CSSProperties = {
    padding: ringWidth,
    borderRadius: outerRadius,
    boxSizing: "border-box",
    WebkitMaskImage: "linear-gradient(#000 0 0), linear-gradient(#000 0 0)",
    WebkitMaskClip: "content-box, border-box",
    WebkitMaskComposite: "xor",
    maskImage: "linear-gradient(#000 0 0), linear-gradient(#000 0 0)",
    maskClip: "content-box, border-box",
    maskComposite: "exclude",
  };

  const canvasFill: CSSProperties = {
    position: "absolute",
    inset: 0,
    width: "100%",
    height: "100%",
    display: "block",
  };

  const glowSpread = ringWidth * 3;

  return (
    <span
      ref={wrapRef}
      className={className}
      style={{
        position: "relative",
        display: "inline-flex",
        padding: ringWidth,
        borderRadius: outerRadius,
        ...style,
      }}
      {...rest}
    >
      {glow && (
        <span
          aria-hidden="true"
          style={{
            position: "absolute",
            inset: -glowSpread,
            borderRadius: outerRadius + glowSpread,
            filter: `blur(${Math.max(6, ringWidth * 4)}px)`,
            opacity: 0.55 * clampStrength,
            pointerEvents: "none",
            zIndex: 0,
          }}
        >
          <canvas ref={glowRef} style={canvasFill} />
        </span>
      )}

      <span aria-hidden="true" style={{ position: "absolute", inset: 0, pointerEvents: "none", zIndex: 1, ...ringMask }}>
        <canvas ref={ringRef} style={canvasFill} />
      </span>

      {/* Content wrapper. `flex: 1` + minWidth: 0 lets it fill a stretched
          outer (a caller passing width/flex) instead of shrink-wrapping, so a
          full-width child sits flush inside the ring; for the default content-
          sized outer it still hugs the content. */}
      <span style={{ position: "relative", zIndex: 2, display: "inline-flex", flex: "1 1 auto", minWidth: 0, borderRadius: radius }}>
        {children}
      </span>
    </span>
  );
};

export default LiquidMetal;
