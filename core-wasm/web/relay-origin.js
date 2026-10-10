// The relay this web build talks to. Empty means the page's own origin; a
// separately hosted build sets it once at startup. Invitations must name it.
let relayOrigin = '';

export function setRelayOrigin(origin) {
  if (origin && !/^https?:\/\/[A-Za-z0-9.-]+(:\d+)?$/.test(origin)) throw new Error('Invalid relay origin');
  relayOrigin = origin || '';
}

export const expectedRelayOrigin = () => relayOrigin || location.origin;

export const relayUrl = (path) => `${relayOrigin}${path}`;
