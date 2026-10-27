import type {WireCommand, WireEvent} from './protocol';

/** Typed facade over legacy demo fixtures; implementation is not yet checked. */
export function createPreview(deliver: (event: WireEvent) => void): {
  send(command: WireCommand): Promise<WireEvent[]>;
  readLab(key: string): Promise<ArrayBuffer>;
};
