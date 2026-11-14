import { CSSProperties, useEffect, useState } from "react";
import {
  AbsoluteFill,
  Freeze,
  Img,
  OffthreadVideo,
  Sequence,
  cancelRender,
  continueRender,
  delayRender,
  staticFile,
  useCurrentFrame,
} from "remotion";

// The trailer: one take cut to a drum. bin/video reads the drum's hits and
// the take's events and hands this file the whole plan in frames, so it only
// draws. Sizes are for a picture 2160 high and scale with it.
//
// Nothing here scales the picture over time. A punch-in is a hard cut to a
// fixed crop and a hard cut back; the only motion of the picture is the few
// pixels of a thump.

export type Hit = { phrase: number; hit: number | "echo"; frame: number };

/** Video frames [from, to) show the crop at (x, y), w wide, as fractions of the frame. */
export type Punch = { from: number; to: number; x: number; y: number; w: number };

export type TrailerBadge = { frame: number; keys: string[] };

/** The head arrives at (x, y) on `frame`, beside a row w by h, all fractions of the frame. */
export type Arrow = { frame: number; x: number; y: number; w?: number; h?: number };

export type Scene = {
  number: number;
  start: number;
  end: number;
  at: number;
  rate: number;
  punch: Punch[];
  badges: TrailerBadge[];
  fills: number[];
  arrow: Arrow | null;
  slats: boolean;
  tag: boolean;
  veil?: Veil | null;
};

/** Where a scene's order toasts fall, as fractions of the frame: covered by a capsule. */
export type Veil = { x: number; y: number; w: number; h: number };

export type Card = {
  number: number;
  pa: number;
  echo: number;
  letgo: number;
  kind: "cover" | "text" | "end";
  headline: string;
  second: string;
  sub?: string;
  small?: string;
  still: number | null;
};

export type TrailerProps = {
  width: number;
  height: number;
  fps: number;
  frames: number;
  src: string;
  hits: Hit[];
  scenes: Scene[];
  cards: Card[];
  cover: string;
  ribbon: string;
  mark: string;
};

const PAPER = "#f3ecdc";
const INK = "#0d1114";
const GOLD = "#e2c285"; // the cover's label

const FLASH = 2; // frames of white on a PA
const RULE_PRINT = 6;
const THUMP = 10; // frames the headline takes to settle
const FILL_THUMP = 8;
const LIFT = 10; // frames for a card's grade to lift off the footage
const SHUTTER = 8;
const BADGE_HOLD = 24;
const ARROW_DRAW = 8;
const FADE_OUT = 40;

// The settle of a thing that has just landed: `amp` pixels low at first,
// a damped oscillation at 9 Hz with a 60 ms decay, then still.
const thump = (since: number, fps: number, amp: number, length: number) => {
  if (since < 0 || since >= length) return 0;
  const t = since / fps;
  return amp * Math.exp(-t / 0.06) * Math.cos(2 * Math.PI * 9 * t);
};

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

// The card's grade as one colour matrix: a duotone from the ink in the
// shadows towards the gold in the highlights, held down so that white lands
// at 22% luminance: a dark, warm ghost of the picture. `s` blends it with the
// untouched picture, so the lift out of a card is linear.
const LUMA = [0.2126, 0.7152, 0.0722];
const rgb = (c: string) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16) / 255);
const luma = (c: number[]) => c.reduce((sum, v, i) => sum + v * LUMA[i], 0);
const CARD_LUMINANCE = 0.22;
const gradeMatrix = (s: number) => {
  const ink = rgb(INK);
  const gold = rgb(GOLD);
  const reach = (CARD_LUMINANCE - luma(ink)) / (luma(gold) - luma(ink));
  const rows: number[][] = [];
  for (let c = 0; c < 3; c++) {
    const span = reach * (gold[c] - ink[c]);
    const tone = [span * LUMA[0], span * LUMA[1], span * LUMA[2], 0, ink[c]];
    const identity = [0, 0, 0, 0, 0];
    identity[c] = 1;
    rows.push(tone.map((v, i) => s * v + (1 - s) * identity[i]));
  }
  rows.push([0, 0, 0, 1, 0]);
  return rows.map((r) => r.map((v) => v.toFixed(5)).join(" ")).join(" ");
};

const useFonts = () => {
  const [handle] = useState(() => delayRender("Trailer fonts"));
  useEffect(() => {
    const faces = [
      new FontFace("Archivo", `url(${staticFile("archivo-800.woff2")})`, { weight: "800" }),
      new FontFace("Cascadia Mono", `url(${staticFile("cascadia-mono-700.woff2")})`, { weight: "700" }),
    ];
    Promise.all(faces.map((f) => f.load()))
      .then((loaded) => {
        loaded.forEach((f) => document.fonts.add(f));
        continueRender(handle);
      })
      .catch((err) => cancelRender(err));
  }, [handle]);
};

export const Trailer = (props: TrailerProps) => {
  useFonts();
  const f = useCurrentFrame();
  const { width: W, height: H, fps, frames, src, hits, scenes, cards } = props;
  const unit = H / 2160;

  const scene = scenes.find((s) => f >= s.start && f < s.end);
  const card = cards.find((c) => f >= c.pa && f < (c.kind === "end" ? frames : c.letgo + LIFT));

  // How much of a text card's grade is on the picture: all of it while the
  // card holds, lifting over LIFT frames from its letgo.
  let grade = 0;
  if (card && card.kind === "text") grade = f < card.letgo ? 1 : 1 - (f - card.letgo) / LIFT;

  const fill = scene ? Math.max(0, ...scene.fills.map((x) => thump(f - x, fps, 5 * unit, FILL_THUMP))) : 0;
  const fillNow = scene?.fills.some((x) => x === f) ?? false;

  const nextScene = (c: Card) => scenes.find((s) => s.number === c.number + 1);

  return (
    <AbsoluteFill style={{ backgroundColor: "black", overflow: "hidden" }}>
      <svg width={0} height={0} style={{ position: "absolute" }}>
        <filter id="trailer-grade" colorInterpolationFilters="sRGB">
          <feColorMatrix type="matrix" values={gradeMatrix(clamp01(grade))} />
        </filter>
      </svg>

      <AbsoluteFill style={{ transform: `translateY(${fill}px)`, filter: grade > 0 ? "url(#trailer-grade)" : undefined }}>
        {scenes.map((s) => (
          <Sequence key={s.number} from={s.start} durationInFrames={s.end - s.start} layout="none">
            <Footage scene={s} src={src} fps={fps} start={s.start} />
          </Sequence>
        ))}
        {cards
          .filter((c) => c.kind === "text" && c.still !== null)
          .map((c) => {
            const next = nextScene(c);
            const punch = next?.punch.find((p) => next.start >= p.from && next.start < p.to);
            return (
              <Sequence key={c.number} from={c.pa} durationInFrames={c.letgo - c.pa} layout="none">
                <Freeze frame={0}>
                  <Picture src={src} trimBefore={(c.still as number) * fps} rate={1} punch={punch} veil={next?.veil} />
                </Freeze>
              </Sequence>
            );
          })}
        {scene?.slats && <Slats hits={hits} f={f} W={W} unit={unit} />}
        {scene?.arrow && f >= scene.arrow.frame - ARROW_DRAW && <ArrowMark arrow={scene.arrow} f={f} W={W} H={H} unit={unit} />}
      </AbsoluteFill>

      {card?.kind === "text" && <TextCard card={card} f={f} W={W} H={H} fps={fps} unit={unit} />}
      {card?.kind === "cover" && <CoverCard card={card} f={f} fps={fps} unit={unit} W={W} H={H} cover={props.cover} />}
      {card?.kind === "end" && <EndCard card={card} f={f} fps={fps} unit={unit} W={W} H={H} ribbon={props.ribbon} mark={props.mark} />}

      {(() => {
        // The meter rides the footage of each phrase and fires its last mark
        // on the PA, under the flash.
        const flashing = cards.find((c) => f >= c.pa && f < c.pa + FLASH);
        const phrase = flashing ? flashing.number : scene && !card ? scene.number : null;
        const ridden = scenes.find((s) => s.number === phrase);
        const punched = ridden?.punch.some((p) => f >= p.from && f < p.to);
        const veil = ridden?.veil && !punched && !flashing ? ridden.veil : null;
        return phrase !== null && <Meter hits={hits} phrase={phrase} f={f} fill={fillNow} unit={unit} veil={veil} W={W} H={H} />;
      })()}

      {scene?.badges
        .filter((b) => f >= b.frame && f < b.frame + BADGE_HOLD)
        .map((b) => (
          <KeyCaps key={b.frame} keys={b.keys} unit={unit} />
        ))}

      {scene?.tag && (
        <div
          style={{
            position: "absolute",
            right: 80 * unit,
            bottom: 52 * unit,
            fontFamily: "Cascadia Mono, monospace",
            fontWeight: 700,
            fontSize: 26 * unit,
            letterSpacing: 2 * unit,
            color: PAPER,
            opacity: 0.45,
          }}
        >
          SIMULATED MARKET
        </div>
      )}

      {cards.some((c) => f >= c.pa && f < c.pa + FLASH) && <AbsoluteFill style={{ background: "white", opacity: 0.14 }} />}
      {f >= frames - FADE_OUT && <AbsoluteFill style={{ background: "black", opacity: (f - (frames - FADE_OUT) + 1) / FADE_OUT }} />}
    </AbsoluteFill>
  );
};

// One scene's stretch of the take: video frame `start` shows recording time
// `at`, and it runs at `rate` from there. A punch-in shows its crop full
// frame for as long as it holds.
const Footage = ({ scene, src, fps, start }: { scene: Scene; src: string; fps: number; start: number }) => {
  const f = useCurrentFrame() + start;
  const punch = scene.punch.find((p) => f >= p.from && f < p.to);
  return <Picture src={src} trimBefore={scene.at * fps} rate={scene.rate} punch={punch} veil={scene.veil} />;
};

const Picture = ({ src, trimBefore, rate, punch, veil }: { src: string; trimBefore: number; rate: number; punch?: Punch; veil?: Veil | null }) => {
  const k = punch ? 1 / punch.w : 1;
  return (
    <AbsoluteFill
      style={{
        transformOrigin: "0 0",
        transform: punch ? `scale(${k}) translate(${-punch.x * 100}%, ${-punch.y * 100}%)` : undefined,
      }}
    >
      <OffthreadVideo src={staticFile(src)} trimBefore={trimBefore} playbackRate={rate} muted style={{ width: "100%", height: "100%" }} />
      {veil && <Capsule veil={veil} />}
    </AbsoluteFill>
  );
};

// The app's order toasts sit at the bottom of the window, over the volume
// bars, under nearly every frame. A dark glass capsule lies over the place
// they take in each scene, part of the picture (a punch-in carries it with
// the crop), and the drum's meter rides in it.
const Capsule = ({ veil }: { veil: Veil }) => (
  <div
    style={{
      position: "absolute",
      left: `${veil.x * 100}%`,
      top: `${veil.y * 100}%`,
      width: `${veil.w * 100}%`,
      height: `${veil.h * 100}%`,
      borderRadius: 9999,
      background: "rgba(14, 16, 22, 0.97)",
      backdropFilter: "blur(14px) saturate(0.6)",
      border: "2px solid rgba(243, 236, 220, 0.09)",
      boxShadow: "0 10px 40px rgba(0, 0, 0, 0.45)",
    }}
  />
);

// Scene 1 opens on black and the chart comes in as ten slats from the right,
// one a hit, each in three frames behind a one-frame bright edge.
const Slats = ({ hits, f, W, unit }: { hits: Hit[]; f: number; W: number; unit: number }) => {
  const slat = W / 10;
  let left = W;
  let edge: number | null = null;
  for (const h of hits) {
    if (h.phrase !== 1 || typeof h.hit !== "number" || h.hit > 10 || f < h.frame) continue;
    const index = 10 - h.hit; // hit 1 is the rightmost slat
    const since = f - h.frame;
    const arrived = Math.min(1, (since + 1) / 3);
    const x = index * slat + (1 - arrived) * slat;
    left = Math.min(left, x);
    if (since === 0) edge = x;
  }
  return (
    <>
      <div style={{ position: "absolute", left: 0, top: 0, bottom: 0, width: left, background: "black" }} />
      {edge !== null && (
        <div style={{ position: "absolute", left: edge, top: 0, bottom: 0, width: 6 * unit, background: "white", opacity: 0.85 }} />
      )}
    </>
  );
};

// A gold arrow drawn in from the right toward a row, and the row ringed as
// it lands. Both hold to the end of the scene.
const ArrowMark = ({ arrow, f, W, H, unit }: { arrow: Arrow; f: number; W: number; H: number; unit: number }) => {
  const rw = (arrow.w ?? 0.3) * W;
  const rh = (arrow.h ?? 0.035) * H;
  const cx = arrow.x * W;
  const cy = arrow.y * H;
  const tip = { x: cx + rw / 2 + 28 * unit, y: cy };
  const tail = { x: Math.min(W - 60 * unit, tip.x + 0.16 * W), y: Math.max(60 * unit, cy - 0.12 * H) };
  const ctrl = { x: tip.x + 0.11 * W, y: cy };
  let length = 0;
  let prev = tail;
  for (let i = 1; i <= 64; i++) {
    const t = i / 64;
    const p = {
      x: (1 - t) ** 2 * tail.x + 2 * (1 - t) * t * ctrl.x + t * t * tip.x,
      y: (1 - t) ** 2 * tail.y + 2 * (1 - t) * t * ctrl.y + t * t * tip.y,
    };
    length += Math.hypot(p.x - prev.x, p.y - prev.y);
    prev = p;
  }
  const drawn = clamp01((f - (arrow.frame - ARROW_DRAW)) / ARROW_DRAW);
  const landed = f >= arrow.frame;
  const head = 44 * unit;
  const stroke = { stroke: GOLD, strokeWidth: 10 * unit, strokeLinecap: "round" as const, strokeLinejoin: "round" as const, fill: "none" };
  return (
    <svg
      width={W}
      height={H}
      style={{ position: "absolute", left: 0, top: 0, filter: `drop-shadow(0 ${4 * unit}px ${10 * unit}px rgba(0,0,0,0.55))` }}
    >
      <path
        d={`M ${tail.x} ${tail.y} Q ${ctrl.x} ${ctrl.y} ${tip.x} ${tip.y}`}
        {...stroke}
        strokeDasharray={length}
        strokeDashoffset={length * (1 - drawn)}
      />
      {landed && (
        <>
          <path
            d={`M ${tip.x + head * 0.82} ${tip.y - head * 0.57} L ${tip.x} ${tip.y} L ${tip.x + head * 0.82} ${tip.y + head * 0.57}`}
            {...stroke}
          />
          <rect x={cx - rw / 2} y={cy - rh / 2} width={rw} height={rh} rx={14 * unit} {...stroke} />
        </>
      )}
    </svg>
  );
};

// The drum made visible: ten marks and the PA's, lit as they sound.
const Meter = ({ hits, phrase, f, fill, unit, veil, W, H }: {
  hits: Hit[]; phrase: number; f: number; fill: boolean; unit: number; veil: Veil | null; W: number; H: number;
}) => {
  const mine = hits.filter((h) => h.phrase === phrase && typeof h.hit === "number") as { phrase: number; hit: number; frame: number }[];
  const lit = (n: number) => mine.find((h) => h.hit === n && f >= h.frame);
  let current = 0;
  for (let n = 1; n <= 11; n++) if (lit(n)) current = n;
  return (
    <div
      style={{
        position: "absolute",
        ...(veil
          ? { left: veil.x * W, width: veil.w * W, top: veil.y * H, height: veil.h * H }
          : { left: 0, right: 0, bottom: 60 * unit }),
        display: "flex",
        justifyContent: "center",
        alignItems: "center",
        gap: 14 * unit,
      }}
    >
      {Array.from({ length: 11 }, (_, i) => {
        const n = i + 1;
        const h = lit(n);
        let color = PAPER;
        let opacity = 0.18;
        if (h) {
          color = GOLD;
          opacity = f - h.frame < 2 ? 1 : 0.7;
        }
        if (fill && n === Math.max(1, current)) {
          color = "white";
          opacity = 1;
        }
        return (
          <div
            key={n}
            style={{
              width: (n === 11 ? 56 : 28) * unit,
              height: (n === 11 ? 10 : 6) * unit,
              borderRadius: 2 * unit,
              background: color,
              opacity,
            }}
          />
        );
      })}
    </div>
  );
};

// The chord just pressed, in the keycaps of the polish pass, on the hit and
// off again with no motion.
const KeyCaps = ({ keys, unit }: { keys: string[]; unit: number }) => (
  <AbsoluteFill style={{ justifyContent: "flex-end", alignItems: "center", paddingBottom: 200 * unit }}>
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
      }}
    >
      {keys.map((key, i) => (
        <div
          key={i}
          style={{
            fontFamily: "Cascadia Mono, monospace",
            fontWeight: 700,
            fontSize: 64 * unit,
            lineHeight: 1,
            color: PAPER,
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

type CardArgs = { card: Card; f: number; W: number; H: number; fps: number; unit: number };

// The second line, cut in on the echo with a one-frame gold flick under it.
const Echo = ({ text, f, echo, unit, style }: { text: string; f: number; echo: number; unit: number; style?: CSSProperties }) =>
  f < echo ? null : (
    <div style={{ position: "absolute", ...style }}>
      <div
        style={{
          fontFamily: "Cascadia Mono, monospace",
          fontWeight: 700,
          fontSize: 66 * unit,
          letterSpacing: 1 * unit,
          lineHeight: 1.2,
          color: PAPER,
          opacity: 0.88,
          textShadow: `0 ${4 * unit}px ${24 * unit}px rgba(0,0,0,0.6)`,
          whiteSpace: "nowrap",
        }}
      >
        {text}
      </div>
      {f === echo && <div style={{ marginTop: 10 * unit, height: 4 * unit, background: GOLD }} />}
    </div>
  );

const Rule = ({ since, unit }: { since: number; unit: number }) => (
  <div style={{ height: 10 * unit, width: `${clamp01((since + 1) / RULE_PRINT) * 100}%`, background: GOLD }} />
);

// A headline over the graded still of the next scene, the rule printing
// under it, the second line on the echo; at letgo a shutter closes on the
// rule and takes the words away.
const TextCard = ({ card, f, W, H, fps, unit }: CardArgs) => {
  const since = f - card.pa;
  const long = card.headline.length > 30;
  const ruleY = H * 0.58;
  const ruleMid = ruleY + 5 * unit;
  const shut = f >= card.letgo ? clamp01((f - card.letgo + 1) / SHUTTER) : 0;
  if (shut >= 1) return null;
  return (
    <AbsoluteFill style={{ clipPath: shut > 0 ? `inset(${shut * ruleMid}px 0 ${shut * (H - ruleMid)}px 0)` : undefined }}>
      <div style={{ position: "absolute", left: W * 0.12, bottom: H - ruleY - 10 * unit, maxWidth: W * 0.76 }}>
        <div
          style={{
            fontFamily: "Archivo, sans-serif",
            fontWeight: 800,
            fontSize: (long ? 120 : 200) * unit,
            lineHeight: long ? 1.12 : 1,
            letterSpacing: "-0.02em",
            color: PAPER,
            textShadow: `0 ${6 * unit}px ${40 * unit}px rgba(0,0,0,0.5)`,
            transform: `translateY(${thump(since, fps, 14 * unit, THUMP)}px)`,
            marginBottom: 44 * unit,
            display: "inline-block",
          }}
        >
          {card.headline}
        </div>
        <Rule since={since} unit={unit} />
      </div>
      <Echo text={card.second} f={f} echo={card.echo} unit={unit} style={{ left: W * 0.12, top: ruleY + 10 * unit + 52 * unit }} />
    </AbsoluteFill>
  );
};

// Card 1 is the cover as it is, landing with a thump, its line on the echo
// under the lockup; it dissolves into scene 2 over the lift.
const CoverCard = ({ card, f, fps, unit, W, H, cover }: CardArgs & { cover: string }) => {
  const since = f - card.pa;
  const opacity = f < card.letgo ? 1 : 1 - (f - card.letgo) / LIFT;
  return (
    <AbsoluteFill style={{ opacity, backgroundColor: "black" }}>
      <AbsoluteFill style={{ transform: `translateY(${thump(since, fps, 10 * unit, THUMP)}px)` }}>
        <Img src={staticFile(cover)} style={{ width: "100%", height: "100%" }} />
      </AbsoluteFill>
      <Echo text={card.second} f={f} echo={card.echo} unit={unit} style={{ left: W * 0.12, top: H * 0.74 }} />
    </AbsoluteFill>
  );
};

// The end: the mark and wordmark over the site's ribbon, the headline and
// its sub-line slammed in under the wordmark, the address and the small
// print on the echo. It holds to the last frame.
const EndCard = ({ card, f, fps, unit, W, H, ribbon, mark }: CardArgs & { ribbon: string; mark: string }) => {
  const since = f - card.pa;
  const drop = thump(since, fps, 14 * unit, THUMP);
  return (
    <AbsoluteFill style={{ backgroundColor: INK }}>
      <Img
        src={staticFile(ribbon)}
        style={{ width: "100%", height: "100%", objectFit: "cover", filter: "brightness(0.42) saturate(0.9)" }}
      />
      <AbsoluteFill
        style={{
          background: "radial-gradient(60% 60% at 50% 50%, rgba(13,17,20,0.9) 0%, rgba(13,17,20,0.75) 50%, rgba(13,17,20,0.35) 100%)",
        }}
      />
      <AbsoluteFill style={{ justifyContent: "center", alignItems: "center" }}>
        <div style={{ display: "flex", alignItems: "flex-start", gap: 84 * unit, marginTop: -60 * unit }}>
          <Img
            src={staticFile(mark)}
            style={{
              width: 400 * unit,
              height: 400 * unit,
              borderRadius: "22%",
              boxShadow: `0 ${28 * unit}px ${70 * unit}px rgba(0,0,0,0.5)`,
            }}
          />
          <div style={{ display: "flex", flexDirection: "column" }}>
            <div
              style={{
                fontFamily: "Archivo, sans-serif",
                fontWeight: 800,
                fontSize: 390 * unit,
                lineHeight: 0.86,
                letterSpacing: "-0.045em",
                margin: "-0.077em 0 -0.096em",
                color: PAPER,
                textShadow: "0 .02em .14em rgba(0,0,0,.6)",
              }}
            >
              Omacharts
            </div>
            <div style={{ marginTop: 120 * unit, transform: `translateY(${drop}px)` }}>
              <div
                style={{
                  fontFamily: "Archivo, sans-serif",
                  fontWeight: 800,
                  fontSize: 200 * unit,
                  lineHeight: 1,
                  letterSpacing: "-0.02em",
                  color: PAPER,
                }}
              >
                {card.headline}
              </div>
              {card.sub && (
                <div
                  style={{
                    marginTop: 34 * unit,
                    fontFamily: "Cascadia Mono, monospace",
                    fontWeight: 700,
                    fontSize: 60 * unit,
                    letterSpacing: 3 * unit,
                    color: GOLD,
                    whiteSpace: "nowrap",
                  }}
                >
                  {card.sub}
                </div>
              )}
            </div>
            <div style={{ marginTop: 44 * unit }}>
              <Rule since={since} unit={unit} />
            </div>
            <div style={{ position: "relative", marginTop: 48 * unit, height: 190 * unit }}>
              <Echo text={card.second} f={f} echo={card.echo} unit={unit} style={{ left: 0, top: 0 }} />
              {card.small && f >= card.echo && (
                <div
                  style={{
                    position: "absolute",
                    left: 0,
                    top: 104 * unit,
                    fontFamily: "Cascadia Mono, monospace",
                    fontWeight: 700,
                    fontSize: 44 * unit,
                    letterSpacing: 1 * unit,
                    color: PAPER,
                    opacity: 0.62,
                    whiteSpace: "nowrap",
                  }}
                >
                  {card.small}
                </div>
              )}
            </div>
          </div>
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  );
};
