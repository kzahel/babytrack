import { relayUrl } from './relay-origin.js';
// Browser transport for an already staged, signed encrypted batch. The
// caller confirms acceptance through the signed public pull before clearing
// its durable outbox.
export async function relayPost(path, envelope) {
  if (!/^\/v1\/families\/[0-9a-f]{32}\/batches$/.test(path)) {
    throw new Error('Invalid relay batch path');
  }
  if (envelope.length < 1 || envelope.length > 258 * 1024) {
    throw new Error('Invalid encrypted batch length');
  }
  const response = await fetch(relayUrl(path), {
    credentials: 'omit',
    method: 'POST',
    headers: { 'Content-Type': 'application/cbor' },
    body: envelope,
    redirect: 'error',
    cache: 'no-store',
    signal: AbortSignal.timeout(15000),
  });
  if (!response.ok) throw new Error(`Relay batch upload failed: ${response.status}`);
}

export async function relayPostControl(path, candidate) {
  if (!/^\/v1\/families\/[0-9a-f]{32}\/control$/.test(path)) {
    throw new Error('Invalid relay control path');
  }
  if (candidate.length < 1 || candidate.length > 1024 * 1024) {
    throw new Error('Invalid control candidate length');
  }
  const response = await fetch(relayUrl(path), {
    credentials: 'omit',
    method: 'POST',
    headers: { 'Content-Type': 'application/cbor' },
    body: candidate,
    redirect: 'error',
    cache: 'no-store',
    signal: AbortSignal.timeout(15000),
  });
  if (!response.ok) throw new Error(`Relay control commit failed: ${response.status}`);
  const body = new Uint8Array(await response.arrayBuffer());
  if (body.length < 1 || body.length > 1024 * 1024 + 128) {
    throw new Error('Relay control result exceeds limit');
  }
  return body;
}
