"""Proton's Lab, from inside a run.

Copied into every run's folder beside its main.py, so `import proton_lab`
works however the run was written. What it writes stays in the run's folder
and is what the Lab pane draws:

    figure(fig, "spectrum")        # a matplotlib figure (the current one if none), as figures/spectrum.png
    metric("peak_nm", 405.2, "nm") # into metrics.json
    scene = Scene("orbit")          # a 3D scene, as orbit.scene.json (see Scene)
    scene.spheres(centres, radii, frames=positions); scene.save()

Nothing here reaches the network or anything outside the run's folder; the
sandbox would refuse it if it did.
"""

import json
import math
import os
import pathlib

RUN = pathlib.Path(os.environ.get("PROTON_RUN_DIR") or pathlib.Path(__file__).resolve().parent)


def _name(name, default, suffix):
    stem = "".join(c if c.isalnum() or c in "-_." else "-" for c in str(name or default)).strip("-.") or default
    return stem if stem.endswith(suffix) else stem + suffix


def figure(name=None, fig=None, dpi=144):
    """Save a matplotlib figure — the current one unless given — into figures/.

    The figure and its name may come in either order — figure("spectrum"),
    figure(fig, "spectrum"), figure("spectrum", fig) — as agents are told
    one and write the other."""
    if hasattr(name, "savefig") and not hasattr(fig, "savefig"):
        name, fig = fig, name
    if fig is None:
        import matplotlib

        matplotlib.use("Agg", force=False)
        import matplotlib.pyplot as plt

        fig = plt.gcf()
    folder = RUN / "figures"
    folder.mkdir(exist_ok=True)
    taken = len(list(folder.glob("*.png")))
    path = folder / _name(name, f"figure-{taken + 1}", ".png")
    fig.savefig(path, dpi=dpi, bbox_inches="tight")
    return path


def metric(name, value, unit=None):
    """Record a number the run found: metrics.json, by name."""
    path = RUN / "metrics.json"
    try:
        metrics = json.loads(path.read_text())
    except (OSError, ValueError):
        metrics = {}
    if isinstance(value, float) and not math.isfinite(value):
        value = str(value)
    metrics[str(name)] = {"value": value, "unit": unit}
    path.write_text(json.dumps(metrics, indent=2) + "\n")
    return value


def _many(value):
    """Whether a value is one per item rather than one for all."""
    return hasattr(value, "__len__") and not isinstance(value, (str, bytes, dict))


def _each(value, i):
    """The i-th of a value given one per item, or the value given for all."""
    if _many(value) and len(value):
        return value[i] if i < len(value) else value[-1]
    return value


def _each_color(value, i):
    """The i-th colour of colours given one per item — names, or triples —
    or the one colour given for all, a name or a single (r, g, b)."""
    if _many(value) and len(value) and (isinstance(value[0], str) or _many(value[0])):
        return _each(value, i)
    return value


def _xyz(points):
    return [list(map(float, p)) for p in points]


class Scene:
    """A 3D scene for the Lab's viewer: points, lines, spheres and bonds,
    meshes and arrows. Saved as <name>.scene.json in the proton.scene/1
    format, as a list of frames, each the whole picture at one moment.

    Anything that moves is given `frames=`: its positions at each moment,
    time × N × 3 — `scene.spheres(centres, radii, frames=positions)`. What is
    given without them is in every frame. `frame()` starts the next frame by
    hand, for scenes drawn a moment at a time."""

    def __init__(self, name="scene", title=None, units=None):
        self.name = name
        self.title = title or name
        self.units = units
        self.frames = [[]]
        self.timed = []

    def _add(self, item):
        self.frames[-1].append(item)
        return self

    def _over(self, frames, make):
        """An item that moves: one copy of it a frame, made from that frame's positions."""
        self.timed.append([make(f) for f in frames])
        return self

    def frame(self):
        """Start the next frame. Each frame is the whole picture at one moment,
        drawn on its own; the viewer plays them in turn."""
        self.frames.append([])
        return self

    def points(self, xyz, color=None, size=None, frames=None):
        make = lambda p: {"kind": "points", "xyz": _xyz(p), "color": color, "size": size}
        return self._add(make(xyz)) if frames is None else self._over(frames, make)

    def line(self, xyz, color=None, width=None, frames=None):
        make = lambda p: {"kind": "line", "xyz": _xyz(p), "color": color, "width": width}
        return self._add(make(xyz)) if frames is None else self._over(frames, make)

    def sphere(self, center, radius=1.0, color=None, label=None, frames=None):
        make = lambda c: {"kind": "sphere", "center": list(map(float, c)), "radius": float(radius), "color": color, "label": label}
        return self._add(make(center)) if frames is None else self._over(frames, make)

    def spheres(self, centres, radii=1.0, color=None, labels=None, frames=None):
        """One sphere for each centre; `radii`, `color` and `labels` one for all or one each."""
        for i, centre in enumerate(centres):
            moving = None if frames is None else [f[i] for f in frames]
            self.sphere(centre, float(_each(radii, i)), _each_color(color, i), _each(labels, i), frames=moving)
        return self

    def bond(self, a, b, radius=0.1, color=None, frames=None):
        """A bond from a to b; `frames=` gives its ends at each moment, time × 2 × 3."""
        make = lambda ab: {"kind": "bond", "a": list(map(float, ab[0])), "b": list(map(float, ab[1])), "radius": float(radius), "color": color}
        return self._add(make((a, b))) if frames is None else self._over(frames, make)

    def bonds(self, pairs, radius=0.1, color=None, frames=None):
        """One bond for each pair of points, (a, b); `frames=` gives the pairs at each moment."""
        for i, (a, b) in enumerate(pairs):
            moving = None if frames is None else [f[i] for f in frames]
            self.bond(a, b, float(_each(radius, i)), _each_color(color, i), frames=moving)
        return self

    def arrow(self, origin, vector, color=None, frames=None):
        make = lambda o: {"kind": "arrow", "origin": list(map(float, o)), "vector": list(map(float, vector)), "color": color}
        return self._add(make(origin)) if frames is None else self._over(frames, make)

    def arrows(self, origins, vectors, color=None, frames=None):
        """One arrow for each origin and vector; `frames=` moves the origins."""
        for i, (origin, vector) in enumerate(zip(origins, vectors)):
            moving = None if frames is None else [f[i] for f in frames]
            self.arrow(origin, vector, _each_color(color, i), frames=moving)
        return self

    def mesh(self, vertices, faces, color=None, opacity=None, frames=None):
        make = lambda v: {"kind": "mesh", "vertices": _xyz(v), "faces": [list(map(int, f)) for f in faces], "color": color, "opacity": opacity}
        return self._add(make(vertices)) if frames is None else self._over(frames, make)

    def save(self, name=None):
        """Write the scene beside the run; `name` names its file, if given."""
        if name:
            self.name = name
        drawn = [f for f in self.frames if f] or [[]]
        count = max([len(drawn)] + [len(t) for t in self.timed])
        frames = []
        for at in range(count):
            # Drawn without frames=: the same in every frame, or frame by frame as frame() made them.
            frame = list(drawn[min(at, len(drawn) - 1)])
            for moving in self.timed:
                if moving:
                    frame.append(moving[min(at, len(moving) - 1)])
            frames.append(frame)
        doc = {"format": "proton.scene/1", "title": self.title, "units": self.units, "frames": frames}
        path = RUN / _name(self.name, "scene", ".scene.json")
        path.write_text(json.dumps(doc) + "\n")
        return path
