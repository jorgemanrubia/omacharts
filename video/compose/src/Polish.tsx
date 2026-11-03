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

export type PolishProps = {
  src: string;
  width: number;
  height: number;
  fps: number;
  frames: number;
  zooms: Zoom[];
  badges: Badge[];
  splits?: Split[];
};

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

export const Polish = ({ src, width, height, zooms, badges, splits = [] }: PolishProps) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const t = frame / fps;

  const zoom = zooms.find((z) => t >= z.start && t <= z.end);
  const k = zoom ? amount(zoom, t) : 0;
  const scale = zoom ? 1 + (zoom.scale - 1) * k : 1;
  // Scaling about the point of interest keeps that point still and the frame
  // filled: nothing outside the recording ever shows at the edges.
  const origin = zoom ? `${zoom.x}px ${zoom.y}px` : "50% 50%";

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
          transform: `scale(${scale * (1 - 0.035 * move)})`,
          transformOrigin: origin,
          filter: move > 0.01 ? `blur(${(10 * move * unit).toFixed(2)}px) brightness(${1 + 0.12 * move})` : undefined,
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
