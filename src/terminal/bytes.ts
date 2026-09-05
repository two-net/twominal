export const MAX_INPUT_CHUNK_BYTES = 64 * 1024;

const encoder = new TextEncoder();

export function encodeTerminalData(data: string): Uint8Array {
  return encoder.encode(data);
}

export function encodeBinaryData(data: string): Uint8Array {
  const bytes = new Uint8Array(data.length);

  for (let index = 0; index < data.length; index += 1) {
    bytes[index] = data.charCodeAt(index) & 0xff;
  }

  return bytes;
}

export function concatBytes(left: Uint8Array, right: Uint8Array): Uint8Array {
  const combined = new Uint8Array(left.byteLength + right.byteLength);
  combined.set(left);
  combined.set(right, left.byteLength);
  return combined;
}

export function chunkBytes(
  data: Uint8Array,
  maximumBytes = MAX_INPUT_CHUNK_BYTES,
): Uint8Array[] {
  if (maximumBytes < 1) {
    throw new RangeError("maximumBytes must be positive");
  }

  const chunks: Uint8Array[] = [];
  for (let offset = 0; offset < data.byteLength; offset += maximumBytes) {
    chunks.push(data.slice(offset, offset + maximumBytes));
  }
  return chunks;
}
