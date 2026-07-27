export const LOOKS = {
  studio_director: {
    skin: 0xf0c092, hair: 0x1b1c22, hairStyle: "spiky",
    outfit: "suit", jacket: 0x16181e, jacketAlt: 0x21242c, trim: 0xd9b44a,
    shirt: 0x0e1015, pants: 0x191b22, shoes: 0x2e333f, soles: 0x4a5160,
    accessory: "none", prop: "folio",
  },
  producer: {
    skin: 0xf3c79a, hair: 0x5c3c20, hairStyle: "curly",
    outfit: "jacket", jacket: 0x1f3c5e, jacketAlt: 0x2a4d74, trim: 0xe8c14a,
    shirt: 0x2a2d34, pants: 0x24395a, shoes: 0x6d4626, soles: 0x8c5e36,
    accessory: "neckphones", prop: "kanban",
  },
  game_designer: {
    skin: 0xf6d0a4, hair: 0xc4661f, hairStyle: "swoop",
    outfit: "jacket", jacket: 0x9aa1ac, jacketAlt: 0x7a818d, trim: 0xf0c53c,
    shirt: 0x23262e, pants: 0x2c313b, shoes: 0x2c313b, soles: 0xe9e7e0,
    accessory: "none", prop: "flowchart",
  },
  level_designer: {
    skin: 0xf1c193, hair: 0x4c3520, hairStyle: "curly",
    outfit: "field", jacket: 0x8a8562, jacketAlt: 0x6c694c, trim: 0xe2c04a,
    shirt: 0x22242a, pants: 0x767252, shoes: 0x7e7a5d, soles: 0x494737,
    accessory: "none", prop: "mappad",
  },
  narrative_designer: {
    skin: 0xf6cfa2, hair: 0x8c3c1a, hairStyle: "mop",
    outfit: "jacket", jacket: 0xa32633, jacketAlt: 0x7c1c27, trim: 0xd94a52,
    shirt: 0x23262f, pants: 0x2b3040, shoes: 0xe9e1ca, soles: 0x6d4c31,
    accessory: "none", prop: "script",
  },
  ux_designer: {
    skin: 0xf3c79a, hair: 0x4a3524, hairStyle: "mop",
    outfit: "jacket", jacket: 0x6a3fc4, jacketAlt: 0x53309b, trim: 0x4fc8f0,
    shirt: 0xe6e9f0, pants: 0x22252f, shoes: 0x2a2e3a, soles: 0xe9ecf2,
    accessory: "none", prop: "wireframe",
  },
  systems_engineer: {
    skin: 0xf2c69a, hair: 0x6a4526, hairStyle: "short",
    outfit: "tech", jacket: 0x1b2333, jacketAlt: 0x232d40, trim: 0x35d2e8,
    shirt: 0x2a3244, pants: 0x1e2534, shoes: 0x272f3e, soles: 0x8d96a6,
    accessory: "glasses", prop: "codepad",
  },
  gameplay_engineer: {
    skin: 0xf4c99c, hair: 0x7a4a24, hairStyle: "swoop",
    outfit: "hoodie", jacket: 0x232833, jacketAlt: 0x2d3341, trim: 0x35b8e8,
    shirt: 0x1a1e27, pants: 0x262b36, shoes: 0x2a2f3b, soles: 0xd6dae2,
    accessory: "none", prop: "gamepad",
  },
  infra_engineer: {
    skin: 0xf1c193, hair: 0x6a4020, hairStyle: "short",
    outfit: "tech", jacket: 0x22262e, jacketAlt: 0x2c313b, trim: 0x35d9a8,
    shirt: 0x1a1d24, pants: 0x252a33, shoes: 0x2b3039, soles: 0x757d8a,
    accessory: "none", prop: "statustower",
  },
  tech_artist: {
    skin: 0xf5cda0, hair: 0x7a3a20, hairStyle: "mop",
    outfit: "jacket", jacket: 0xb01f45, jacketAlt: 0x871533, trim: 0xf0417a,
    shirt: 0x22252d, pants: 0x272b35, shoes: 0x22262e, soles: 0xf0417a,
    accessory: "none", prop: "nodepad",
  },
  artist: {
    skin: 0xf6d0a4, hair: 0x8a4a22, hairStyle: "curly",
    outfit: "jacket", jacket: 0xd42a5e, jacketAlt: 0xa81f49, trim: 0xf47aa4,
    shirt: 0x23262f, pants: 0x2b303a, shoes: 0x24282f, soles: 0x9aa2ae,
    accessory: "none", prop: "palettepad",
  },
  qa_engineer: {
    skin: 0xf2c69a, hair: 0x7a4c26, hairStyle: "short",
    outfit: "field", jacket: 0x262b34, jacketAlt: 0x30363f, trim: 0xf09030,
    shirt: 0x1b1e25, pants: 0x22262e, shoes: 0x282d36, soles: 0x6e7684,
    accessory: "none", prop: "checklist",
  },
  audio_designer: {
    skin: 0xf3c79a, hair: 0x5e3a1e, hairStyle: "swoop",
    outfit: "tech", jacket: 0x1c1e26, jacketAlt: 0x25282f, trim: 0x9a5fe0,
    shirt: 0x14161c, pants: 0x1f222a, shoes: 0x24272f, soles: 0x5b626e,
    accessory: "neckphones", prop: "recorder",
  },
};

export const HIP_Y = 9;
export const KNEE_Y = 5;
export const SHOULDER_Y = 15;
export const NECK_Y = 16;
export const HAND_Y = 9;

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

function darker(color, factor) {
  const r = Math.round(((color >> 16) & 255) * factor);
  const g = Math.round(((color >> 8) & 255) * factor);
  const b = Math.round((color & 255) * factor);
  return (r << 16) | (g << 8) | b;
}

function look(roleId) {
  return LOOKS[roleId] || LOOKS.gameplay_engineer;
}

function torsoVoxels(p) {
  const v = [];
  box(v, -3, HIP_Y, -2, 6, 7, 4, p.jacket);
  box(v, -3, HIP_Y, -2, 6, 1, 4, darker(p.pants, 0.8), 4);

  switch (p.outfit) {
    case "suit":
      box(v, -1, HIP_Y + 2, 1, 2, 5, 1, p.shirt, 3);
      vox(v, -2, HIP_Y + 5, 1, p.jacketAlt);
      vox(v, 1, HIP_Y + 5, 1, p.jacketAlt);
      box(v, 0, HIP_Y + 4, 1, 2, 1, 1, p.trim, 5);
      box(v, -3, HIP_Y, -2, 6, 1, 4, p.trim, 4);
      break;
    case "jacket":
      box(v, -1, HIP_Y + 1, 1, 2, 6, 1, p.shirt, 3);
      box(v, -2, HIP_Y + 1, 1, 1, 6, 1, p.trim, 4);
      box(v, 1, HIP_Y + 1, 1, 1, 6, 1, p.trim, 4);
      box(v, -3, HIP_Y + 5, -2, 6, 1, 4, p.jacketAlt, 4);
      box(v, -3, HIP_Y, -2, 6, 1, 4, darker(p.trim, 0.66), 4);
      break;
    case "hoodie":
      box(v, -3, HIP_Y + 5, -2, 6, 2, 3, p.jacketAlt, 4);
      box(v, -2, HIP_Y + 4, 1, 4, 1, 1, p.trim, 5);
      box(v, -2, HIP_Y + 1, 1, 4, 2, 1, p.jacketAlt, 4);
      vox(v, -1, HIP_Y + 3, 1, 0xdfe3ea);
      vox(v, 0, HIP_Y + 3, 1, 0xdfe3ea);
      break;
    case "tech":
      box(v, -3, HIP_Y + 5, -2, 6, 1, 4, p.jacketAlt, 4);
      box(v, -3, HIP_Y + 4, 1, 6, 1, 1, p.trim, 5);
      box(v, 1, HIP_Y + 2, 1, 1, 3, 1, p.trim, 5);
      box(v, -3, HIP_Y + 1, 1, 1, 1, 1, p.trim, 5);
      box(v, -1, HIP_Y, 1, 2, 1, 1, 0x9aa2ae, 4);
      break;
    case "field":
      box(v, -3, HIP_Y + 1, 1, 2, 2, 1, p.jacketAlt, 4);
      box(v, 0, HIP_Y + 1, 1, 2, 2, 1, p.jacketAlt, 4);
      box(v, -3, HIP_Y + 5, -2, 6, 1, 4, p.jacketAlt, 4);
      vox(v, -2, HIP_Y + 5, 1, p.trim);
      vox(v, 1, HIP_Y + 5, 1, p.trim);
      box(v, -3, HIP_Y, -2, 6, 1, 4, darker(p.trim, 0.55), 4);
      box(v, -4, HIP_Y + 1, -1, 1, 2, 2, p.jacketAlt, 4);
      break;
  }

  box(v, -1, NECK_Y - 1, -1, 2, 1, 2, p.skin, 3);
  if (p.accessory === "neckphones") {
    box(v, -3, NECK_Y - 2, -1, 1, 2, 2, 0x2b3038, 3);
    box(v, 2, NECK_Y - 2, -1, 1, 2, 2, 0x2b3038, 3);
    box(v, -2, NECK_Y - 1, -2, 4, 1, 1, 0x3a414f, 3);
    vox(v, -3, NECK_Y - 1, 0, 0xb8c2cc);
    vox(v, 2, NECK_Y - 1, 0, 0xb8c2cc);
  }
  return v;
}

function hairVoxels(p) {
  const v = [];
  const h = p.hair;
  box(v, -3, NECK_Y + 5, -2, 6, 1, 5, h, 5);
  box(v, -3, NECK_Y + 2, -2, 6, 3, 1, h, 5);
  box(v, -3, NECK_Y + 3, -2, 1, 2, 5, h, 5);
  box(v, 2, NECK_Y + 3, -2, 1, 2, 5, h, 5);

  switch (p.hairStyle) {
    case "spiky":
      box(v, -3, NECK_Y + 6, -1, 6, 1, 3, h, 7);
      vox(v, -2, NECK_Y + 7, 0, h);
      vox(v, 0, NECK_Y + 7, -1, h);
      vox(v, 2, NECK_Y + 7, 1, h);
      box(v, -3, NECK_Y + 4, 2, 6, 1, 1, h, 5);
      break;
    case "curly":
      box(v, -4, NECK_Y + 4, -3, 8, 2, 7, h, 10);
      box(v, -3, NECK_Y + 6, -1, 6, 1, 4, h, 10);
      box(v, -3, NECK_Y + 4, 2, 6, 1, 1, h, 8);
      break;
    case "swoop":
      box(v, -3, NECK_Y + 6, -1, 5, 1, 4, h, 7);
      box(v, -3, NECK_Y + 4, 2, 6, 1, 1, h, 6);
      box(v, -4, NECK_Y + 4, 1, 1, 2, 2, h, 7);
      vox(v, -4, NECK_Y + 6, 0, h);
      vox(v, -3, NECK_Y + 7, 0, h);
      break;
    case "mop":
      box(v, -4, NECK_Y + 3, -3, 8, 3, 7, h, 8);
      box(v, -3, NECK_Y + 6, -1, 6, 1, 4, h, 8);
      box(v, -3, NECK_Y + 4, 2, 6, 1, 1, h, 7);
      vox(v, -2, NECK_Y + 3, 2, h);
      vox(v, 1, NECK_Y + 3, 2, h);
      break;
    default:
      box(v, -3, NECK_Y + 4, -2, 6, 1, 5, h, 4);
      box(v, -3, NECK_Y + 4, 2, 6, 1, 1, h, 4);
      break;
  }
  return v;
}

function headVoxels(p) {
  const v = [];
  box(v, -3, NECK_Y, -2, 6, 6, 5, p.skin);
  box(v, -4, NECK_Y + 2, -1, 1, 2, 2, p.skin, 3);
  box(v, 3, NECK_Y + 2, -1, 1, 2, 2, p.skin, 3);

  for (const h of hairVoxels(p)) v.push(h);

  box(v, -2, NECK_Y + 2, 2, 1, 2, 1, 0x14161c, 2);
  box(v, 1, NECK_Y + 2, 2, 1, 2, 1, 0x14161c, 2);
  box(v, -1, NECK_Y + 1, 2, 2, 1, 1, darker(p.skin, 0.72), 3);

  if (p.accessory === "glasses") {
    box(v, -3, NECK_Y + 4, 2, 6, 1, 1, 0x8d96a6, 3);
    box(v, -3, NECK_Y + 1, 2, 1, 1, 1, 0x8d96a6, 3);
    box(v, 2, NECK_Y + 1, 2, 1, 1, 1, 0x8d96a6, 3);
    box(v, -1, NECK_Y + 3, 2, 2, 1, 1, 0x8d96a6, 3);
    box(v, -3, NECK_Y + 3, 1, 1, 1, 1, 0x8d96a6, 3);
    box(v, 2, NECK_Y + 3, 1, 1, 1, 1, 0x8d96a6, 3);
  }
  return v;
}

function armVoxels(p, side) {
  const v = [];
  const x = side < 0 ? -4 : 2;
  box(v, x, HIP_Y + 2, -1, 2, 4, 3, p.jacket);
  box(v, x, HIP_Y + 1, -1, 2, 1, 3, p.jacketAlt, 4);
  box(v, x, HAND_Y - 1, -1, 2, 2, 2, p.skin, 3);
  if (p.outfit === "tech" || p.outfit === "hoodie") {
    box(v, x, SHOULDER_Y - 1, -1, 2, 1, 3, p.trim, 5);
  }
  if (p.outfit === "field") {
    box(v, x, SHOULDER_Y - 1, 1, 2, 1, 1, p.trim, 5);
  }
  return v;
}

function thighVoxels(p, side) {
  const v = [];
  const x = side < 0 ? -3 : 0;
  box(v, x, KNEE_Y, -1, 3, 4, 3, p.pants);
  box(v, x, HIP_Y - 1, -1, 3, 1, 3, darker(p.pants, 0.86), 4);
  return v;
}

function shinVoxels(p, side) {
  const v = [];
  const x = side < 0 ? -3 : 0;
  box(v, x, 1, -1, 3, 4, 3, darker(p.pants, 0.92));
  box(v, x, 1, -1, 3, 1, 3, p.shoes, 4);
  box(v, x, 0, -2, 3, 1, 4, p.shoes, 4);
  box(v, x, 0, -2, 3, 1, 1, p.soles, 5);
  return v;
}

function propVoxels(p) {
  const v = [];
  const screen = (cells) => {
    box(v, 0, HAND_Y, 1, 5, 4, 1, 0x2a2f3a, 3);
    box(v, 1, HAND_Y + 1, 2, 3, 2, 1, 0xdfe3ea, 3);
    for (const [cx, cy, c] of cells) vox(v, 1 + cx, HAND_Y + 1 + cy, 2, c);
  };

  switch (p.prop) {
    case "folio":
      box(v, 0, HAND_Y - 1, 1, 4, 5, 1, 0x14161c, 3);
      box(v, 0, HAND_Y - 1, 2, 1, 5, 1, p.trim, 4);
      box(v, 3, HAND_Y - 1, 1, 1, 5, 1, 0x24272f, 3);
      break;
    case "kanban":
      screen([[0, 1, 0x4a90d9], [1, 1, 0x4ad991], [2, 1, 0xe8a34a], [0, 0, 0x8a94a4], [2, 0, 0x4ad991]]);
      break;
    case "flowchart":
      screen([[0, 1, 0x2b3240], [2, 1, 0x2b3240], [1, 0, 0x2b3240], [1, 1, 0x8a94a4]]);
      box(v, 0, HAND_Y - 2, 1, 4, 1, 3, 0xe9e7e0, 4);
      box(v, 0, HAND_Y - 3, 1, 4, 1, 3, p.trim, 6);
      break;
    case "mappad":
      box(v, 0, HAND_Y, 1, 5, 4, 1, 0x2a2f3a, 3);
      box(v, 1, HAND_Y + 1, 2, 3, 2, 1, 0x1d2a24, 3);
      vox(v, 1, HAND_Y + 2, 2, 0x6ad98a);
      vox(v, 2, HAND_Y + 1, 2, 0x6ad98a);
      vox(v, 3, HAND_Y + 2, 2, 0xd9d0a0);
      break;
    case "script":
      box(v, 0, HAND_Y, 1, 3, 5, 1, 0xf0ead6, 4);
      box(v, 0, HAND_Y + 3, 2, 3, 1, 1, 0xd9a06a, 5);
      box(v, 0, HAND_Y + 1, 2, 3, 1, 1, 0xc8bda0, 5);
      break;
    case "wireframe":
      screen([[0, 1, 0x8a94a4], [1, 1, 0x8a94a4], [2, 1, p.trim], [0, 0, 0x8a94a4], [1, 0, 0x8a94a4]]);
      box(v, -1, HAND_Y + 2, 1, 1, 3, 1, 0x1e2129, 4);
      break;
    case "codepad":
      box(v, 0, HAND_Y, 1, 5, 4, 1, 0x2a2f3a, 3);
      box(v, 1, HAND_Y + 1, 2, 3, 2, 1, 0x121821, 3);
      vox(v, 1, HAND_Y + 2, 2, 0x4ad991);
      vox(v, 2, HAND_Y + 2, 2, 0xe8c14a);
      vox(v, 1, HAND_Y + 1, 2, 0x4a90d9);
      vox(v, 3, HAND_Y + 1, 2, 0xe07a4a);
      break;
    case "gamepad":
      box(v, 0, HAND_Y, 1, 5, 2, 2, 0x3a414f, 3);
      box(v, 0, HAND_Y - 1, 1, 1, 1, 2, 0x2b3038, 3);
      box(v, 4, HAND_Y - 1, 1, 1, 1, 2, 0x2b3038, 3);
      vox(v, 1, HAND_Y + 2, 2, 0xd6dae2);
      vox(v, 3, HAND_Y + 2, 2, p.trim);
      break;
    case "statustower":
      box(v, 1, HAND_Y, 1, 3, 6, 2, 0x22262e, 3);
      vox(v, 2, HAND_Y + 4, 3, 0x4ad991);
      vox(v, 2, HAND_Y + 2, 3, 0xe8c14a);
      vox(v, 2, HAND_Y, 3, 0xd95555);
      box(v, 1, HAND_Y + 1, 3, 1, 4, 1, 0x3a414f, 3);
      break;
    case "nodepad":
      screen([[0, 1, 0x6ad95a], [2, 1, 0x4a90d9], [1, 0, 0xe07a4a], [2, 0, 0x8a94a4]]);
      box(v, -1, HAND_Y + 2, 1, 1, 3, 1, 0x1e2129, 4);
      vox(v, -1, HAND_Y + 5, 1, p.trim);
      break;
    case "palettepad":
      screen([[0, 1, 0x6ad95a], [1, 1, 0xdfe3ea], [2, 1, 0xe0503a], [1, 0, 0x4a90d9]]);
      box(v, 0, HAND_Y - 1, 1, 5, 1, 1, 0x2a2f3a, 3);
      vox(v, 0, HAND_Y - 1, 2, 0x4ac8e0);
      vox(v, 1, HAND_Y - 1, 2, 0xe8c14a);
      vox(v, 2, HAND_Y - 1, 2, 0xe0503a);
      vox(v, 3, HAND_Y - 1, 2, 0x6ad95a);
      break;
    case "checklist":
      box(v, 0, HAND_Y - 1, 1, 4, 6, 1, 0xdfe3ea, 3);
      box(v, 0, HAND_Y + 4, 1, 4, 1, 1, 0x3a414f, 3);
      for (let i = 0; i < 3; i++) {
        vox(v, 1, HAND_Y + 3 - i, 2, 0x2aa96b);
        box(v, 2, HAND_Y + 3 - i, 2, 2, 1, 1, 0x8a94a4, 4);
      }
      vox(v, 1, HAND_Y, 2, p.trim);
      break;
    case "recorder":
      box(v, 1, HAND_Y, 1, 3, 5, 2, 0x24272f, 3);
      box(v, 1, HAND_Y + 3, 3, 3, 1, 1, 0x6ad98a, 4);
      box(v, 2, HAND_Y + 5, 1, 1, 2, 2, 0x8d96a6, 3);
      vox(v, 2, HAND_Y + 1, 3, p.trim);
      break;
  }
  return v;
}

const KEY_BIAS = 128;
const KEY_SPAN = 256;

function key(x, y, z) {
  return ((x + KEY_BIAS) * KEY_SPAN + (y + KEY_BIAS)) * KEY_SPAN + (z + KEY_BIAS);
}

const OPEN_FACE = 17;
const AO_STEP = 0.042;
const AO_MAX = 0.32;

function occlude(hex, amount) {
  const f = 1 - amount;
  return (
    (Math.round(((hex >> 16) & 255) * f) << 16) |
    (Math.round(((hex >> 8) & 255) * f) << 8) |
    Math.round((hex & 255) * f)
  );
}

export function shell(voxels) {
  const filled = new Set();
  for (const v of voxels) filled.add(key(v.x, v.y, v.z));
  const kept = new Set();
  const out = [];
  for (let i = voxels.length - 1; i >= 0; i--) {
    const v = voxels[i];
    const k = key(v.x, v.y, v.z);
    if (kept.has(k)) continue;
    kept.add(k);
    if (
      filled.has(key(v.x + 1, v.y, v.z)) && filled.has(key(v.x - 1, v.y, v.z)) &&
      filled.has(key(v.x, v.y + 1, v.z)) && filled.has(key(v.x, v.y - 1, v.z)) &&
      filled.has(key(v.x, v.y, v.z + 1)) && filled.has(key(v.x, v.y, v.z - 1))
    ) continue;

    let near = 0;
    for (let dx = -1; dx <= 1; dx++)
      for (let dy = -1; dy <= 1; dy++)
        for (let dz = -1; dz <= 1; dz++) {
          if (!dx && !dy && !dz) continue;
          if (filled.has(key(v.x + dx, v.y + dy, v.z + dz))) near++;
        }

    if (near <= OPEN_FACE) {
      out.push(v);
      continue;
    }
    const dim = Math.min(AO_MAX, (near - OPEN_FACE) * AO_STEP);
    out.push({ x: v.x, y: v.y, z: v.z, c: occlude(v.c, dim) });
  }
  return out;
}

function part(voxels, pivot) {
  return { voxels: shell(voxels), pivot };
}

const partsByRole = new Map();

export function buildCharacterParts(roleId) {
  const known = partsByRole.get(roleId);
  if (known) return known;
  const built = characterParts(roleId);
  partsByRole.set(roleId, built);
  return built;
}

function characterParts(roleId) {
  const p = look(roleId);
  return {
    torso: part(torsoVoxels(p), [0, HIP_Y, 0]),
    head: part(headVoxels(p), [0, NECK_Y, 0]),
    armL: part(armVoxels(p, -1), [-3, SHOULDER_Y, 0]),
    armR: part(armVoxels(p, 1), [3, SHOULDER_Y, 0]),
    thighL: part(thighVoxels(p, -1), [-1.5, HIP_Y, 0]),
    thighR: part(thighVoxels(p, 1), [1.5, HIP_Y, 0]),
    shinL: part(shinVoxels(p, -1), [-1.5, KNEE_Y, 0]),
    shinR: part(shinVoxels(p, 1), [1.5, KNEE_Y, 0]),
    prop: part(propVoxels(p), [3, SHOULDER_Y, 0]),
  };
}

export function characterBounds() {
  return { w: 8, h: 23, d: 6 };
}

export function buildDesk(tint, variant = -1) {
  const v = [];
  box(v, 0, 0, 0, 10, 1, 6, 0x3a3f4c, 4);
  box(v, 0, 1, 0, 10, 6, 1, 0x2e3340, 4);
  box(v, 0, 1, 5, 10, 6, 1, 0x2e3340, 4);
  box(v, 0, 7, 0, 10, 1, 6, 0x4a5060, 4);
  box(v, 2, 8, 1, 6, 4, 1, 0x14161c, 3);
  box(v, 3, 9, 2, 4, 2, 1, tint, 6);
  box(v, 2, 8, 4, 6, 1, 2, 0x2a2e38, 4);
  if (variant === 0) {
    box(v, 8, 8, 4, 1, 2, 1, 0xd9d3c4, 3);
    box(v, 0, 8, 3, 2, 1, 2, 0xe7e3d7, 3);
  } else if (variant === 1) {
    box(v, 8, 8, 1, 2, 3, 1, 0x14161c, 3);
    box(v, 8, 9, 2, 2, 1, 1, tint, 6);
    box(v, 0, 8, 4, 2, 1, 1, 0xc4573f, 4);
  } else if (variant === 2) {
    box(v, 0, 8, 2, 2, 3, 2, 0x3f6b3a, 7);
    box(v, 8, 8, 4, 1, 2, 1, 0xcf7a45, 3);
  }
  return v;
}

export function buildChair(tint) {
  const v = [];
  box(v, 0, 0, 0, 5, 1, 5, 0x23272f, 3);
  box(v, 2, 1, 2, 1, 3, 1, 0x2b3038, 3);
  box(v, 0, 4, 0, 5, 1, 5, 0x33394a, 4);
  box(v, 0, 5, 0, 5, 5, 1, 0x3a4152, 4);
  box(v, 0, 8, 0, 5, 1, 1, tint, 6);
  return v;
}

export function buildPlant() {
  const v = [];
  box(v, 1, 0, 1, 4, 3, 4, 0x6b4b38, 4);
  box(v, 1, 3, 1, 4, 1, 4, 0x4a3428, 3);
  box(v, 2, 4, 2, 2, 4, 2, 0x3f6b3a, 6);
  box(v, 0, 6, 1, 6, 3, 4, 0x4a8046, 10);
  box(v, 1, 9, 2, 4, 2, 2, 0x56914f, 10);
  box(v, 2, 11, 2, 2, 1, 2, 0x62a05a, 8);
  return v;
}

export function buildCabinet(tint) {
  const v = [];
  box(v, 0, 0, 0, 12, 7, 4, 0x2f343f, 4);
  box(v, 0, 7, 0, 12, 1, 4, 0x3a414f, 4);
  for (let i = 0; i < 3; i++) box(v, 1 + i * 4, 1, 4, 3, 5, 1, 0x262b34, 3);
  for (let i = 0; i < 3; i++) box(v, 2 + i * 4, 3, 5, 1, 1, 1, tint, 6);
  box(v, 1, 8, 1, 2, 2, 2, 0xb8ae95, 6);
  box(v, 8, 8, 1, 3, 1, 2, 0x8a94a4, 6);
  return v;
}

export function buildWhiteboard(tint) {
  const v = [];
  box(v, 0, 0, 0, 14, 9, 1, 0x2a2f3a, 3);
  box(v, 1, 1, 1, 12, 7, 1, 0xd9dbe0, 3);
  box(v, 2, 6, 2, 5, 1, 1, tint, 8);
  box(v, 2, 4, 2, 8, 1, 1, 0x7f8794, 8);
  box(v, 2, 2, 2, 6, 1, 1, 0x7f8794, 8);
  return v;
}

export function buildServerRack() {
  const v = [];
  box(v, 0, 0, 0, 6, 14, 5, 0x1c2028, 4);
  for (let i = 0; i < 6; i++) {
    box(v, 1, 1 + i * 2, 5, 4, 1, 1, 0x2b313c, 3);
    vox(v, 1, 1 + i * 2, 5, i % 2 ? 0x4ad991 : 0x4a90d9);
    vox(v, 4, 1 + i * 2, 5, i % 3 ? 0x4ad991 : 0xd9c24a);
  }
  return v;
}

export function buildEasel(tint) {
  const v = [];
  box(v, 0, 0, 2, 1, 10, 1, 0x8a6a48, 4);
  box(v, 6, 0, 2, 1, 10, 1, 0x8a6a48, 4);
  box(v, 0, 5, 0, 7, 1, 3, 0x8a6a48, 4);
  box(v, 0, 6, 1, 7, 7, 1, 0xe6e2d8, 3);
  box(v, 1, 8, 2, 3, 3, 1, tint, 10);
  box(v, 4, 7, 2, 2, 2, 1, 0xd97a4a, 10);
  return v;
}

export function buildSofa(tint) {
  const v = [];
  box(v, 0, 0, 0, 14, 3, 6, 0x2e3440, 4);
  box(v, 0, 3, 0, 14, 4, 2, 0x39404f, 4);
  box(v, 0, 3, 2, 2, 2, 4, 0x39404f, 4);
  box(v, 12, 3, 2, 2, 2, 4, 0x39404f, 4);
  box(v, 3, 3, 3, 3, 1, 2, tint, 8);
  box(v, 8, 3, 3, 3, 1, 2, tint, 8);
  return v;
}

export function buildTestBench(tint) {
  const v = [];
  box(v, 0, 0, 0, 10, 5, 5, 0x2c313c, 4);
  box(v, 0, 5, 0, 10, 1, 5, 0x3a4150, 4);
  box(v, 1, 6, 1, 3, 3, 1, 0x14161c, 3);
  box(v, 5, 6, 1, 3, 3, 1, 0x14161c, 3);
  box(v, 2, 7, 2, 1, 1, 1, tint, 8);
  box(v, 6, 7, 2, 1, 1, 1, 0xd95555, 8);
  return v;
}

export function buildMeetingTable(tint) {
  const v = [];
  box(v, 0, 0, 0, 16, 1, 9, 0x3b4250, 4);
  box(v, 1, 1, 1, 14, 4, 7, 0x2c313c, 3);
  box(v, 0, 5, 0, 16, 1, 9, 0x4a5262, 5);
  box(v, 3, 6, 3, 3, 1, 2, tint, 8);
  box(v, 10, 6, 4, 3, 1, 2, 0xd8d2c0, 6);
  box(v, 7, 6, 2, 2, 2, 2, 0x8a94a4, 6);
  return v;
}

export function buildCoffeeBar() {
  const v = [];
  box(v, 0, 0, 0, 14, 6, 5, 0x33394a, 4);
  box(v, 0, 6, 0, 14, 1, 5, 0x59627a, 5);
  box(v, 1, 7, 1, 3, 4, 3, 0x22272f, 3);
  box(v, 2, 8, 4, 1, 2, 1, 0xd97a4a, 8);
  box(v, 6, 7, 1, 2, 3, 2, 0x8a94a4, 5);
  box(v, 9, 7, 2, 1, 2, 1, 0xd8d2c0, 6);
  box(v, 11, 7, 2, 1, 2, 1, 0xd8d2c0, 6);
  return v;
}

export function buildWaterCooler() {
  const v = [];
  box(v, 0, 0, 0, 4, 8, 4, 0xd4dae4, 4);
  box(v, 1, 8, 1, 2, 4, 2, 0x5fa8c8, 10);
  box(v, 1, 3, 4, 2, 2, 1, 0x2b3038, 3);
  return v;
}

export function buildShelf() {
  const v = [];
  box(v, 0, 0, 0, 12, 16, 4, 0x39404f, 4);
  for (let s = 0; s < 4; s++) {
    box(v, 1, 3 + s * 4, 1, 10, 1, 3, 0x2c313c, 3);
    for (let b = 0; b < 5; b++) {
      const h = 2 + ((s + b) % 2);
      box(v, 1 + b * 2, 4 + s * 4, 1, 1, h, 3, [0xc8a24a, 0x8a6fd1, 0x4a90d9, 0xd16f9a, 0x4fb3a5][(s * 5 + b) % 5], 12);
    }
  }
  return v;
}

export function buildBoxes() {
  const v = [];
  box(v, 0, 0, 0, 5, 4, 5, 0x8a7048, 6);
  box(v, 0, 4, 1, 5, 3, 4, 0x9a7c52, 6);
  box(v, 5, 0, 1, 4, 3, 4, 0x7d6540, 6);
  box(v, 1, 7, 2, 3, 1, 2, 0x6a5638, 5);
  return v;
}
