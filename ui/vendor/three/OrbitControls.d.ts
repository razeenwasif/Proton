// What ui/lab3d.js uses of three.js's OrbitControls; see three.module.d.ts.
import type { Camera, Vector3 } from "./three.module.js";

export class OrbitControls {
  constructor(camera: Camera, domElement: HTMLElement);
  target: Vector3;
  minDistance: number;
  maxDistance: number;
  update(): boolean;
  saveState(): void;
  reset(): void;
  dispose(): void;
  addEventListener(type: "change" | "start" | "end", listener: () => void): void;
}
