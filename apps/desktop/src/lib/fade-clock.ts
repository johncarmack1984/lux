/**
 * When the last scene recall's crossfade will have finished, so `useBuffer`
 * knows to poll while one is in flight.
 *
 * A recall changes the buffer from the backend for its whole fade, and the
 * `bufferSet` events that carry those ticks never reach the webview on iOS.
 * The buffer poll normally runs only while a remote peer is connected (see
 * useBuffer); this clock turns it on for the one other window where the
 * backend is driving the buffer on its own. Its own module so the actions
 * that set it and the hook that reads it don't import each other.
 */
let fadeUntil = 0;

/** Note a recall just started: poll until `fadeMs` from now, plus slack for
 * the last tick and one poll interval. */
export function noteFadeStarted(fadeMs: number) {
  fadeUntil = Date.now() + fadeMs + 500;
}

/** Whether a recall's crossfade may still be writing the buffer. */
export function fadeInFlight(): boolean {
  return Date.now() < fadeUntil;
}
