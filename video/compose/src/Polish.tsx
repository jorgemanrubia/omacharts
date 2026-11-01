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

export type PolishProps = {
  src: string;
  width: number;
  height: number;
  fps: number;
  frames: number;
  zooms: Zoom[];
  badges: Badge[];
};

const EASE_IN = 0.5; // seconds to settle into a zoom
const EASE_OUT = 0.6; // and to pull back out of it
const BADGE_FADE = 0.15;

const ease = Easing.bezier(0.45, 0, 0.25, 1);

// How far into a zoom the picture is at time t: 0 at full frame, 1 settled.
const amount = (zoom: Zoom, t: number) =>
  interpolate(t, [zoom.start, zoom.start + EASE_IN, zoom.end - EASE_OUT, zoom.end], [0, 1, 1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: ease,
  });

export const Polish = ({ src, width, height, zooms, badges }: PolishProps) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const t = frame / fps;

  const zoom = zooms.find((z) => t >= z.start && t <= z.end);
  const k = zoom ? amount(zoom, t) : 0;
  const scale = zoom ? 1 + (zoom.scale - 1) * k : 1;
  // Scaling about the point of interest keeps that point still and the frame
  // filled: nothing outside the recording ever shows at the edges.
  const origin = zoom ? `${zoom.x}px ${zoom.y}px` : "50% 50%";

  return (
    <AbsoluteFill style={{ backgroundColor: "black" }}>
      <AbsoluteFill style={{ transform: `scale(${scale})`, transformOrigin: origin }}>
        <OffthreadVideo src={staticFile(src)} muted style={{ width, height }} />
      </AbsoluteFill>
      {badges
        .filter((b) => t >= b.start && t <= b.end)
        .map((b) => (
          <KeyBadge key={`${b.start}`} badge={b} t={t} height={height} />
        ))}
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
