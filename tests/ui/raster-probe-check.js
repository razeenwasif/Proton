// See raster-probe.html.
import {compileRaster, createRasterRuntime} from '/raster-runtime.js';
import {createRasterRender} from '/raster-render.js';

const result = {};
try {
  const rt = await createRasterRuntime();
  rt.open({width: 256, height: 256, preset: 'paint', palette: [], generation: 1,
    layers: [{id: 1, name: 'a', opacity: 255, blend: 'normal', hidden: false, locked: false, alphaLock: false}]});
  rt.apply({op: 'fillRect', layer: 1, rect: [64, 64, 64, 64], color: [255, 0, 0, 255]});
  const rects = rt.dirty();
  result.rects = rects.length / 4;
  const gpu = createRasterRender(document.getElementById('c'));
  result.gl = Boolean(gpu);
  if (gpu) {
    gpu.resize(256, 256);
    gpu.upload(rects, rt.composite());
    gpu.draw({zoom: 1, x: 0, y: 0}, 256, 256, 1, {ground: [0, 0, 0], light: [1, 1, 1], dark: [0.5, 0.5, 0.5], pixel: true});
    result.inside = gpu.probe(96, 96);
    result.outside = gpu.probe(10, 10);
  }
  // A memory of its own, grown to the cap and refused past it.
  const {exports} = await WebAssembly.instantiate(await compileRaster(), {});
  const now = exports.memory.buffer.byteLength / 65536;
  exports.memory.grow(16384 - now);
  result.grew = exports.memory.buffer.byteLength === 1024 * 1024 * 1024;
  try {
    exports.memory.grow(1);
    result.beyond = 'grew';
  } catch (error) {
    result.beyond = error.name;
  }
} catch (error) {
  result.error = String(error);
}
window.result = result;
document.getElementById('out').textContent = JSON.stringify(result);
