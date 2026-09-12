import type {WireCommand, WireEvent} from './protocol';

/** Typed facade over legacy demo fixtures; implementation is not yet checked. */
export function createPreview(deliver: (event: WireEvent) => void, latency: Map<string, number>): {
  send(command: WireCommand): Promise<WireEvent[]>;
  readGeometry(key: string): Promise<ArrayBuffer>;
};
