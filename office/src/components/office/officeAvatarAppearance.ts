/**
 * Deterministic original avatar appearance for the isometric Office.
 *
 * `lookIndex` comes from the layout, where it is unique across the
 * scene. It makes five Implementers five recognizable people who are This mapping turns it into clothing, hair, skin and accessory
 * choices, so five Implementers are five recognizable people rather than five
 * still stable across polling and a stable identity-based palette. The palette
 * is Factory-original; no game art is used.
 */

export interface OfficeAvatarLook {
  /** 1-based palette indices. */
  skin: number;
  hairColor: number;
  /** 0–5: short, side part, bob, bun, curls, ponytail. */
  hairStyle: number;
  clothing: number;
  /** 1-based lanyard/badge accent. */
  accent: number;
  /** 0 none, 1 glasses, 2 headphones. */
  accessory: number;
  signature: string;
}

const SKIN_COUNT = 5;
const HAIR_COLOR_COUNT = 7;
const HAIR_STYLE_COUNT = 6;
const CLOTHING_COUNT = 8;

export function officeAvatarLook(lookIndex: number): OfficeAvatarLook {
  const index =
    ((Math.trunc(lookIndex) % (HAIR_STYLE_COUNT * CLOTHING_COUNT)) +
      HAIR_STYLE_COUNT * CLOTHING_COUNT) %
    (HAIR_STYLE_COUNT * CLOTHING_COUNT);
  const clothing = (index % CLOTHING_COUNT) + 1;
  const hairStyle = Math.floor(index / CLOTHING_COUNT);
  const hairColor = ((index * 3 + hairStyle) % HAIR_COLOR_COUNT) + 1;
  const skin = ((index * 2 + Math.floor(index / 5)) % SKIN_COUNT) + 1;
  const accent = (index % 4) + 1;
  const accessory = index % 3;
  return {
    skin,
    hairColor,
    hairStyle,
    clothing,
    accent,
    accessory,
    signature: `${skin}-${hairColor}-${hairStyle}-${clothing}-${accent}-${accessory}`,
  };
}
