"use client";

import Counter from "./Counter";
import Logo from "./Logo";

/**
 * The hero centrepiece: a compact performance readout with a self-drawing FPS
 * line. The number counts up when it scrolls into view; the line strokes itself
 * on in CSS (stroke-dashoffset). Deliberately data-shaped, not decorative — the
 * product is about frames, so frames are the hero. Figures are honest reference
 * values (Richtwerte), framed as such in the page copy.
 */

// A gently rising FPS trace across a 320×120 viewport (y grows downward).
const PTS = "0,98 27,90 53,94 80,72 107,80 133,55 160,62 187,40 213,48 240,28 267,34 293,18 320,24";

export default function PerfChart() {
  return (
    <div className="readout" role="img" aria-label="Live-Leistung: rund 318 Bilder pro Sekunde">
      <div className="readout__top">
        <Logo />
        <span>leistung — echtzeit</span>
        <span className="readout__dot" />
      </div>

      <div className="readout__fps">
        <span className="big">
          <Counter to={318} duration={1800} />
        </span>
        <span className="unit">FPS · flüssig</span>
      </div>

      <div className="readout__chart">
        <svg viewBox="0 0 320 120" preserveAspectRatio="none">
          <g className="readout__gridlines">
            <line x1="0" y1="30" x2="320" y2="30" />
            <line x1="0" y1="60" x2="320" y2="60" />
            <line x1="0" y1="90" x2="320" y2="90" />
          </g>
          <polygon className="pc-area" points={`0,120 ${PTS} 320,120`} />
          <polyline
            className="pc-line"
            points={PTS}
            pathLength={100}
            fill="none"
          />
          <circle className="pc-tip" cx="320" cy="24" r="3.5" />
        </svg>
      </div>

      <div className="readout__rows">
        <div className="readout__row">
          <span className="k">ladezeit</span>
          <span className="l" />
          <span className="v good">4,8 s</span>
        </div>
        <div className="readout__row">
          <span className="k">speicher</span>
          <span className="l" />
          <span className="v good">1,4 GB</span>
        </div>
        <div className="readout__row">
          <span className="k">status</span>
          <span className="l" />
          <span className="v good">bereit</span>
        </div>
      </div>
    </div>
  );
}
