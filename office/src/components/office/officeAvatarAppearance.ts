/**
 * Deterministic original avatar appearance for the isometric Office.
 *
 * `lookIndex` comes from the layout, where it is unique across the scene. This
 * mapping turns it into clothing, hair, skin, silhouette, accessory and
 * facial-hair choices, so Implementers are recognizable people
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
export const OFFICE_ACCESSORY_COUNT = 4;
export const OFFICE_JACKET_COUNT = 4;
export const OFFICE_TROUSER_COUNT = 4;
export const OFFICE_BUILD_COUNT = 3;
export const OFFICE_FACIAL_HAIR_COUNT = 5;

/** Number of deterministic combinations supported by the original palette. */
export const OFFICE_AVATAR_LOOK_COUNT =
  OFFICE_SKIN_COUNT *
  OFFICE_HAIR_COLOR_COUNT *
  OFFICE_HAIR_STYLE_COUNT *
  OFFICE_CLOTHING_COUNT *
  OFFICE_ACCENT_COUNT *
  OFFICE_ACCESSORY_COUNT *
  OFFICE_JACKET_COUNT *
  OFFICE_TROUSER_COUNT *
  OFFICE_BUILD_COUNT *
  OFFICE_FACIAL_HAIR_COUNT;

function makeLook(
  skin: number,
  hairColor: number,
  hairStyle: number,
  clothing: number,
  accent: number,
  accessory: number,
  build: number,
  facialHair: number,
  trouser: number,
  jacket: number,
): OfficeAvatarLook {
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

function legacyLook(index: number): OfficeAvatarLook {
  const clothing = (index % OFFICE_CLOTHING_COUNT) + 1;
  const hairStyle = Math.floor(index / OFFICE_CLOTHING_COUNT);
  const hairColor = ((index * 3 + hairStyle) % OFFICE_HAIR_COLOR_COUNT) + 1;
  const skin = ((index * 2 + Math.floor(index / 5)) % OFFICE_SKIN_COUNT) + 1;
  const accent = (index % OFFICE_ACCENT_COUNT) + 1;
  const accessory = index % OFFICE_ACCESSORY_COUNT;
  const build = ((index * 5 + Math.floor(index / 8)) % OFFICE_BUILD_COUNT) + 1;
  const facialHair = index % 7;
  const trouser = ((index * 3 + hairStyle) % OFFICE_TROUSER_COUNT) + 1;
  const jacket = index % OFFICE_JACKET_COUNT;
  return makeLook(
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
  );
}

function expandedLook(index: number): OfficeAvatarLook {
  let remaining = index;
  const take = (count: number) => {
    const value = remaining % count;
    remaining = Math.floor(remaining / count);
    return value;
  };
  const clothing = take(OFFICE_CLOTHING_COUNT) + 1;
  const hairStyle = take(OFFICE_HAIR_STYLE_COUNT);
  const skin = take(OFFICE_SKIN_COUNT) + 1;
  const hairColor = take(OFFICE_HAIR_COLOR_COUNT) + 1;
  const accent = take(OFFICE_ACCENT_COUNT) + 1;
  const accessory = take(OFFICE_ACCESSORY_COUNT);
  const build = take(OFFICE_BUILD_COUNT) + 1;
  const facialHair = take(OFFICE_FACIAL_HAIR_COUNT);
  const trouser = take(OFFICE_TROUSER_COUNT) + 1;
  const jacket = take(OFFICE_JACKET_COUNT);
  return makeLook(
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
  );
}

const LEGACY_LOOK_COUNT = OFFICE_HAIR_STYLE_COUNT * OFFICE_CLOTHING_COUNT;
const LEGACY_LOOKS = Array.from({ length: LEGACY_LOOK_COUNT }, (_, index) =>
  legacyLook(index),
);
const LEGACY_LOOK_SIGNATURES = new Set(
  LEGACY_LOOKS.map((look) => look.signature),
);

function encodeLook(look: OfficeAvatarLook): number {
  let encoded = look.jacket;
  encoded = encoded * OFFICE_TROUSER_COUNT + (look.trouser - 1);
  encoded =
    encoded * OFFICE_FACIAL_HAIR_COUNT +
    (look.facialHair % OFFICE_FACIAL_HAIR_COUNT);
  encoded = encoded * OFFICE_BUILD_COUNT + (look.build - 1);
  encoded = encoded * OFFICE_ACCESSORY_COUNT + look.accessory;
  encoded = encoded * OFFICE_ACCENT_COUNT + (look.accent - 1);
  encoded = encoded * OFFICE_HAIR_COLOR_COUNT + (look.hairColor - 1);
  encoded = encoded * OFFICE_SKIN_COUNT + (look.skin - 1);
  encoded = encoded * OFFICE_HAIR_STYLE_COUNT + look.hairStyle;
  return encoded * OFFICE_CLOTHING_COUNT + (look.clothing - 1);
}

const EXTENDED_LOOK_OFFSET =
  Math.max(...LEGACY_LOOKS.map((look) => encodeLook(look))) + 1;
const EXTENDED_LOOK_COUNT = OFFICE_AVATAR_LOOK_COUNT - EXTENDED_LOOK_OFFSET;

export function officeAvatarLook(lookIndex: number): OfficeAvatarLook {
  const normalized = Number.isFinite(lookIndex) ? Math.trunc(lookIndex) : 0;
  const index =
    ((normalized % OFFICE_AVATAR_LOOK_COUNT) + OFFICE_AVATAR_LOOK_COUNT) %
    OFFICE_AVATAR_LOOK_COUNT;
  if (index < LEGACY_LOOK_COUNT) return LEGACY_LOOKS[index];

  // Keep the established first 48 looks unchanged. The extended range starts
  // beyond every encoded legacy combination, so each subsequent index maps
  // directly to a distinct supported palette combination.
  const extendedIndex =
    EXTENDED_LOOK_OFFSET + ((index - LEGACY_LOOK_COUNT) % EXTENDED_LOOK_COUNT);
  const look = expandedLook(extendedIndex);
  return LEGACY_LOOK_SIGNATURES.has(look.signature) ? LEGACY_LOOKS[0] : look;
}
