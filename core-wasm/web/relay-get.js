// Browser-owned byte transport for the Rust signed public pull. Reads go to
// the build's relay origin (the page's own by default) without cookies.
import { relayUrl } from './relay-origin.js';
const hex = (value) => Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('');

export async function relayGet(path, signedRead) {
  if (!path.startsWith('/v1/families/') || path.includes('..') || path.includes('#')) {
    throw new Error('Invalid relay read path');
  }
  if (signedRead.length < 1 || signedRead.length > 2048) {
    throw new Error('Invalid signed relay read');
  }
  const response = await fetch(relayUrl(path), {
    credentials: 'omit',
    method: 'GET',
    headers: { Authorization: `Babytrack-Read ${hex(signedRead)}` },
    redirect: 'error',
    cache: 'no-store',
    signal: AbortSignal.timeout(15000),
  });
  if (!response.ok || !response.body) throw new Error(`Relay read failed: ${response.status}`);
  const reader = response.body.getReader();
  const chunks = [];
  let total = 0;
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      total += value.length;
      if (total > 4 * 1024 * 1024) throw new Error('Relay read exceeds page limit');
      chunks.push(value);
    }
  } catch (error) {
    await reader.cancel().catch(() => {});
    throw error;
  } finally {
    reader.releaseLock();
  }
  const result = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return result;
}
