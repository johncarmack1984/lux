/**
 * Role-aware color mixing for RGB / RGBW / RGBA / RGBAW fixtures.
 *
 * A picked sRGB color is decomposed across whatever color emitters a fixture
 * actually has. The White emitter takes the achromatic (desaturated) part, the
 * Amber emitter takes the warm yellow-orange part, and the residual chroma stays
 * in R/G/B. With these nominal emitter colors the split reconstructs the target
 * exactly, so the picked color is preserved while more emitters carry it: a
 * fuller spectrum (cleaner whites, richer warms) than faking everything in RGB.
 *
 * The chromaticities below are uncalibrated defaults (neutral white, a deep
 * ~590nm amber), which makes the result perceptual rather than colorimetric.
 * That is good enough without per-fixture measurement, and it is the single
 * place to tune if we ever add calibration.
 */

// Emitter appearance as sRGB fractions (0..1).
const AMBER = { r: 1, g: 0.5, b: 0 };
const WHITE = { r: 1, g: 1, b: 1 };

export interface Emitters {
  r: number;
  g: number;
  b: number;
  a: number; // amber
  w: number; // white
}

const clamp = (n: number) => Math.max(0, Math.min(255, Math.round(n)));

/**
 * Decompose a target sRGB color (0..255) into emitter intensities, using the
 * White and Amber emitters only when the fixture has them.
 */
export function mixToEmitters(
  r: number,
  g: number,
  b: number,
  has: { amber: boolean; white: boolean }
): Emitters {
  let a = 0;
  let w = 0;

  // White carries the shared achromatic floor.
  if (has.white) {
    w = Math.min(r, g, b);
    r -= w;
    g -= w;
    b -= w;
  }

  // Amber carries the yellow-orange content (red and green together, no blue),
  // bounded by how much red and green remain given amber's own r:g ratio.
  if (has.amber) {
    a = Math.min(r / AMBER.r, g / AMBER.g);
    r -= a * AMBER.r;
    g -= a * AMBER.g;
  }

  return { r: clamp(r), g: clamp(g), b: clamp(b), a: clamp(a), w: clamp(w) };
}

/**
 * Recombine emitter intensities into the approximate sRGB color they produce.
 * Used to render the swatch so it stays honest after a mix, or when the amber /
 * white sliders are nudged by hand.
 */
export function emittersToRgb(e: Emitters): { r: number; g: number; b: number } {
  return {
    r: clamp(e.r + e.a * AMBER.r + e.w * WHITE.r),
    g: clamp(e.g + e.a * AMBER.g + e.w * WHITE.g),
    b: clamp(e.b + e.a * AMBER.b + e.w * WHITE.b),
  };
}
