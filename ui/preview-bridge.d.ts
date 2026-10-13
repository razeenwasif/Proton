import type {WireCommand, WireEvent} from './protocol';

/** Typed facade over legacy demo fixtures; implementation is not yet checked. */
export function createPreview(deliver: (event: WireEvent) => void, latency: Map<string, number>): {
  send(command: WireCommand): Promise<WireEvent[]>;
  readGeometry(key: string): Promise<ArrayBuffer>;
  readLab(key: string): Promise<ArrayBuffer>;
  readGame(key: string): Promise<ArrayBuffer>;
  readRaster(generation: number, layer: number, tiles: number[]): Promise<ArrayBuffer>;
};
