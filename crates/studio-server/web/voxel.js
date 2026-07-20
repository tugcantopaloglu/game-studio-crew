export const PALETTES = {
  studio_director: { shirt: 0x8d6b1f, trim: 0xd9b44a, pants: 0x3a3320, skin: 0xe8b98a, hair: 0x4a3a26, accent: 0xf0d97a },
  producer:        { shirt: 0x7a6a3a, trim: 0xc8a24a, pants: 0x36322a, skin: 0xd9a878, hair: 0x2e2822, accent: 0xe6c66a },
  game_designer:   { shirt: 0x5a4a8f, trim: 0x8a6fd1, pants: 0x2f2a44, skin: 0xe8c39a, hair: 0x33264a, accent: 0xb49ae8 },
  level_designer:  { shirt: 0x4c3f7d, trim: 0x7f68c0, pants: 0x2b2740, skin: 0xc99a72, hair: 0x241c33, accent: 0xa78fdd },
  narrative_designer: { shirt: 0x584a86, trim: 0x9179cc, pants: 0x2e2942, skin: 0xefcaa6, hair: 0x5a3a2a, accent: 0xbca6ec },
  ux_designer:     { shirt: 0x50448a, trim: 0x8874c8, pants: 0x2c2840, skin: 0xd7a97e, hair: 0x1f1a2c, accent: 0xb09ae4 },
  systems_engineer:{ shirt: 0x2f5a8a, trim: 0x4a90d9, pants: 0x243040, skin: 0xdcae82, hair: 0x2b2b30, accent: 0x7dc0f0 },
  gameplay_engineer:{ shirt: 0x2a5480, trim: 0x4485cc, pants: 0x222c3a, skin: 0xe6bd92, hair: 0x3a2c20, accent: 0x74b6ea },
  infra_engineer:  { shirt: 0x8a4a2a, trim: 0xd97a4a, pants: 0x33281f, skin: 0xc99368, hair: 0x241c16, accent: 0xf0a06a },
  tech_artist:     { shirt: 0x8a4468, trim: 0xd16f9a, pants: 0x3a2430, skin: 0xecc3a2, hair: 0x4a2438, accent: 0xf094ba },
  artist:          { shirt: 0x9c4a72, trim: 0xe07aa6, pants: 0x3e2634, skin: 0xd9a67e, hair: 0x6a2a44, accent: 0xf7a8c8 },
  qa_engineer:     { shirt: 0x8a5230, trim: 0xd97a4a, pants: 0x352a20, skin: 0xe0b389, hair: 0x2a2018, accent: 0xf0a070 },
  audio_designer:  { shirt: 0x2f7a70, trim: 0x4fb3a5, pants: 0x223834, skin: 0xd9ab80, hair: 0x1e2a28, accent: 0x7fd6c8 },
};

export const HEADGEAR = {
  studio_director: "crown",
  producer: "cap",
  game_designer: "none",
  level_designer: "cap",
  narrative_designer: "none",
  ux_designer: "none",
  systems_engineer: "hardhat",
  gameplay_engineer: "headset",
  infra_engineer: "hardhat",
  tech_artist: "none",
  artist: "beret",
  qa_engineer: "cap",
  audio_designer: "headphones",
};

export const PROPS = {
  studio_director: "none",
  producer: "clipboard",
  game_designer: "clipboard",
  level_designer: "ruler",
  narrative_designer: "quill",
  ux_designer: "tablet",
  systems_engineer: "wrench",
  gameplay_engineer: "wrench",
  infra_engineer: "wrench",
  tech_artist: "brush",
  artist: "brush",
  qa_engineer: "magnifier",
  audio_designer: "none",
};

const SKIN_DARK = 0x000000;

function jitter(color, amount, seed) {
  const r = (color >> 16) & 255, g = (color >> 8) & 255, b = color & 255;
  const n = (Math.sin(seed * 12.9898) * 43758.5453) % 1;
  const d = Math.round((n - 0.5) * 2 * amount);
  const c = (v) => Math.max(0, Math.min(255, v + d));
  return (c(r) << 16) | (c(g) << 8) | c(b);
}

export function box(out, x, y, z, w, h, d, color, jit = 6) {
  for (let ix = 0; ix < w; ix++)
    for (let iy = 0; iy < h; iy++)
      for (let iz = 0; iz < d; iz++) {
        const px = x + ix, py = y + iy, pz = z + iz;
        out.push({ x: px, y: py, z: pz, c: jitter(color, jit, px * 7 + py * 13 + pz * 29) });
      }
}

function vox(out, x, y, z, color) {
  out.push({ x, y, z, c: color });
}

function headgear(out, kind, p) {
  switch (kind) {
    case "crown":
      box(out, 1, 15, 1, 4, 1, 3, p.accent, 3);
      vox(out, 1, 16, 2, p.accent);
      vox(out, 2, 17, 2, p.accent);
      vox(out, 4, 16, 2, p.accent);
      break;
    case "hardhat":
      box(out, 1, 15, 1, 4, 1, 3, p.accent, 3);
      box(out, 0, 15, 1, 6, 1, 3, p.accent, 3);
      break;
    case "cap":
      box(out, 1, 15, 1, 4, 1, 3, p.trim, 3);
      box(out, 1, 15, 4, 4, 1, 1, p.trim, 3);
      break;
    case "beret":
      box(out, 1, 15, 1, 4, 1, 3, p.accent, 4);
      vox(out, 4, 16, 2, p.accent);
      break;
    case "headset":
      box(out, 0, 13, 1, 1, 2, 3, p.accent, 2);
      box(out, 5, 13, 1, 1, 2, 3, p.accent, 2);
      box(out, 1, 15, 2, 4, 1, 1, p.accent, 2);
      break;
    case "headphones":
      box(out, 0, 12, 1, 1, 3, 3, p.accent, 2);
      box(out, 5, 12, 1, 1, 3, 3, p.accent, 2);
      box(out, 1, 15, 1, 4, 1, 3, p.accent, 2);
      break;
  }
}

function prop(out, kind, p) {
  switch (kind) {
    case "clipboard":
      box(out, 6, 7, 1, 1, 3, 2, 0xd8d2c0, 4);
      break;
    case "tablet":
      box(out, 6, 7, 1, 1, 3, 3, 0x2b3040, 4);
      break;
    case "ruler":
      box(out, 6, 6, 2, 1, 5, 1, 0xd8c88a, 4);
      break;
    case "quill":
      box(out, 6, 8, 2, 1, 4, 1, 0xf0ece0, 5);
      break;
    case "wrench":
      box(out, 6, 7, 2, 1, 4, 1, 0x9aa4b0, 4);
      vox(out, 6, 11, 2, 0xb8c2cc);
      vox(out, 6, 11, 1, 0xb8c2cc);
      break;
    case "brush":
      box(out, 6, 7, 2, 1, 4, 1, 0xb08050, 4);
      vox(out, 6, 11, 2, p.trim);
      break;
    case "magnifier":
      box(out, 6, 7, 2, 1, 2, 1, 0x8a7050, 4);
      box(out, 6, 9, 1, 1, 2, 3, 0x9aa4b0, 3);
      break;
  }
}

export function buildCharacter(roleId) {
  const p = PALETTES[roleId] || PALETTES.gameplay_engineer;
  const v = [];

  box(v, 1, 0, 1, 2, 4, 3, p.pants);
  box(v, 3, 0, 1, 2, 4, 3, p.pants);
  box(v, 1, 0, 1, 2, 1, 3, 0x1a1a20, 3);
  box(v, 3, 0, 1, 2, 1, 3, 0x1a1a20, 3);

  box(v, 1, 4, 1, 4, 6, 3, p.shirt);
  box(v, 1, 9, 1, 4, 1, 3, p.trim, 4);

  box(v, 0, 4, 1, 1, 5, 3, p.shirt);
  box(v, 5, 4, 1, 1, 5, 3, p.shirt);
  box(v, 0, 3, 1, 1, 1, 3, p.skin);
  box(v, 5, 3, 1, 1, 1, 3, p.skin);

  box(v, 2, 10, 2, 2, 1, 2, p.skin);
  box(v, 1, 11, 1, 4, 4, 3, p.skin);
  box(v, 1, 14, 1, 4, 1, 3, p.hair, 4);
  box(v, 1, 13, 0, 4, 2, 1, p.hair, 4);
  box(v, 0, 12, 1, 1, 3, 3, p.hair, 4);
  box(v, 5, 12, 1, 1, 3, 3, p.hair, 4);

  vox(v, 2, 12, 4, SKIN_DARK);
  vox(v, 4, 12, 4, SKIN_DARK);

  headgear(v, HEADGEAR[roleId] || "none", p);
  prop(v, PROPS[roleId] || "none", p);

  return v;
}

export function buildDesk(tint) {
  const v = [];
  box(v, 0, 0, 0, 10, 1, 6, 0x3a3f4c, 4);
  box(v, 0, 1, 0, 10, 4, 1, 0x2e3340, 4);
  box(v, 0, 1, 5, 10, 4, 1, 0x2e3340, 4);
  box(v, 0, 5, 0, 10, 1, 6, 0x4a5060, 4);
  box(v, 2, 6, 1, 6, 4, 1, 0x14161c, 3);
  box(v, 3, 7, 2, 4, 2, 1, tint, 6);
  box(v, 2, 6, 4, 6, 1, 2, 0x2a2e38, 4);
  return v;
}

export function characterBounds() {
  return { w: 7, h: 18, d: 5 };
}
