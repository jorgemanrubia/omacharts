import { AbsoluteFill, Easing, OffthreadVideo, continueRender, delayRender, interpolate, staticFile, useCurrentFrame, useVideoConfig } from "remotion";

// The site's monospace, for the keycaps — loaded before any frame is taken,
// or the first ones would be drawn in a fallback.
const fontReady = delayRender("Cascadia Mono");
new FontFace("Cascadia Mono", `url(${staticFile("cascadia-mono-700.woff2")})`, { weight: "700" })
  .load()
  .then((face) => {
    document.fonts.add(face);
    continueRender(fontReady);
  });

// The plan is made by bin/video from what the recording did — every click's
// moment and position — so this file only draws. Times are seconds into the
// recording; positions are its pixels.

export type Zoom = { start: number; end: number; x: number; y: number; scale: number };
export type Badge = { start: number; end: number; keys: string[] };
/** The terminal standing beside the app, `share` of the width on the right. */
export type Split = { start: number; end: number; share: number };

/** Where a music-driven video's camera looks, and how close, at time t:
 * the centre of the view in the recording's pixels. `ease` is how the camera
 * arrives here from the keyframe before: a snap that settles inside a beat,
 * a slow glide, a smooth return, or a hold where it stays put. */
export type CameraKey = { t: number; x: number; y: number; scale: number; ease: "snap" | "glide" | "smooth" | "hold" | "risein" | "whip" };
/** A whip-pan's motion blur: strongest mid-move, over `d` seconds from `t`. */
export type Blur = { t: number; d: number };
/** A kick on a drum: the picture a touch closer for an instant, falling back. */
export type Pulse = { t: number; strength: number; decay?: number };
/** A whip across a cut on its beat: the old picture flung out one way, the
 * new one flung in after it, blurred with the speed. */
export type Whip = { t: number; dir: number };
/** A lift toward white and back, under a cut. */
export type Flash = { t: number; strength: number };

export type PolishProps = {
  src: string;
  width: number;
  height: number;
  fps: number;
  frames: number;
  zooms: Zoom[];
  badges: Badge[];
  splits?: Split[];
  camera?: CameraKey[];
  pulses?: Pulse[];
  flashes?: Flash[];
  whips?: Whip[];
  blurs?: Blur[];
};

const EASES = {
  snap: Easing.bezier(0.16, 1, 0.3, 1),
  glide: Easing.bezier(0.37, 0, 0.63, 1),
  smooth: Easing.bezier(0.45, 0, 0.25, 1),
  hold: (x: number) => x,
  risein: (x: number) => x * x,
  whip: Easing.bezier(0.65, 0, 0.35, 1),
};

// The view at time t: interpolated between the keyframes either side, by the
// later one's ease.
const view = (keys: CameraKey[], t: number, width: number, height: number) => {
  const full = { x: width / 2, y: height / 2, scale: 1 };
  if (!keys.length) return full;
  const i = keys.findIndex((k) => k.t > t);
  if (i === -1) return keys[keys.length - 1];
  if (i === 0) return keys[0];
  const a = keys[i - 1];
  const b = keys[i];
  const k = EASES[b.ease](Math.min(1, Math.max(0, (t - a.t) / Math.max(1e-6, b.t - a.t))));
  return { x: a.x + (b.x - a.x) * k, y: a.y + (b.y - a.y) * k, scale: a.scale + (b.scale - a.scale) * k };
};

// How much of a kick is left at t: in over two frames, out over a beat's
// share.
const kick = (pulses: Pulse[], t: number) =>
  pulses.reduce((sum, p) => {
    const d = t - p.t;
    const decay = p.decay ?? 0.11;
    if (d < -0.03 || d > decay * 4.5) return sum;
    const shape = d < 0 ? (d + 0.03) / 0.03 : Math.exp(-d / decay);
    return sum + p.strength * shape;
  }, 0);

// Where a whip has the picture at t: a shift in widths, and how fast it is
// moving (for the blur). Out over 0.14 s before the beat, in over 0.2 s after.
const WHIP_OUT = 0.14;
const WHIP_IN = 0.2;
const WHIP_SHIFT = 0.11;
const whipAt = (whips: Whip[], t: number) => {
  for (const w of whips) {
    const d = t - w.t;
    if (d >= -WHIP_OUT && d < 0) {
      const k = (d + WHIP_OUT) / WHIP_OUT;
      return { shift: -w.dir * WHIP_SHIFT * k * k, speed: k };
    }
    if (d >= 0 && d < WHIP_IN) {
      const k = 1 - d / WHIP_IN;
      return { shift: w.dir * WHIP_SHIFT * k * k * k, speed: k };
    }
  }
  return { shift: 0, speed: 0 };
};

const lift = (flashes: Flash[], t: number) =>
  flashes.reduce((most, f) => {
    const d = t - f.t;
    if (d < -0.04 || d > 0.45) return most;
    const shape = d < 0 ? (d + 0.04) / 0.04 : Math.exp(-d / 0.09);
    return Math.max(most, f.strength * shape);
  }, 0);

const EASE_IN = 0.5; // seconds to settle into a zoom
const EASE_OUT = 0.6; // and to pull back out of it
const BADGE_FADE = 0.15;

const ease = Easing.bezier(0.45, 0, 0.25, 1);

// How long the move into the split and back out takes: long enough to cover
// the window snapping into its new size, short enough to stay a transition.
const SPLIT_MOVE = 0.7;

// How far into a split's arrival or departure the picture is at time t: 0
// before and after, 1 at its height.
const swell = (moment: number, t: number) =>
  interpolate(t, [moment - 0.05, moment + SPLIT_MOVE * 0.35, moment + SPLIT_MOVE], [0, 1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: ease,
  });

// How far into a zoom the picture is at time t: 0 at full frame, 1 settled.
const amount = (zoom: Zoom, t: number) =>
  interpolate(t, [zoom.start, zoom.start + EASE_IN, zoom.end - EASE_OUT, zoom.end], [0, 1, 1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: ease,
  });

export const Polish = ({ src, width, height, zooms, badges, splits = [], camera = [], pulses = [], flashes = [], whips = [], blurs = [] }: PolishProps) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const t = frame / fps;

  const zoom = zooms.find((z) => t >= z.start && t <= z.end);
  const k = zoom ? amount(zoom, t) : 0;
  let scale = zoom ? 1 + (zoom.scale - 1) * k : 1;
  // Scaling about the point of interest keeps that point still and the frame
  // filled: nothing outside the recording ever shows at the edges.
  let origin = zoom ? `${zoom.x}px ${zoom.y}px` : "50% 50%";
  let shift = "";
  if (camera.length) {
    // The camera's view centred where it looks, kept inside the recording so
    // its edges never show, with the kick on top.
    const v = view(camera, t, width, height);
    const whip = whipAt(whips, t);
    // A whip needs room to slide without showing an edge: it pushes in by
    // as much as it shifts.
    scale = Math.max(1, v.scale) * (1 + kick(pulses, t)) * (1 + 2.2 * Math.abs(whip.shift));
    const vx = v.x + whip.shift * width;
    const cx = Math.min(width - width / (2 * scale), Math.max(width / (2 * scale), vx));
    const cy = Math.min(height - height / (2 * scale), Math.max(height / (2 * scale), v.y));
    origin = "0px 0px";
    shift = `translate(${(width / 2 - scale * cx).toFixed(2)}px, ${(height / 2 - scale * cy).toFixed(2)}px) `;
  } else if (pulses.length) {
    scale *= 1 + kick(pulses, t);
  }
  const flash = lift(flashes, t);
  const panBlur = blurs.reduce((most, b) => {
    const k = (t - b.t) / b.d;
    return k <= 0 || k >= 1 ? most : Math.max(most, Math.sin(Math.PI * k));
  }, 0);
  const whipBlur = camera.length ? Math.max(whipAt(whips, t).speed, panBlur) : 0;

  // The terminal arriving or leaving: the window snaps to its new size in a
  // few frames, so the picture breathes in and out over it, softened at the
  // height of the move, with a band of light crossing the way the terminal
  // came from.
  const moments = splits.flatMap((s) => [s.start, s.end]);
  const move = Math.max(0, ...moments.map((m) => swell(m, t)));
  const sweepAt = moments.find((m) => t >= m - 0.05 && t <= m + SPLIT_MOVE);
  const split = splits.find((s) => t >= s.start && t <= s.end);
  const unit = height / 2160;

  return (
    <AbsoluteFill style={{ backgroundColor: "black" }}>
      <AbsoluteFill
        style={{
          transform: `${shift}scale(${scale * (1 - 0.035 * move)})`,
          transformOrigin: origin,
          filter:
            move > 0.01
              ? `blur(${(10 * move * unit).toFixed(2)}px) brightness(${1 + 0.12 * move})`
              : flash > 0.01 || whipBlur > 0.01
                ? `${whipBlur > 0.01 ? `blur(${(14 * whipBlur * unit).toFixed(2)}px) ` : ""}brightness(${(1 + flash).toFixed(3)})`
                : undefined,
        }}
      >
        <OffthreadVideo src={staticFile(src)} muted style={{ width, height }} />
      </AbsoluteFill>
      {sweepAt !== undefined && <Sweep moment={sweepAt} t={t} width={width} />}
      {split && <PaneLabels split={split} t={t} width={width} height={height} />}
      {badges
        .filter((b) => t >= b.start && t <= b.end)
        .map((b) => (
          <KeyBadge key={`${b.start}`} badge={b} t={t} height={height} />
        ))}
    </AbsoluteFill>
  );
};

// A band of light crossing the frame from the right, where the terminal
// comes in.
const Sweep = ({ moment, t, width }: { moment: number; t: number; width: number }) => {
  const at = interpolate(t, [moment - 0.05, moment + SPLIT_MOVE], [1.15, -0.35], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: ease,
  });
  const strength = Math.sin(Math.PI * Math.min(1, Math.max(0, (t - moment + 0.05) / (SPLIT_MOVE + 0.05))));
  return (
    <AbsoluteFill
      style={{
        background: `linear-gradient(100deg, transparent ${(at - 0.18) * 100}%, rgba(170, 200, 255, ${0.16 * strength}) ${at * 100}%, transparent ${(at + 0.18) * 100}%)`,
        mixBlendMode: "screen",
        width,
      }}
    />
  );
};

// What each half is as the terminal comes in beside the app: a small label
// under the top of each pane, the chart's with a beat that says it is live,
// gone before the agent's output reaches it.
const LABEL_HOLD = 3.2;

const PaneLabels = ({ split, t, width, height }: { split: Split; t: number; width: number; height: number }) => {
  const unit = height / 2160;
  const opacity = interpolate(t, [split.start + SPLIT_MOVE, split.start + SPLIT_MOVE + 0.4, split.start + LABEL_HOLD, split.start + LABEL_HOLD + 0.5], [0, 1, 1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  const beat = 0.55 + 0.45 * Math.sin(t * Math.PI * 1.6);
  const chip = (text: string, x: number, live: boolean) => (
    <div
      style={{
        position: "absolute",
        top: 150 * unit,
        left: x,
        transform: "translateX(-50%)",
        display: "flex",
        alignItems: "center",
        gap: 14 * unit,
        padding: `${12 * unit}px ${26 * unit}px`,
        borderRadius: 999,
        background: "rgba(13, 17, 20, 0.78)",
        border: `${2 * unit}px solid rgba(243, 236, 220, 0.16)`,
        fontFamily: "Cascadia Mono, monospace",
        fontWeight: 700,
        fontSize: 30 * unit,
        letterSpacing: 1.5 * unit,
        color: "#f3ecdc",
        opacity,
      }}
    >
      {live && (
        <span style={{ width: 14 * unit, height: 14 * unit, borderRadius: 999, background: "#9ece6a", opacity: beat }} />
      )}
      {text}
    </div>
  );
  const edge = width * (1 - split.share);
  return (
    <AbsoluteFill>
      {chip("OMACHARTS · LIVE", edge / 2, true)}
      {chip("AGENT · TERMINAL", edge + (width - edge) / 2, false)}
    </AbsoluteFill>
  );
};

// The chord just pressed, as keycaps under the picture: how a keyboard-first
// app shows the viewer which keys did that.
const KeyBadge = ({ badge, t, height }: { badge: Badge; t: number; height: number }) => {
  const opacity = interpolate(t, [badge.start, badge.start + BADGE_FADE, badge.end - BADGE_FADE * 2, badge.end], [0, 1, 1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  const rise = interpolate(t, [badge.start, badge.start + BADGE_FADE * 2], [24, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: ease,
  });
  const unit = height / 2160;
  return (
    <AbsoluteFill style={{ justifyContent: "flex-end", alignItems: "center", paddingBottom: 150 * unit, opacity }}>
      <div
        style={{
          display: "flex",
          gap: 22 * unit,
          alignItems: "center",
          padding: `${26 * unit}px ${34 * unit}px`,
          borderRadius: 28 * unit,
          background: "rgba(13, 17, 20, 0.86)",
          boxShadow: `0 ${18 * unit}px ${48 * unit}px rgba(0, 0, 0, 0.45)`,
          border: `${2 * unit}px solid rgba(243, 236, 220, 0.14)`,
          transform: `translateY(${rise * unit}px)`,
        }}
      >
        {badge.keys.map((key, i) => (
          <div
            key={i}
            style={{
              fontFamily: "Cascadia Mono, monospace",
              fontWeight: 700,
              fontSize: 64 * unit,
              lineHeight: 1,
              color: "#f3ecdc",
              minWidth: 64 * unit,
              textAlign: "center",
              padding: `${18 * unit}px ${26 * unit}px`,
              borderRadius: 16 * unit,
              background: "rgba(243, 236, 220, 0.08)",
              border: `${2 * unit}px solid rgba(243, 236, 220, 0.22)`,
              borderBottomWidth: 6 * unit,
            }}
          >
            {key}
          </div>
        ))}
      </div>
    </AbsoluteFill>
  );
};
