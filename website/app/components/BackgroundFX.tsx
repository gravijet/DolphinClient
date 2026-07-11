/**
 * The "Prism" background: a living, iridescent aurora made of four large,
 * heavily-blurred colour blobs that drift continuously and blend additively —
 * so the whole page sits in slowly-moving light. A faint film grain breaks up
 * the gradient banding, and a vignette keeps the edges grounded in deep water.
 * Purely decorative; pointer-events: none. Motion pauses under
 * prefers-reduced-motion (handled in globals.css).
 */
export default function BackgroundFX() {
  return (
    <div className="bg" aria-hidden="true">
      <div className="bg__aurora">
        <span className="bg__blob b1" />
        <span className="bg__blob b2" />
        <span className="bg__blob b3" />
        <span className="bg__blob b4" />
      </div>
      <div className="bg__grain" />
      <div className="bg__vignette" />
    </div>
  );
}
