/**
 * Minimal UUIDv7 generator. No crypto/Node APIs are available in the plugin
 * sandbox (confirmed: doc/openrct2.d.ts exposes no crypto binding), so this
 * uses Math.random() -- fine here, since message_id only needs to be
 * unique and time-ordered within a session, not cryptographically
 * unguessable.
 */
export function randomUuidV7(): string {
  const timestamp = Date.now();
  const timestampHex = timestamp.toString(16).padStart(12, "0");

  const randomHex = (chars: number) => {
    let s = "";
    for (let i = 0; i < chars; i++) {
      s += Math.floor(Math.random() * 16).toString(16);
    }
    return s;
  };

  const variantNibble = "89ab"[Math.floor(Math.random() * 4)];

  return [
    timestampHex.slice(0, 8),
    timestampHex.slice(8, 12),
    `7${randomHex(3)}`,
    `${variantNibble}${randomHex(3)}`,
    randomHex(12),
  ].join("-");
}
