import { Composition } from "remotion";
import { Polish, PolishProps } from "./Polish";
import { Trailer, TrailerProps } from "./Trailer";

// The compositions, sized by the plan bin/video hands each: the recording's
// own size, rate and length. Nothing here is decided per video.
export const Root = () => (
  <>
    <Composition
      id="Polish"
      component={Polish}
      width={3840}
      height={2160}
      fps={60}
      durationInFrames={60}
      defaultProps={
        { src: "recording.mp4", width: 3840, height: 2160, fps: 60, frames: 60, zooms: [], badges: [], splits: [] } as PolishProps
      }
      calculateMetadata={({ props }) => ({
        width: props.width,
        height: props.height,
        fps: props.fps,
        durationInFrames: props.frames,
      })}
    />
    <Composition
      id="Trailer"
      component={Trailer}
      width={3840}
      height={2160}
      fps={60}
      durationInFrames={60}
      defaultProps={
        {
          width: 3840,
          height: 2160,
          fps: 60,
          frames: 60,
          src: "recording.mp4",
          hits: [],
          scenes: [],
          cards: [],
          cover: "cover.png",
          ribbon: "bg-ribbon.jpg",
          mark: "icon.svg",
        } as TrailerProps
      }
      calculateMetadata={({ props }) => ({
        width: props.width,
        height: props.height,
        fps: props.fps,
        durationInFrames: props.frames,
      })}
    />
  </>
);
