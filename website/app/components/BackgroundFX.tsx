/**
 * Fixed background for the "Abyss" theme: a faint blueprint grid that fades in
 * from the top, plus a deep aqua glow pinned to the bottom so the whole page
 * reads as descending into water. Purely decorative, pointer-events: none.
 */
export default function BackgroundFX() {
  return (
    <div className="fx" aria-hidden="true">
      <div className="fx__depth" />
      <div className="fx__grid" />
    </div>
  );
}
