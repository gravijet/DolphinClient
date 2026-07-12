/**
 * The "Sonar" background: a still, precise instrument field — a fine hairline
 * grid that fades out toward the edges, one faint accent "horizon" line high on
 * the page, and a soft vignette. No motion, no blur, no colour wash: the calm,
 * measured opposite of a template gradient. Purely decorative.
 */
export default function BackgroundFX() {
  return (
    <div className="bg" aria-hidden="true">
      <div className="bg__grid" />
      <div className="bg__horizon" />
      <div className="bg__vignette" />
    </div>
  );
}
