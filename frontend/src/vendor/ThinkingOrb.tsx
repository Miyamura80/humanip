// Vendored from https://eito.me/ui ("Thinking orbs" pattern).
// Original: orbs.jakubantalik.com. Code unchanged; em dashes in comments swapped for hyphens.
import {
  useEffect,
  useRef,
  useState,
  type CanvasHTMLAttributes,
} from "react";

/**
 * ThinkingOrb - a dotted, animated "thought-orb" status indicator for AI and
 * agent UIs. Drop one next to a streaming message to show *what* the agent is
 * doing: searching, solving, listening, connecting, and so on.
 *
 * The whole thing is one <canvas> drawing filled arcs - no WebGL, no SVG
 * filters, no `ctx.filter` - so it renders pixel-identically in every browser
 * and stays cheap enough to run several at once. Nine hand-tuned states are
 * built from one primitive: points distributed on (or around) a slowly-rotating
 * unit sphere, projected orthographically, then depth-shaded so dots on the far
 * side read dimmer and smaller. Each state just decides where the dots live and
 * how they move.
 *
 * It behaves, too: it honours `prefers-reduced-motion` (one static frame), and
 * pauses itself when scrolled off-screen or when the tab is hidden.
 *
 * Deps: react only.
 */

export type OrbState =
  | "working"
  | "searching"
  | "solving"
  | "listening"
  | "connecting"
  | "weaving"
  | "composing"
  | "breathing"
  | "shaping";

export interface ThinkingOrbProps
  extends Omit<CanvasHTMLAttributes<HTMLCanvasElement>, "children"> {
  /** Which agent activity to animate. */
  state?: OrbState;
  /** Rendered CSS size in px (square). 64 for avatars, 20 for inline. */
  size?: number;
  /** Dot colour source. "auto" follows the surrounding theme. */
  theme?: "auto" | "dark" | "light";
  /** Explicit dot colour; wins over `theme`. Any CSS colour string. */
  color?: string;
  /** Animation speed multiplier. */
  speed?: number;
  /** Freeze on the current frame. */
  paused?: boolean;
}

export const ORB_STATES: OrbState[] = [
  "working",
  "searching",
  "solving",
  "listening",
  "connecting",
  "weaving",
  "composing",
  "breathing",
  "shaping",
];

const ARIA: Record<OrbState, string> = {
  working: "Working",
  searching: "Searching",
  solving: "Solving",
  listening: "Listening",
  connecting: "Connecting",
  weaving: "Weaving",
  composing: "Composing",
  breathing: "Thinking",
  shaping: "Shaping",
};

/* ---- tiny math kit -------------------------------------------------------- */

type Pt = [x: number, y: number, z: number, boost?: number];

const TAU = Math.PI * 2;
const clamp01 = (v: number) => (v < 0 ? 0 : v > 1 ? 1 : v);
const smooth = (v: number) => {
  const t = clamp01(v);
  return t * t * (3 - 2 * t);
};

/** Rotate a unit vector: `ay` spins about Y, then `ax` tilts about X. */
const rot = (x: number, y: number, z: number, ay: number, ax: number): Pt => {
  const x1 = x * Math.cos(ay) + z * Math.sin(ay);
  const z1 = -x * Math.sin(ay) + z * Math.cos(ay);
  const y2 = y * Math.cos(ax) - z1 * Math.sin(ax);
  const z2 = y * Math.sin(ax) + z1 * Math.cos(ax);
  return [x1, y2, z2];
};

/** Evenly-spread points on a unit sphere (Fibonacci lattice). */
const fibSphere = (n: number): Pt[] => {
  const pts: Pt[] = [];
  const ga = Math.PI * (3 - Math.sqrt(5));
  for (let i = 0; i < n; i++) {
    const y = 1 - (i / (n - 1)) * 2;
    const r = Math.sqrt(Math.max(0, 1 - y * y));
    const th = ga * i;
    pts.push([Math.cos(th) * r, y, Math.sin(th) * r]);
  }
  return pts;
};

/** Deterministic PRNG so "random" layouts are stable across frames. */
const mulberry32 = (seed: number) => () => {
  seed |= 0;
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};

/* ---- per-state dot fields ------------------------------------------------- */
/* Each returns points in rotated unit-sphere space; an optional 4th value
 * boosts a dot's size/opacity (used for highlights like the scan band). */

const fields: Record<OrbState, (t: number, n: number) => Pt[]> = {
  working: (t, n) => {
    const pts: Pt[] = [];
    const orbits = [0.55, -0.65, 0.08];
    const per = Math.round(n / orbits.length);
    orbits.forEach((tilt, o) => {
      for (let i = 0; i < per; i++) {
        const a = (i / per) * TAU + t * 1.7 + o * 2.1;
        pts.push(rot(Math.cos(a), 0, Math.sin(a), t * 0.5, tilt));
      }
    });
    return pts;
  },

  searching: (t, n) => {
    const scan = Math.sin(t * 0.9);
    return fibSphere(n).map(([x, y, z]) => {
      const p = rot(x, y, z, t * 0.35, 0.35);
      const d = Math.abs(p[0] - scan);
      p[3] = d < 0.2 ? 1 + (1 - d / 0.2) * 1.5 : 1;
      return p;
    });
  },

  solving: (t, n) => {
    const lats = [-0.6, -0.3, 0, 0.3, 0.6];
    const per = Math.round(n / lats.length);
    const cycle = (t * 0.5) % 1;
    const solved = smooth((cycle - 0.55) / 0.35);
    const jitter = (1 - solved) * 0.9;
    const rng = mulberry32(99);
    const pts: Pt[] = [];
    lats.forEach((l) => {
      const r = Math.sqrt(Math.max(0, 1 - l * l));
      for (let i = 0; i < per; i++) {
        const off = (rng() - 0.5) * TAU * jitter;
        const a = (i / per) * TAU + off + t * 0.2;
        pts.push(rot(Math.cos(a) * r, l, Math.sin(a) * r, t * 0.3, 0.3));
      }
    });
    return pts;
  },

  listening: (t, n) => {
    const lats = [-0.62, -0.32, 0, 0.32, 0.62];
    const per = Math.round(n / lats.length);
    const pts: Pt[] = [];
    lats.forEach((l) => {
      const base = Math.sqrt(Math.max(0, 1 - l * l));
      for (let i = 0; i < per; i++) {
        const a = (i / per) * TAU;
        const wave = 1 + 0.16 * Math.sin(a * 3 - t * 3 + l * 4);
        pts.push(
          rot(Math.cos(a) * base * wave, l, Math.sin(a) * base * wave, t * 0.25, 0.3),
        );
      }
    });
    return pts;
  },

  connecting: (t, n) => {
    return fibSphere(Math.round(n * 0.6)).map(([x, y, z]) => {
      const p = rot(x, y, z, t * 0.2, 0.4);
      p[3] = 1.25;
      return p;
    });
  },

  weaving: (t, n) => {
    const per = Math.round(n / 3);
    const pts: Pt[] = [];
    for (let s = 0; s < 3; s++) {
      const phase = (s * TAU) / 3;
      for (let i = 0; i < per; i++) {
        const u = i / per;
        const lon = u * TAU * 2;
        const lat = Math.sin(u * TAU * 3 + phase + t * 1.2) * 0.7;
        const r = Math.cos(lat);
        pts.push(rot(Math.cos(lon) * r, Math.sin(lat), Math.sin(lon) * r, t * 0.4, 0.25));
      }
    }
    return pts;
  },

  composing: (t, n) => {
    const rows = [-0.14, -0.05, 0.05, 0.14];
    const per = Math.round(n / rows.length);
    const pts: Pt[] = [];
    rows.forEach((v) => {
      for (let i = 0; i < per; i++) {
        const u = i / per;
        const lon = u * TAU;
        const lat = v + 0.3 * Math.sin(u * TAU * 2 - t * 2.2);
        const r = Math.sqrt(Math.max(0, 1 - lat * lat));
        pts.push(rot(Math.cos(lon) * r, lat, Math.sin(lon) * r, t * 0.3, 0.16));
      }
    });
    return pts;
  },

  breathing: (t, n) => {
    const pts: Pt[] = [];
    [1, 0.6].forEach((scale, ring) => {
      for (let i = 0; i < n; i++) {
        const a = (i / n) * TAU;
        const morph = 1 + 0.09 * Math.sin(3 * a + t * 1.3 + ring);
        const rad = (1 + 0.1 * Math.sin(t * 1.2)) * scale * morph;
        pts.push(rot(Math.cos(a) * rad, Math.sin(a) * rad, 0, t * 0.25, 1.1));
      }
    });
    return pts;
  },

  shaping: (t, n) => {
    const shapes = [0, 3, 4]; // circle (0 sides), triangle, square
    const tt = t * 0.45;
    const idx = Math.floor(tt) % shapes.length;
    const f = smooth(tt % 1);
    const spin = t * 0.15;
    const shapePt = (sides: number, u: number): [number, number] => {
      if (sides === 0) return [Math.cos(u * TAU), Math.sin(u * TAU)];
      const seg = u * sides;
      const k = Math.floor(seg);
      const g = seg - k;
      const va = -Math.PI / 2 + (k / sides) * TAU;
      const vb = -Math.PI / 2 + ((k + 1) / sides) * TAU;
      const scl = 1.12; // fatten polygons so they fill the same disc as the circle
      return [
        (Math.cos(va) + (Math.cos(vb) - Math.cos(va)) * g) * scl,
        (Math.sin(va) + (Math.sin(vb) - Math.sin(va)) * g) * scl,
      ];
    };
    const from = shapes[idx];
    const to = shapes[(idx + 1) % shapes.length];
    const pts: Pt[] = [];
    for (let i = 0; i < n; i++) {
      const u = i / n;
      const a = shapePt(from, u);
      const b = shapePt(to, u);
      const x = a[0] + (b[0] - a[0]) * f;
      const y = a[1] + (b[1] - a[1]) * f;
      // 2D outline: spin in-plane, keep dots uniformly lit (z fixed at front).
      pts.push([x * Math.cos(spin) - y * Math.sin(spin), x * Math.sin(spin) + y * Math.cos(spin), 0.7]);
    }
    return pts;
  },
};

/* ---- theme resolution ----------------------------------------------------- */

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

const ThinkingOrb = ({
  state = "working",
  size = 64,
  theme = "auto",
  color,
  speed = 1,
  paused = false,
  style,
  ...rest
}: ThinkingOrbProps) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const elapsedRef = useRef(0);
  const [resolved, setResolved] = useState<"dark" | "light">("dark");
  const [reduce, setReduce] = useState(false);

  // Track the effective theme when following the surroundings.
  useEffect(() => {
    if (theme !== "auto") {
      setResolved(theme);
      return;
    }
    const el = canvasRef.current;
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

  // Re-evaluate the reduced-motion preference live, so toggling it at the OS
  // level stops or starts the animation without a remount.
  useEffect(() => {
    const mq = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    if (!mq) return;
    const update = () => setReduce(mq.matches);
    update();
    mq.addEventListener?.("change", update);
    return () => mq.removeEventListener?.("change", update);
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    const dpr = Math.min(2, typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1);
    canvas.width = Math.round(size * dpr);
    canvas.height = Math.round(size * dpr);

    const dot = color ?? (resolved === "dark" ? "#eaf0f7" : "#14171b");
    const link = color ?? (resolved === "dark" ? "#eaf0f7" : "#14171b");
    const big = size >= 40;
    const n = big ? 132 : 46;
    const dotR = Math.max(0.7, size * (big ? 0.026 : 0.036));
    const cx = size / 2;
    const cy = size / 2;
    const R = size * 0.42;

    const draw = (t: number) => {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, size, size);
      ctx.fillStyle = dot;

      const pts = fields[state](t, n);

      // "connecting" wires nearest neighbours together before the dots land.
      if (state === "connecting") {
        const prog = (t * 0.12) % 1;
        const shown = Math.floor(smooth(prog) * pts.length);
        ctx.strokeStyle = link;
        ctx.lineWidth = Math.max(0.4, size * 0.006);
        for (let i = 0; i < Math.min(shown, pts.length); i++) {
          let best = -1;
          let bd = Infinity;
          for (let j = 0; j < pts.length; j++) {
            if (j === i) continue;
            const dx = pts[i][0] - pts[j][0];
            const dy = pts[i][1] - pts[j][1];
            const dz = pts[i][2] - pts[j][2];
            const d = dx * dx + dy * dy + dz * dz;
            if (d < bd) {
              bd = d;
              best = j;
            }
          }
          if (best < 0) continue;
          const a = pts[i];
          const b = pts[best];
          ctx.globalAlpha = 0.28 * clamp01((a[2] + b[2]) / 2 + 1);
          ctx.beginPath();
          ctx.moveTo(cx + a[0] * R, cy - a[1] * R);
          ctx.lineTo(cx + b[0] * R, cy - b[1] * R);
          ctx.stroke();
        }
      }

      // Depth-sort so near dots paint over far ones, then shade by depth.
      pts.sort((a, b) => a[2] - b[2]);
      for (const [x, y, z, boost = 1] of pts) {
        const depth = (z + 1) / 2;
        const r = dotR * (0.45 + 0.55 * depth) * boost;
        ctx.globalAlpha = clamp01((0.18 + 0.82 * depth) * Math.min(1, boost));
        ctx.beginPath();
        ctx.arc(cx + x * R, cy - y * R, Math.max(0.1, r), 0, TAU);
        ctx.fill();
      }
      ctx.globalAlpha = 1;
    };

    // Reduced motion / paused: draw a single frame and schedule nothing. Both
    // freeze at whatever time has already elapsed (a fresh mount sits at 0),
    // so pausing holds the current frame instead of jumping back to the start.
    if (reduce) {
      draw(elapsedRef.current || 0.6);
      return;
    }
    if (paused) {
      draw(elapsedRef.current);
      return;
    }

    // Advance a persistent clock by real elapsed time each frame, and actually
    // stop scheduling frames while off-screen or backgrounded - not just skip
    // the paint - so idle orbs cost nothing. The loop restarts from where it
    // left off when the orb returns to view or the tab regains focus.
    let raf = 0;
    let running = false;
    let onScreen = true;
    let last = performance.now();

    const frame = (now: number) => {
      elapsedRef.current += ((now - last) / 1000) * speed;
      last = now;
      draw(elapsedRef.current);
      raf = requestAnimationFrame(frame);
    };
    const startLoop = () => {
      if (running) return;
      running = true;
      last = performance.now();
      raf = requestAnimationFrame(frame);
    };
    const stopLoop = () => {
      running = false;
      cancelAnimationFrame(raf);
    };

    const io = new IntersectionObserver(
      ([entry]) => {
        onScreen = entry.isIntersecting;
        if (onScreen && !document.hidden) startLoop();
        else stopLoop();
      },
      { threshold: 0 },
    );
    io.observe(canvas);

    const onVisibility = () => {
      if (!document.hidden && onScreen) startLoop();
      else stopLoop();
    };
    document.addEventListener("visibilitychange", onVisibility);

    if (!document.hidden) startLoop();

    return () => {
      stopLoop();
      io.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [state, size, color, resolved, speed, paused, reduce]);

  return (
    <canvas
      ref={canvasRef}
      role="img"
      aria-label={rest["aria-label"] ?? ARIA[state]}
      style={{ width: size, height: size, display: "block", ...style }}
      {...rest}
    />
  );
};

export default ThinkingOrb;
