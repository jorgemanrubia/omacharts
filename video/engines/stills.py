# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "opencv-python-headless"]
# ///
"""Make a video's picture from stills of the app: a camera over them, and
crossfades from one look to the next.

    stills.py PLAN.json OUT.mkv

The plan, written by bin/video from a script's Shot:

    {"width": 3840, "height": 2160, "fps": 60, "duration": 92.0,
     "looks": {"dawn": "/path/dawn.png", ...},           # every still the same size
     "fades": [{"t0": 0, "t1": 0, "look": "dawn"}, ...],  # in order; t0 == t1 is a cut;
                                                          # "sweep": true for a soft diagonal sweep
     "camera": [{"t": 0, "x": 0.41, "y": 0.37, "scale": 14}, ...]}

The camera's x and y are the centre of the view as fractions of the still,
and its scale how close it is: 1 is the whole still. Between keys the scale
moves evenly in its logarithm — a pull-back that feels the same speed from
one candle to the whole screen — eased in at the first key and out at the
last, smooth through the ones between. The centre follows the width of the
view, as a camera dollying back in a straight line does: the drift from a
candle to the middle of the screen comes as the view widens.

Each still is kept as a pyramid of halvings, and a frame is drawn from the
two levels either side of the scale it is shown at, blended: no shimmer from
sampling a 16K still down to 4K, and no step in sharpness as the camera
crosses from one level to the next.
"""

import json
import math
import subprocess
import sys
from collections import OrderedDict

import cv2
import numpy as np

cv2.setNumThreads(0)


class Pyramid:
    def __init__(self, path):
        image = cv2.imread(path, cv2.IMREAD_COLOR)
        if image is None:
            sys.exit(f"stills: cannot read {path}")
        self.levels = [image]
        while self.levels[-1].shape[1] > 1200:
            last = self.levels[-1]
            self.levels.append(cv2.resize(last, (last.shape[1] // 2, last.shape[0] // 2), interpolation=cv2.INTER_AREA))
        self.size = (image.shape[1], image.shape[0])


class Looks:
    """The stills, loaded as they are needed and a few kept: each is large."""

    def __init__(self, paths, keep=3):
        self.paths, self.keep, self.cache = paths, keep, OrderedDict()

    def __getitem__(self, name):
        if name in self.cache:
            self.cache.move_to_end(name)
            return self.cache[name]
        pyramid = Pyramid(self.paths[name])
        self.cache[name] = pyramid
        while len(self.cache) > self.keep:
            self.cache.popitem(last=False)
        return pyramid


def smooth(u):
    u = min(max(u, 0.0), 1.0)
    return u * u * (3 - 2 * u)


def slopes(ts, vs):
    """Hermite tangents: flat at both ends, monotone between (Fritsch-Carlson),
    flat at a key where the motion turns."""
    n = len(ts)
    d = [0.0] * n
    for i in range(1, n - 1):
        a = (vs[i] - vs[i - 1]) / max(ts[i] - ts[i - 1], 1e-9)
        b = (vs[i + 1] - vs[i]) / max(ts[i + 1] - ts[i], 1e-9)
        if a * b > 0:
            d[i] = 2 * a * b / (a + b)
    return d


def hermite(t, t0, t1, v0, v1, d0, d1):
    h = t1 - t0
    u = (t - t0) / h
    u2, u3 = u * u, u * u * u
    return (2 * u3 - 3 * u2 + 1) * v0 + (u3 - 2 * u2 + u) * h * d0 + (-2 * u3 + 3 * u2) * v1 + (u3 - u2) * h * d1


class Camera:
    def __init__(self, keys):
        self.keys = sorted(keys, key=lambda k: k["t"])
        self.ts = [k["t"] for k in self.keys]
        self.logs = [math.log(k["scale"]) for k in self.keys]
        self.d = slopes(self.ts, self.logs)

    def at(self, t):
        keys = self.keys
        if t <= self.ts[0]:
            k = keys[0]
            return k["x"], k["y"], k["scale"]
        if t >= self.ts[-1]:
            k = keys[-1]
            return k["x"], k["y"], k["scale"]
        i = max(j for j in range(len(keys) - 1) if self.ts[j] <= t)
        a, b = keys[i], keys[i + 1]
        log = hermite(t, self.ts[i], self.ts[i + 1], self.logs[i], self.logs[i + 1], self.d[i], self.d[i + 1])
        scale = math.exp(log)
        w0, w1, w = 1 / a["scale"], 1 / b["scale"], 1 / scale
        if abs(w1 - w0) > 1e-6 * max(w0, w1):
            # A straight dolly: the centre moves with the width of the view.
            lam = min(max((w - w0) / (w1 - w0), 0.0), 1.0)
        else:
            lam = smooth((t - self.ts[i]) / (self.ts[i + 1] - self.ts[i]))
        return a["x"] + lam * (b["x"] - a["x"]), a["y"] + lam * (b["y"] - a["y"]), scale


def draw(pyramid, cx, cy, scale, out_w, out_h):
    """The view of one still: centre (cx, cy) as fractions, at `scale`."""
    sw, sh = pyramid.size
    # Scale 1 is the largest view of the frame's shape the still holds.
    base = min(sw, sh * out_w / out_h)
    # Output pixels per pixel of the full-size still.
    m = out_w * scale / base
    # The view kept inside the still: past its edge there is nothing drawn.
    half_w, half_h = 0.5 * out_w / m / sw, 0.5 * out_h / m / sh
    cx = min(max(cx, half_w), 1 - half_w) if half_w < 0.5 else 0.5
    cy = min(max(cy, half_h), 1 - half_h) if half_h < 0.5 else 0.5
    f = math.log2(1 / m) if m < 1 else 0.0
    level = min(int(math.floor(f)), len(pyramid.levels) - 1)
    frac = f - level if level + 1 < len(pyramid.levels) else 0.0

    def view(lv):
        img = pyramid.levels[lv]
        k = m * (2 ** lv)  # output pixels per pixel of this level
        lw, lh = img.shape[1], img.shape[0]
        # Pixel centres: the still's point (cx, cy) lands on the frame's centre.
        px, py = cx * lw - 0.5, cy * lh - 0.5
        matrix = np.array([[k, 0, out_w / 2 - 0.5 - k * px], [0, k, out_h / 2 - 0.5 - k * py]], dtype=np.float64)
        interp = cv2.INTER_CUBIC if k > 1.05 else cv2.INTER_LINEAR
        return cv2.warpAffine(img, matrix, (out_w, out_h), flags=interp, borderMode=cv2.BORDER_REPLICATE)

    a = view(level)
    if frac > 0.002:
        b = view(level + 1)
        return cv2.addWeighted(a, 1 - frac, b, frac, 0)
    return a


SWEEP_EDGE = 0.12  # the sweep's soft edge, as a share of its travel: wider, and most of the frame is a grey blend


def sweep_mask(w, h, alpha, origin="top-left"):
    """How much of the new look shows at each pixel: a soft edge travelling
    across the frame from a corner. A light change between a dark theme and
    a light one, dissolved evenly, passes through a flat, washed-out grey."""
    ramps = sweep_mask.__dict__.setdefault("ramps", {})
    if origin not in ramps:
        x = np.linspace(0, 1, w, dtype=np.float32)[None, :]
        y = np.linspace(0, 1, h, dtype=np.float32)[:, None]
        if "right" in origin:
            x = 1 - x
        if "bottom" in origin:
            y = 1 - y
        ramps[origin] = 0.6 * x + 0.4 * y
    u = np.clip((alpha * (1 + SWEEP_EDGE) - ramps[origin]) / SWEEP_EDGE, 0, 1)
    return (u * u * (3 - 2 * u))[:, :, None]


# Blending in linear light: the middle of a dark-to-light edge is bright,
# as light is, rather than the muddy grey a blend of gamma-coded values is.
TO_LINEAR = ((np.arange(256, dtype=np.float32) / 255) ** 2.2)
FROM_LINEAR = np.clip(np.round((np.linspace(0, 1, 4096, dtype=np.float32) ** (1 / 2.2)) * 255), 0, 255).astype(np.uint8)


def blend_linear(old, new, mask):
    mixed = TO_LINEAR[old] * (1 - mask) + TO_LINEAR[new] * mask
    return FROM_LINEAR[np.clip((mixed * 4095 + 0.5).astype(np.int32), 0, 4095)]


def main():
    plan = json.load(open(sys.argv[1]))
    out = sys.argv[2]
    w, h, fps = plan["width"], plan["height"], plan["fps"]
    frames = int(round(plan["duration"] * fps))
    looks = Looks(plan["looks"])
    camera = Camera(plan["camera"])
    fades = plan["fades"]
    encoder = subprocess.Popen(
        ["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgr24", "-s", f"{w}x{h}", "-r", str(fps), "-i", "-",
         "-vaapi_device", "/dev/dri/renderD128", "-vf", "format=nv12,hwupload", "-c:v", "hevc_vaapi", "-qp", "12", out],
        stdin=subprocess.PIPE)
    for n in range(frames):
        t = n / fps
        # The look on screen, and the one fading in over it.
        current, coming, alpha, sweep = None, None, 0.0, False
        for fade in fades:
            if t >= fade["t1"]:
                current = fade["look"]
            elif t >= fade["t0"]:
                sweep = fade.get("sweep", False)
                origin = fade.get("from", "top-left")
                u = (t - fade["t0"]) / (fade["t1"] - fade["t0"])
                coming, alpha = fade["look"], (u if sweep else smooth(u))
                break
            else:
                break
        cx, cy, scale = camera.at(t)
        frame = draw(looks[current], cx, cy, scale, w, h) if current else np.zeros((h, w, 3), np.uint8)
        if coming and sweep:
            frame = blend_linear(frame, draw(looks[coming], cx, cy, scale, w, h), sweep_mask(w, h, alpha, origin))
        elif coming:
            frame = cv2.addWeighted(frame, 1 - alpha, draw(looks[coming], cx, cy, scale, w, h), alpha, 0)
        encoder.stdin.write(frame.tobytes())
        if n % (fps * 5) == 0:
            print(f"stills: {t:5.1f}s of {plan['duration']:.1f}s", flush=True)
    encoder.stdin.close()
    if encoder.wait():
        sys.exit("stills: the encoder failed")


if __name__ == "__main__":
    main()
