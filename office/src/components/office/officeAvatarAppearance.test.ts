import { describe, expect, it } from "vitest";
import {
  OFFICE_AVATAR_LOOK_COUNT,
  OFFICE_BUILD_COUNT,
  OFFICE_HAIR_COLOR_COUNT,
  OFFICE_HAIR_STYLE_COUNT,
  OFFICE_JACKET_COUNT,
  OFFICE_SKIN_COUNT,
  OFFICE_TROUSER_COUNT,
  officeAvatarLook,
} from "./officeAvatarAppearance";

describe("officeAvatarLook", () => {
  it("is deterministic for every stable look index", () => {
    for (let index = 0; index < 256; index += 1) {
      expect(officeAvatarLook(index)).toEqual(officeAvatarLook(index));
      expect(officeAvatarLook(index).signature).toBe(
        officeAvatarLook(index).signature,
      );
    }
    expect(officeAvatarLook(48)).not.toEqual(officeAvatarLook(0));
    expect(officeAvatarLook(OFFICE_AVATAR_LOOK_COUNT)).toEqual(
      officeAvatarLook(0),
    );
    expect(officeAvatarLook(-1)).toEqual(
      officeAvatarLook(OFFICE_AVATAR_LOOK_COUNT - 1),
    );
    expect(officeAvatarLook(Number.NaN).signature).toBe(
      officeAvatarLook(0).signature,
    );
  });

  it("keeps every palette value inside its original palette", () => {
    for (let index = 0; index < 200; index += 1) {
      const look = officeAvatarLook(index);
      expect(look.skin).toBeGreaterThanOrEqual(1);
      expect(look.skin).toBeLessThanOrEqual(OFFICE_SKIN_COUNT);
      expect(look.hairColor).toBeGreaterThanOrEqual(1);
      expect(look.hairColor).toBeLessThanOrEqual(OFFICE_HAIR_COLOR_COUNT);
      expect(look.hairStyle).toBeGreaterThanOrEqual(0);
      expect(look.hairStyle).toBeLessThan(OFFICE_HAIR_STYLE_COUNT);
      expect(look.build).toBeGreaterThanOrEqual(1);
      expect(look.build).toBeLessThanOrEqual(OFFICE_BUILD_COUNT);
      expect(look.jacket).toBeGreaterThanOrEqual(0);
      expect(look.jacket).toBeLessThanOrEqual(OFFICE_JACKET_COUNT - 1);
      expect(look.trouser).toBeGreaterThanOrEqual(1);
      expect(look.trouser).toBeLessThanOrEqual(OFFICE_TROUSER_COUNT);
      expect(look.accessory).toBeGreaterThanOrEqual(0);
      expect(look.accessory).toBeLessThanOrEqual(3);
    }
  });

  it("gives 48 observed participants 48 distinct signatures", () => {
    const looks = Array.from({ length: 48 }, (_, index) =>
      officeAvatarLook(index),
    );
    expect(new Set(looks.map((look) => look.signature)).size).toBe(48);
    expect(new Set(looks.map((look) => look.skin)).size).toBe(
      OFFICE_SKIN_COUNT,
    );
    expect(new Set(looks.map((look) => look.hairStyle)).size).toBe(
      OFFICE_HAIR_STYLE_COUNT,
    );
    expect(new Set(looks.map((look) => look.build)).size).toBe(
      OFFICE_BUILD_COUNT,
    );
    expect(new Set(looks.map((look) => look.jacket)).size).toBe(
      OFFICE_JACKET_COUNT,
    );
    expect(new Set(looks.map((look) => look.trouser)).size).toBe(
      OFFICE_TROUSER_COUNT,
    );
    expect(new Set(looks.map((look) => look.facialHair)).size).toBeGreaterThan(
      3,
    );
  });

  it("gives the current 200-participant Office page distinct signatures", () => {
    const looks = Array.from({ length: 200 }, (_, index) =>
      officeAvatarLook(index),
    );
    expect(new Set(looks.map((look) => look.signature)).size).toBe(200);
  });
});
