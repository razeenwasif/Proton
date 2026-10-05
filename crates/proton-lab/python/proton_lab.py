"""Proton's Lab, from inside a run.

Copied into every run's folder beside its main.py, so `import proton_lab`
works however the run was written. What it writes stays in the run's folder
and is what the Lab pane draws:

    figure("spectrum")             # the current matplotlib figure, as figures/spectrum.png
    metric("peak_nm", 405.2, "nm") # into metrics.json
    scene = Scene("orbit")          # a 3D scene, as orbit.scene.json (see Scene)

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
    """Save a matplotlib figure — the current one unless given — into figures/."""
    import matplotlib

    matplotlib.use("Agg", force=False)
    import matplotlib.pyplot as plt

    fig = fig or plt.gcf()
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


class Scene:
    """A 3D scene for the Lab's viewer: points, lines, spheres and bonds,
    meshes and arrows, in frames for anything that moves. Saved as
    <name>.scene.json in the proton.scene/1 format."""

    def __init__(self, name="scene", title=None, units=None):
        self.name = name
        self.title = title or name
        self.units = units
        self.frames = [[]]

    def _add(self, item):
        self.frames[-1].append(item)
        return self

    def frame(self):
        """Start the next frame: what follows is drawn after what came before."""
        self.frames.append([])
        return self

    def points(self, xyz, color=None, size=None):
        return self._add({"kind": "points", "xyz": [list(map(float, p)) for p in xyz], "color": color, "size": size})

    def line(self, xyz, color=None, width=None):
        return self._add({"kind": "line", "xyz": [list(map(float, p)) for p in xyz], "color": color, "width": width})

    def sphere(self, center, radius=1.0, color=None, label=None):
        return self._add({"kind": "sphere", "center": list(map(float, center)), "radius": float(radius), "color": color, "label": label})

    def bond(self, a, b, radius=0.1, color=None):
        return self._add({"kind": "bond", "a": list(map(float, a)), "b": list(map(float, b)), "radius": float(radius), "color": color})

    def arrow(self, origin, vector, color=None):
        return self._add({"kind": "arrow", "origin": list(map(float, origin)), "vector": list(map(float, vector)), "color": color})

    def mesh(self, vertices, faces, color=None, opacity=None):
        return self._add({"kind": "mesh", "vertices": [list(map(float, v)) for v in vertices], "faces": [list(map(int, f)) for f in faces],
                          "color": color, "opacity": opacity})

    def save(self):
        frames = [f for f in self.frames if f] or [[]]
        doc = {"format": "proton.scene/1", "title": self.title, "units": self.units, "frames": frames}
        path = RUN / _name(self.name, "scene", ".scene.json")
        path.write_text(json.dumps(doc) + "\n")
        return path
