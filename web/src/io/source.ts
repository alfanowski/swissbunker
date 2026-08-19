/**
 * A range-readable sequence of bytes.
 *
 * Two implementations exist for one reason: production reads multi-gigabyte `File` objects
 * that no test can fabricate, while tests need buffers that no production path should use.
 * One interface means everything above never learns which one it holds.
 */
export interface ByteSource {
  readonly size: number;
  readonly name: string;
  read(offset: number, length: number): Promise<Uint8Array>;
  /** Returns null when this source cannot serve a blocking read. */
  readSync(offset: number, length: number): Uint8Array | null;
}

/** Clamp a requested range to what the source actually holds. */
function clamp(offset: number, length: number, size: number): [number, number] {
  const start = Math.max(0, Math.min(offset, size));
  const end = Math.max(start, Math.min(offset + length, size));
  return [start, end];
}

export class BufferSource implements ByteSource {
  constructor(private readonly buf: Uint8Array, readonly name: string) {}

  get size(): number { return this.buf.length; }

  async read(offset: number, length: number): Promise<Uint8Array> {
    return this.readSync(offset, length)!;
  }

  readSync(offset: number, length: number): Uint8Array {
    const [start, end] = clamp(offset, length, this.size);
    return this.buf.subarray(start, end);
  }
}

export class FileSource implements ByteSource {
  constructor(private readonly file: File) {}

  get size(): number { return this.file.size; }
  get name(): string { return this.file.name; }

  async read(offset: number, length: number): Promise<Uint8Array> {
    const [start, end] = clamp(offset, length, this.size);
    if (end <= start) { return new Uint8Array(0); }
    return new Uint8Array(await this.file.slice(start, end).arrayBuffer());
  }

  /**
   * Blocking read of a file range.
   *
   * SQLite's VFS demands synchronous reads; File.slice() is asynchronous; and a null origin
   * denies SharedArrayBuffer, so the usual worker + Atomics.wait bridge does not exist. What
   * remains is a synchronous XHR against a Blob URL of the slice — measured at 0.7 ms for
   * 4 KB on all four engines in Phase 0, and the reason the Portable runtime is possible.
   */
  readSync(offset: number, length: number): Uint8Array | null {
    const [start, end] = clamp(offset, length, this.size);
    if (end <= start) { return new Uint8Array(0); }

    const url = URL.createObjectURL(this.file.slice(start, end));
    try {
      const xhr = new XMLHttpRequest();
      xhr.open('GET', url, false);
      // responseType is forbidden on a synchronous XHR in a window context, so bytes come
      // back through x-user-defined: an encoding that maps every byte to one character in
      // the range U+F700..U+F7FF, leaving the original byte in the low 8 bits. Masking with
      // 0xff recovers it exactly, including 0x00 and everything above 0x7F — which a UTF-8
      // decode would have destroyed, and an index is full of such bytes.
      xhr.overrideMimeType('text/plain; charset=x-user-defined');
      xhr.send(null);
      // A Blob URL answers 200; some engines report 0 for local reads. Anything else is a
      // genuine failure and must not be mistaken for empty data.
      if (xhr.status !== 200 && xhr.status !== 0) { return null; }

      const text = xhr.responseText;
      const out = new Uint8Array(text.length);
      for (let i = 0; i < text.length; i++) { out[i] = text.charCodeAt(i) & 0xff; }
      return out;
    } catch {
      return null;
    } finally {
      URL.revokeObjectURL(url);
    }
  }
}
