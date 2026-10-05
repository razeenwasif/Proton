// What ui/lab3d.js uses of three.js — described here so the type checker
// reads this rather than two megabytes of library. Not the library's own
// types: those are a separate package, many times the size of the code.

export type ColorLike = string | number | Color;

export class Color {
  constructor(value?: ColorLike);
}

export class Vector3 {
  constructor(x?: number, y?: number, z?: number);
  x: number;
  y: number;
  z: number;
  set(x: number, y: number, z: number): this;
  copy(v: Vector3): this;
  clone(): Vector3;
  add(v: Vector3): this;
  sub(v: Vector3): this;
  multiplyScalar(s: number): this;
  length(): number;
  normalize(): this;
}

export class Quaternion {
  setFromUnitVectors(from: Vector3, to: Vector3): this;
}

export class Matrix4 {
  compose(position: Vector3, quaternion: Quaternion, scale: Vector3): this;
}

export class Object3D {
  position: Vector3;
  quaternion: Quaternion;
  scale: Vector3;
  visible: boolean;
  add(...objects: Object3D[]): this;
}

export class Group extends Object3D {}
export class Scene extends Object3D {}

export class BufferAttribute {}
export class Float32BufferAttribute extends BufferAttribute {
  constructor(array: ArrayLike<number>, itemSize: number);
}

export class BufferGeometry {
  setAttribute(name: string, attribute: BufferAttribute): this;
  setIndex(index: number[]): this;
  computeVertexNormals(): void;
  dispose(): void;
}
export class SphereGeometry extends BufferGeometry {
  constructor(radius?: number, widthSegments?: number, heightSegments?: number);
}
export class CylinderGeometry extends BufferGeometry {
  constructor(radiusTop?: number, radiusBottom?: number, height?: number, radialSegments?: number, heightSegments?: number, openEnded?: boolean);
}
export class ConeGeometry extends BufferGeometry {
  constructor(radius?: number, height?: number, radialSegments?: number);
}

export class Material {
  dispose(): void;
}
export class MeshStandardMaterial extends Material {
  constructor(parameters?: { color?: ColorLike; roughness?: number; metalness?: number; transparent?: boolean; opacity?: number; side?: number });
}
export class LineBasicMaterial extends Material {
  constructor(parameters?: { color?: ColorLike });
}
export const DoubleSide: number;

export class Mesh extends Object3D {
  constructor(geometry: BufferGeometry, material: Material);
}
export class InstancedMesh extends Mesh {
  constructor(geometry: BufferGeometry, material: Material, count: number);
  instanceMatrix: { needsUpdate: boolean };
  setMatrixAt(index: number, matrix: Matrix4): void;
  dispose(): this;
}
export class Line extends Object3D {
  constructor(geometry: BufferGeometry, material: Material);
}

export class Camera extends Object3D {}
export class PerspectiveCamera extends Camera {
  constructor(fov: number, aspect: number, near: number, far: number);
  aspect: number;
  updateProjectionMatrix(): void;
}

export class Light extends Object3D {}
export class HemisphereLight extends Light {
  constructor(skyColor?: ColorLike, groundColor?: ColorLike, intensity?: number);
}
export class DirectionalLight extends Light {
  constructor(color?: ColorLike, intensity?: number);
}

export class WebGLRenderer {
  constructor(parameters?: { canvas?: HTMLCanvasElement; antialias?: boolean; alpha?: boolean; powerPreference?: string });
  setClearColor(color: ColorLike, alpha?: number): void;
  setPixelRatio(ratio: number): void;
  setSize(width: number, height: number, updateStyle?: boolean): void;
  render(scene: Object3D, camera: Camera): void;
  dispose(): void;
}
