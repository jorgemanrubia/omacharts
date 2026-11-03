import { Composition } from "remotion";
import { Polish, PolishProps } from "./Polish";

// One composition, sized by the plan bin/video hands it: the recording's own
// size, rate and length. Nothing here is decided per video.
export const Root = () => (
  <Composition
    id="Polish"
    component={Polish}
    width={3840}
    height={2160}
    fps={60}
    durationInFrames={60}
    defaultProps={{ src: "recording.mp4", width: 3840, height: 2160, fps: 60, frames: 60, zooms: [], badges: [], splits: [] } as PolishProps}
    calculateMetadata={({ props }) => ({
      width: props.width,
      height: props.height,
      fps: props.fps,
      durationInFrames: props.frames,
    })}
  />
);
