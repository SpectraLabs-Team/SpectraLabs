// Collision-free placement for the hero's three holo cards, computed from measured geometry
// (real font metrics, real card heights, the plate's actual scale) so it holds in every browser.

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The spiral, as a rotated ellipse in hero-backdrop.webp pixel coordinates (1672×941). */
export const PLATE = { w: 1672, h: 941 };
const SPIRAL = { cx: 1405, cy: 192, a: 292, b: 118, rot: (-21 * Math.PI) / 180 };
/** Ambient drift (see tailwind `hero-drift`): scale 1 → 1.04 about 84%/22% of the plate, plus a -1%/-0.5% shift. */
const DRIFT = { scale: 1.04, ox: 0.84, oy: 0.22, tx: -0.01, ty: -0.005 };

export interface Ellipse {
  cx: number;
  cy: number;
  a: number;
  b: number;
  rot: number;
}

/**
 * Where the spiral is in hero coordinates, for a plate box (its layout box, transforms excluded)
 * rendered with object-fit: cover and object-position: right top. Returns the ellipse at both ends
 * of the drift, each grown by `margin` px.
 */
export function spiralEllipses(plate: Rect, margin: number): Ellipse[] {
  const s = Math.max(plate.w / PLATE.w, plate.h / PLATE.h);
  const left = plate.x + plate.w - PLATE.w * s;
  const top = plate.y;
  const base = { cx: left + SPIRAL.cx * s, cy: top + SPIRAL.cy * s, a: SPIRAL.a * s, b: SPIRAL.b * s, rot: SPIRAL.rot };
  const ox = plate.x + DRIFT.ox * plate.w;
  const oy = plate.y + DRIFT.oy * plate.h;
  const end = {
    cx: ox + DRIFT.scale * (base.cx - ox) + DRIFT.tx * plate.w,
    cy: oy + DRIFT.scale * (base.cy - oy) + DRIFT.ty * plate.h,
    a: base.a * DRIFT.scale,
    b: base.b * DRIFT.scale,
    rot: base.rot,
  };
  return [base, end].map((e) => ({ ...e, a: e.a + margin, b: e.b + margin }));
}

function inEllipse(px: number, py: number, e: Ellipse) {
  const dx = px - e.cx;
  const dy = py - e.cy;
  const c = Math.cos(e.rot);
  const s = Math.sin(e.rot);
  const u = (dx * c + dy * s) / e.a;
  const v = (-dx * s + dy * c) / e.b;
  return u * u + v * v <= 1;
}

/** Rectangle/ellipse intersection, sampled on a fine grid (cards are far smaller than the ellipse). */
export function hitsEllipse(r: Rect, e: Ellipse) {
  if (inEllipse(r.x + r.w / 2, r.y + r.h / 2, e)) return true;
  const nx = 12;
  const ny = 8;
  for (let i = 0; i <= nx; i++)
    for (let j = 0; j <= ny; j++) if (inEllipse(r.x + (r.w * i) / nx, r.y + (r.h * j) / ny, e)) return true;
  // Ellipse centre inside the rect (not reachable for card-sized rects, kept for safety).
  return e.cx >= r.x && e.cx <= r.x + r.w && e.cy >= r.y && e.cy <= r.y + r.h;
}

const overlaps = (a: Rect, b: Rect, gap: number) => a.x < b.x + b.w + gap && b.x < a.x + a.w + gap && a.y < b.y + b.h + gap && b.y < a.y + a.h + gap;

export interface LayoutInput {
  /** Card sizes in order: Kernel, Risk, Ledger. */
  sizes: { w: number; h: number }[];
  ellipses: Ellipse[];
  /** Cards must start at or right of this x (headline text extent plus a gap). */
  minX: number;
  /** Right edge of the content column. */
  maxX: number;
  /** Top of the card area. */
  minY: number;
  /** Bottom limit (top of the ticker row, minus a gap). */
  maxY: number;
}

/** Cards float up to 8px; reserve that above each card. */
const FLOAT = 8;
const STEP = 4;

/**
 * Kernel: highest free spot, as far right as it can sit beside the galaxy (i.e. in the dark space
 * to its left). Risk: below Kernel with a 40px gap (24px if space is short). Ledger: far right,
 * below the galaxy, clear of both. A card that cannot be placed returns null and is not shown.
 */
export function layoutHeroCards({ sizes, ellipses, minX, maxX, minY, maxY }: LayoutInput): (Rect | null)[] {
  const free = (r: Rect, placed: (Rect | null)[], gap = 20) => {
    const hit = { ...r, y: r.y - FLOAT, h: r.h + FLOAT };
    if (r.x < minX || r.x + r.w > maxX || r.y < minY || r.y + r.h > maxY) return false;
    if (ellipses.some((e) => hitsEllipse(hit, e))) return false;
    return placed.every((p) => !p || !overlaps(hit, p, gap));
  };

  const [k, rk, lg] = sizes;
  // Kernel leaves a Ledger-wide lane on the right when it can, so reading order stays
  // Kernel → Risk (below) → Ledger (right). Retry without the lane if nothing fits.
  let kernel: Rect | null = null;
  for (const lane of [lg.w + 24, 0]) {
    for (let y = minY; !kernel && y + k.h <= maxY; y += STEP)
      for (let x = maxX - k.w - lane; x >= minX; x -= STEP) {
        const r = { x, y, ...k };
        if (free(r, [])) {
          kernel = r;
          break;
        }
      }
    if (kernel) break;
  }

  let riskCard: Rect | null = null;
  if (kernel) {
    for (const gapV of [40, 32, 24]) {
      for (let y = kernel.y + kernel.h + gapV; !riskCard && y + rk.h <= maxY; y += STEP)
        for (let x = Math.min(maxX - rk.w, kernel.x + 48); x >= minX; x -= STEP) {
          const r = { x, y, ...rk };
          if (free(r, [kernel], gapV - FLOAT)) {
            riskCard = r;
            break;
          }
        }
      if (riskCard) break;
    }
  }

  // Ledger hugs the right edge: search a narrow right-hand band first, widening only if needed.
  let ledger: Rect | null = null;
  for (const band of [96, 200, Infinity]) {
    const xMin = Math.max(minX, maxX - lg.w - band);
    for (let y = minY; !ledger && y + lg.h <= maxY; y += STEP)
      for (let x = maxX - lg.w; x >= xMin; x -= STEP) {
        const r = { x, y, ...lg };
        if (free(r, [kernel, riskCard], 24)) {
          ledger = r;
          break;
        }
      }
    if (ledger) break;
  }

  return [kernel, riskCard, ledger];
}
