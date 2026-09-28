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
  const response = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/cbor' },
    body: envelope,
    redirect: 'error',
    cache: 'no-store',
  });
  if (!response.ok) throw new Error(`Relay batch upload failed: ${response.status}`);
}
