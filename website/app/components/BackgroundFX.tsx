/**
 * Dekorativer Hintergrund: ein sehr dezentes Raster, das nach unten ausblendet.
 * Rein visuell, pointer-events: none, hinter allem. Bewusst zurückhaltend —
 * die Farbe kommt aus den weichen Verläufen am Seitenrand (globals.css).
 */
export default function BackgroundFX() {
  return (
    <div className="fx" aria-hidden="true">
      <div className="fx__grid" />
    </div>
  );
}
