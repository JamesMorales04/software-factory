/**
 * Deterministic original avatar appearance for the isometric Office.
 *
 * `lookIndex` comes from the layout, where it is unique across the scene. This
 * mapping turns it into clothing, hair, skin, silhouette, accessory and
 * facial-hair choices, so five Implementers are five recognizable people
 * rather than five copies of a role icon. The appearance remains stable across
 * polling and for the same observed set. The palette is Factory-original; no
 * game art is used.
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
  /** 0 none, 1 glasses, 2 headphones, 3 both. */
  accessory: number;
  /** 1–3 body build, used only for an original silhouette. */
  build: number;
  /** 0 none, 1 full beard, 2 goatee, 3 moustache, 4 stubble. */
  facialHair: number;
  /** 1-based trouser palette. */
  trouser: number;
  /** 0 none, 1–3 jacket/overshirt palette. */
  jacket: number;
  signature: string;
}

export const OFFICE_SKIN_COUNT = 6;
export const OFFICE_HAIR_COLOR_COUNT = 8;
export const OFFICE_HAIR_STYLE_COUNT = 6;
export const OFFICE_CLOTHING_COUNT = 8;
export const OFFICE_ACCENT_COUNT = 5;
export const OFFICE_JACKET_COUNT = 4;
export const OFFICE_TROUSER_COUNT = 4;
export const OFFICE_BUILD_COUNT = 3;

export function officeAvatarLook(lookIndex: number): OfficeAvatarLook {
  const cycle = OFFICE_HAIR_STYLE_COUNT * OFFICE_CLOTHING_COUNT;
  const normalized = Number.isFinite(lookIndex) ? Math.trunc(lookIndex) : 0;
  const index = ((normalized % cycle) + cycle) % cycle;
  const clothing = (index % OFFICE_CLOTHING_COUNT) + 1;
  const hairStyle = Math.floor(index / OFFICE_CLOTHING_COUNT);
  const hairColor = ((index * 3 + hairStyle) % OFFICE_HAIR_COLOR_COUNT) + 1;
  const skin = ((index * 2 + Math.floor(index / 5)) % OFFICE_SKIN_COUNT) + 1;
  const accent = (index % OFFICE_ACCENT_COUNT) + 1;
  const accessory = index % 4;
  const build = ((index * 5 + Math.floor(index / 8)) % OFFICE_BUILD_COUNT) + 1;
  const facialHair = index % 7;
  const trouser = ((index * 3 + hairStyle) % OFFICE_TROUSER_COUNT) + 1;
  const jacket = index % OFFICE_JACKET_COUNT;
  return {
    skin,
    hairColor,
    hairStyle,
    clothing,
    accent,
    accessory,
    build,
    facialHair,
    trouser,
    jacket,
    signature: [
      skin,
      hairColor,
      hairStyle,
      clothing,
      accent,
      accessory,
      build,
      facialHair,
      trouser,
      jacket,
    ].join("-"),
  };
}
