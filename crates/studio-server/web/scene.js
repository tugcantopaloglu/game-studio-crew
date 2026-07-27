import * as THREE from "/vendor/three.module.js";
import {
  buildDesk, buildChair, buildPlant, buildCabinet,
  buildWhiteboard, buildServerRack, buildEasel, buildSofa, buildTestBench,
  buildMeetingTable, buildCoffeeBar, buildWaterCooler, buildShelf, buildBoxes,
  characterBounds, shell,
} from "/voxel.js";
import { buildAvatar, voxelMesh, VOX } from "/avatar.js";
import { budget } from "/perf.js";

export { VOX, voxelMesh };
const PICK_MATERIAL = new THREE.MeshBasicMaterial({ visible: false });
export const WALL_H = 2.6;
export const WALL_T = 0.16;
export const LEVEL_H = 3.6;
const DOOR_W = 2.6;
const LOW_WALL_H = 1.15;
const CAP_H = 0.09;
const SIDES = ["-z", "+z", "-x", "+x"];

const lamberts = new Map();
const basics = new Map();

function lambert(color) {
  let m = lamberts.get(color);
  if (!m) {
    m = new THREE.MeshLambertMaterial({ color });
    lamberts.set(color, m);
  }
  return m;
}

function basic(color, opacity) {
  const key = opacity === undefined ? color : color + ":" + opacity;
  let m = basics.get(key);
  if (!m) {
    m = opacity === undefined
      ? new THREE.MeshBasicMaterial({ color, toneMapped: false })
      : new THREE.MeshBasicMaterial({ color, transparent: true, opacity, depthWrite: false, toneMapped: false });
    basics.set(key, m);
  }
  return m;
}

const vertexLit = new THREE.MeshLambertMaterial({ vertexColors: true });
const RING_GEO = new THREE.RingGeometry(0.4, 0.56, 40);
const SHADE_GEO = new THREE.ConeGeometry(0.2, 0.18, 14, 1, true);
const BULB_GEO = new THREE.SphereGeometry(0.07, 10, 8);
const POOL_GEO = new THREE.CircleGeometry(0.75, 24);
const CONE_GEO = new THREE.CylinderGeometry(0.16, 0.75, 2.2, 18, 1, true);
const SHADE_MATERIAL = new THREE.MeshLambertMaterial({ color: 0x161c26, side: THREE.DoubleSide });

let pickGeo = null;
function proxyGeo() {
  if (!pickGeo) {
    const cb = characterBounds();
    pickGeo = new THREE.BoxGeometry(cb.w * VOX, cb.h * VOX, cb.d * VOX);
  }
  return pickGeo;
}

export const FAMILY_TINT = {
  leadership: 0xffc84a, design: 0xa678ff, engineering: 0x4aa8ff,
  art: 0xff6fae, audio: 0x3ce0c8, qa: 0xff8a3c,
};

const SCREEN_STYLE = {
  leadership: "chart", production: "chart", design: "swatch",
  engineering: "code", art: "swatch", audio: "wave",
  qa: "code", infra: "graph",
};

const tileGeo = new THREE.BoxGeometry(1, 0.2, 1);
tileGeo.setAttribute(
  "color",
  new THREE.BufferAttribute(new Float32Array(tileGeo.attributes.position.count * 3).fill(1), 3)
);

let rng = 1;
function rand() {
  rng = (rng * 1664525 + 1013904223) % 4294967296;
  return rng / 4294967296;
}

function place(raw, x, y, z, rotY = 0) {
  const voxels = shell(raw);
  const mesh = voxelMesh(voxels);
  const w = Math.max(...voxels.map((v) => v.x)) + 1;
  const d = Math.max(...voxels.map((v) => v.z)) + 1;
  mesh.scale.setScalar(VOX);
  mesh.position.set((-w * VOX) / 2, 0, (-d * VOX) / 2);
  const g = new THREE.Group();
  g.add(mesh);
  g.position.set(x, y, z);
  g.rotation.y = rotY;
  return { group: g, mesh };
}

function drawScreen(x, style, tint, data, crew) {
  const hex = '#' + tint.toString(16).padStart(6, '0');
  x.fillStyle = '#080b11'; x.fillRect(0, 0, 256, 160);
  x.fillStyle = hex; x.fillRect(0, 0, 256, 3);
  x.font = '600 15px ui-monospace, monospace';
  x.fillStyle = '#8892a4';
  const money = (n) => String.fromCharCode(36) + n.toFixed(4);

  if (style === 'chart') {
    x.fillText('RUN SPEND', 10, 24);
    x.font = '700 32px ui-monospace, monospace'; x.fillStyle = hex;
    x.fillText(money(data.spend || 0), 10, 60);
    x.font = '600 14px ui-monospace, monospace'; x.fillStyle = '#8892a4';
    x.fillText((data.tokens || 0).toLocaleString() + ' tokens', 10, 84);
    x.fillText((data.events || 0) + ' events', 10, 104);
    x.fillStyle = hex;
    (data.history || []).forEach((v, i) => {
      const h = Math.max(2, v * 42);
      x.fillRect(10 + i * 9, 150 - h, 6, h);
    });
  } else if (style === 'code') {
    x.fillText('EVENT STREAM', 10, 24);
    x.font = '600 13px ui-monospace, monospace';
    const feed = (data.feed || []).slice(-8);
    feed.forEach((e, i) => {
      x.fillStyle = e.bad ? '#ff6b6b' : i === feed.length - 1 ? hex : '#6d7686';
      x.fillText(String(e.seq).padStart(3) + '  ' + e.type.slice(0, 22), 10, 46 + i * 14);
    });
  } else if (style === 'swatch') {
    x.fillText('DEPARTMENT', 10, 24);
    crew.slice(0, 5).forEach((c, i) => {
      const y = 42 + i * 22;
      x.fillStyle = c.color; x.fillRect(10, y - 10, 12, 12);
      x.fillStyle = '#aab3c2'; x.font = '600 13px ui-monospace, monospace';
      x.fillText(c.role.replace(/_/g, ' ').slice(0, 20), 30, y);
      x.fillStyle = '#5d6675';
      x.fillText(c.tokens ? String(c.tokens) : '-', 210, y);
    });
  } else if (style === 'wave') {
    x.fillText('ACTIVITY', 10, 24);
    x.strokeStyle = hex; x.lineWidth = 2; x.beginPath();
    (data.history || []).forEach((v, i) => {
      const px = 10 + i * 9, py = 118 - v * 58;
      i ? x.lineTo(px, py) : x.moveTo(px, py);
    });
    x.stroke();
    x.font = '700 24px ui-monospace, monospace'; x.fillStyle = hex;
    x.fillText((data.events || 0) + ' ev', 10, 150);
  } else {
    x.fillText('CACHE HIT', 10, 24);
    const pct = data.cacheHit;
    x.font = '700 40px ui-monospace, monospace';
    x.fillStyle = pct === null || pct === undefined ? '#5d6675' : hex;
    x.fillText(pct === null || pct === undefined ? '--' : Math.round(pct) + '%', 10, 72);
    x.fillStyle = '#1a2130'; x.fillRect(10, 90, 236, 14);
    x.fillStyle = hex; x.fillRect(10, 90, 236 * ((pct || 0) / 100), 14);
    x.font = '600 13px ui-monospace, monospace'; x.fillStyle = '#8892a4';
    x.fillText((data.cacheRead || 0).toLocaleString() + ' read', 10, 124);
    x.fillText((data.cacheWrite || 0).toLocaleString() + ' written', 10, 144);
  }
}

const screens = [];
const screenByKey = new Map();
const SCREEN_W = 2.0;
const SCREEN_H = 1.25;
const screenFrameGeo = new THREE.BoxGeometry(SCREEN_W + 0.16, SCREEN_H + 0.16, 0.08);
const screenPanelGeo = new THREE.PlaneGeometry(SCREEN_W, SCREEN_H);
let screenPayload = null;
let screenCursor = 0;

function screenSurface(style, tint, department) {
  const key = style + ":" + tint + ":" + department;
  const known = screenByKey.get(key);
  if (known) return known;

  const canvas = document.createElement("canvas");
  canvas.width = 256;
  canvas.height = 160;
  const tex = new THREE.CanvasTexture(canvas);
  tex.magFilter = THREE.NearestFilter;
  tex.colorSpace = THREE.SRGBColorSpace;

  const surface = {
    ctx: canvas.getContext("2d"),
    tex,
    material: new THREE.MeshBasicMaterial({ map: tex, toneMapped: false }),
    style, tint, department,
    dirty: true,
    drawn: null,
  };
  screenByKey.set(key, surface);
  screens.push(surface);
  return surface;
}

function screenSignature(style, data, crew) {
  if (style === "code") {
    const feed = data.feed || [];
    return feed.length ? feed.length + ":" + feed[feed.length - 1].seq : "0";
  }
  if (style === "swatch") {
    let sig = "";
    for (const c of crew) sig += c.role + c.color + c.tokens + ";";
    return sig;
  }
  if (style === "graph") return data.cacheRead + "|" + data.cacheWrite;
  return data.events + "|" + data.tokens + "|" + data.spend;
}

function mountWall(room, cx, cz, doorSide, glassSide, avoid = []) {
  const order = ["-z", "+z", "-x", "+x"];
  const usable = order.filter((k) => k !== doorSide && k !== glassSide && !avoid.includes(k));
  const key = usable[0] || "+z";

  const x0 = room.x - cx, z0 = room.y - cz;
  const x1 = x0 + room.w, z1 = z0 + room.h;
  const inset = WALL_T / 2 + 0.1;

  switch (key) {
    case "-z":
      return { key, along: "x", a0: x0, a1: x1, fixed: z0 + inset, rot: 0 };
    case "+z":
      return { key, along: "x", a0: x0, a1: x1, fixed: z1 - inset, rot: Math.PI };
    case "-x":
      return { key, along: "z", a0: z0, a1: z1, fixed: x0 + inset, rot: Math.PI / 2 };
    default:
      return { key, along: "z", a0: z0, a1: z1, fixed: x1 - inset, rot: -Math.PI / 2 };
  }
}

function wallScreens(parent, room, cx, cz, tint, doorSide, glassSide, avoid = []) {
  const style = SCREEN_STYLE[room.department] || "chart";
  const m = mountWall(room, cx, cz, doorSide, glassSide, avoid);
  const span = m.a1 - m.a0;
  const count = span > 15 ? 3 : 2;

  const surface = screenSurface(style, tint, room.department);

  for (let i = 0; i < count; i++) {
    const t = (i + 1) / (count + 1);
    const along = m.a0 + span * t;

    const px = m.along === "x" ? along : m.fixed;
    const pz = m.along === "x" ? m.fixed : along;

    const group = new THREE.Group();
    group.position.set(px, 1.68, pz);
    group.rotation.y = m.rot;
    parent.add(group);

    const frame = new THREE.Mesh(screenFrameGeo, lambert(0x11151d));
    frame.position.z = -0.03;
    group.add(frame);

    const panel = new THREE.Mesh(screenPanelGeo, surface.material);
    panel.position.z = 0.03;
    group.add(panel);
  }
}

export function refreshScreens(data) {
  screenPayload = data;
  for (const s of screens) s.dirty = true;
  return screens.length;
}

export function paintScreens(limit) {
  if (!screenPayload || !screens.length) return 0;
  let painted = 0;
  let looked = 0;
  while (looked < screens.length && painted < limit) {
    const s = screens[screenCursor];
    screenCursor = (screenCursor + 1) % screens.length;
    looked++;
    if (!s.dirty) continue;
    s.dirty = false;
    const crew = (screenPayload.crewByDept && screenPayload.crewByDept[s.department]) || [];
    const sig = screenSignature(s.style, screenPayload, crew);
    if (sig === s.drawn) continue;
    s.drawn = sig;
    drawScreen(s.ctx, s.style, s.tint, screenPayload, crew);
    s.tex.needsUpdate = true;
    painted++;
  }
  return painted;
}

export function screenCount() {
  return screens.length;
}

function neonEdge(parent, room, cx, cz, tint) {
  const mat = basic(tint);
  const x0 = room.x - cx, z0 = room.y - cz;
  const x1 = x0 + room.w, z1 = z0 + room.h;
  const t = 0.16, y = 0.23, half = t / 2;
  const segs = [
    [(x0 + x1) / 2, z0 + half, room.w, t],
    [(x0 + x1) / 2, z1 - half, room.w, t],
    [x0 + half, (z0 + z1) / 2, t, room.h - t * 2],
    [x1 - half, (z0 + z1) / 2, t, room.h - t * 2],
  ];
  for (const [px, pz, w, d] of segs) {
    const m = new THREE.Mesh(new THREE.BoxGeometry(w, 0.1, d), mat);
    m.position.set(px, y, pz);
    parent.add(m);
    const halo = new THREE.Mesh(
      new THREE.BoxGeometry(w + 0.5, 0.02, d + 0.5),
      basic(tint, 0.16)
    );
    halo.position.set(px, 0.215, pz);
    parent.add(halo);
  }
}

function wallSegment(parent, x, z, w, d, color) {
  const m = new THREE.Mesh(new THREE.BoxGeometry(w, WALL_H, d), lambert(color));
  m.position.set(x, WALL_H / 2, z);
  m.castShadow = true;
  m.receiveShadow = true;
  parent.add(m);
}

function partitionSegment(parent, x, z, w, d, color, tint) {
  const body = new THREE.Mesh(new THREE.BoxGeometry(w, LOW_WALL_H, d), lambert(color));
  body.position.set(x, LOW_WALL_H / 2, z);
  body.castShadow = true;
  body.receiveShadow = true;
  parent.add(body);

  const cap = new THREE.Mesh(
    new THREE.BoxGeometry(w + 0.06, CAP_H, d + 0.06),
    lambert(shade(0x4c5568, tint, 0.3))
  );
  cap.position.set(x, LOW_WALL_H + CAP_H / 2, z);
  cap.castShadow = true;
  parent.add(cap);
}

const glassMat = new THREE.MeshLambertMaterial({
  color: 0x7fb4d8, transparent: true, opacity: 0.16, depthWrite: false,
});

function glassSegment(parent, x, z, w, d, tint) {
  const pane = new THREE.Mesh(new THREE.BoxGeometry(w, WALL_H - 0.5, d), glassMat);
  pane.position.set(x, (WALL_H - 0.5) / 2 + 0.25, z);
  parent.add(pane);

  for (const y of [0.12, WALL_H - 0.12]) {
    const rail = new THREE.Mesh(
      new THREE.BoxGeometry(w, 0.24, d),
      lambert(y > 1 ? 0x2b3242 : tint)
    );
    rail.position.set(x, y, z);
    rail.castShadow = y > 1;
    parent.add(rail);
  }
}

function buildWalls(parent, room, cx, cz, doorSide, glassSide, tint, openThrough = [], low = []) {
  const x0 = room.x - cx, z0 = room.y - cz;
  const x1 = x0 + room.w, z1 = z0 + room.h;
  const mx = (x0 + x1) / 2, mz = (z0 + z1) / 2;
  const half = WALL_T / 2;
  const sides = [
    { key: "-z", x: mx, z: z0 + half, w: room.w, d: WALL_T, axis: "x", shade: shade(0x39435a, tint, 0.16) },
    { key: "+z", x: mx, z: z1 - half, w: room.w, d: WALL_T, axis: "x", shade: shade(0x272e3d, tint, 0.10) },
    { key: "-x", x: x0 + half, z: mz, w: WALL_T, d: room.h - WALL_T * 2, axis: "z", shade: shade(0x313949, tint, 0.13) },
    { key: "+x", x: x1 - half, z: mz, w: WALL_T, d: room.h - WALL_T * 2, axis: "z", shade: shade(0x2a313f, tint, 0.10) },
  ];

  for (const s of sides) {
    const draw = (px, pz, pw, pd) => {
      if (s.key === glassSide) return glassSegment(parent, px, pz, pw, pd, tint);
      if (low.includes(s.key)) return partitionSegment(parent, px, pz, pw, pd, s.shade, tint);
      return wallSegment(parent, px, pz, pw, pd, s.shade);
    };

    if (s.key !== doorSide && !openThrough.includes(s.key)) { draw(s.x, s.z, s.w, s.d); continue; }

    const span = s.axis === "x" ? s.w : s.d;
    const side = (span - DOOR_W) / 2;
    if (s.axis === "x") {
      draw(s.x - (DOOR_W / 2 + side / 2), s.z, side, s.d);
      draw(s.x + (DOOR_W / 2 + side / 2), s.z, side, s.d);
    } else {
      draw(s.x, s.z - (DOOR_W / 2 + side / 2), s.w, side);
      draw(s.x, s.z + (DOOR_W / 2 + side / 2), s.w, side);
    }
  }
}

function buildShell(parent, floor, cx, cz) {
  const w = floor.width, h = floor.height;
  const t = 0.5;
  const foot = -0.4, sill = 0.95, head = 2.35, cap = WALL_H + 0.1;
  const frame = lambert(0x171b24);
  const segs = [
    [0, -h / 2 - t / 2, w + t * 2, t],
    [0, h / 2 + t / 2, w + t * 2, t],
    [-w / 2 - t / 2, 0, t, h + t * 2],
    [w / 2 + t / 2, 0, t, h + t * 2],
  ];
  for (const [px, pz, sw, sd] of segs) {
    const base = new THREE.Mesh(new THREE.BoxGeometry(sw, sill - foot, sd), frame);
    base.position.set(px, (foot + sill) / 2, pz);
    base.castShadow = true;
    base.receiveShadow = true;
    parent.add(base);

    const lintel = new THREE.Mesh(new THREE.BoxGeometry(sw, cap - head, sd), frame);
    lintel.position.set(px, (head + cap) / 2, pz);
    lintel.castShadow = true;
    parent.add(lintel);

    const along = sw > sd;
    const pane = new THREE.Mesh(
      new THREE.BoxGeometry(along ? sw : sd * 0.4, head - sill, along ? sd * 0.4 : sd),
      glassMat
    );
    pane.position.set(px, (sill + head) / 2, pz);
    parent.add(pane);

    const span = along ? sw : sd;
    const bays = Math.max(2, Math.round(span / 9));
    for (let i = 1; i < bays; i++) {
      const f = i / bays - 0.5;
      const mull = new THREE.Mesh(
        new THREE.BoxGeometry(along ? 0.22 : sw, head - sill, along ? sd : 0.22),
        frame
      );
      mull.position.set(px + (along ? f * sw : 0), (sill + head) / 2, pz + (along ? 0 : f * sd));
      mull.castShadow = true;
      parent.add(mull);
    }

    const strip = new THREE.Mesh(
      new THREE.BoxGeometry(sw + 0.1, 0.16, sd + 0.1),
      basic(0x5fd8ff)
    );
    strip.position.set(px, -0.12, pz);
    parent.add(strip);
  }
}

function buildGround(parent, floor) {
  const span = Math.max(floor.width, floor.height);
  const ground = new THREE.Mesh(
    new THREE.BoxGeometry(span * 8, 0.6, span * 8),
    lambert(0x0b0e16)
  );
  ground.position.y = -1.5;
  parent.add(ground);
}

function podFacing(desk, room) {
  if (!room) return 0;
  const col = Math.round((desk.x - room.x - 1) / 3);
  return col % 2 === 0 ? 0 : Math.PI;
}

function facesTheShell(room, floor) {
  const reach = 2.5;
  const out = [];
  if (room.x <= reach) out.push("-x");
  if (floor.width - (room.x + room.w) <= reach) out.push("+x");
  if (room.y <= reach) out.push("-z");
  if (floor.height - (room.y + room.h) <= reach) out.push("+z");
  return out;
}

function partitionSides(room, floor) {
  const solid = facesTheShell(room, floor);
  return SIDES.filter((k) => !solid.includes(k));
}

function doorSideFor(room, cx, cz) {
  const dx = room.x + room.w / 2 - cx;
  const dz = room.y + room.h / 2 - cz;
  if (Math.abs(dx) > Math.abs(dz)) return dx > 0 ? "-x" : "+x";
  return dz > 0 ? "-z" : "+z";
}

function shade(base, tint, amount) {
  const b = new THREE.Color(base), t = new THREE.Color(tint);
  return b.lerp(t, amount).getHex();
}

function insideRect(rect, x, y) {
  return x >= rect.x && x < rect.x + rect.w && y >= rect.y && y < rect.y + rect.h;
}

function tileNoise(x, z) {
  const n = Math.sin(x * 127.1 + z * 311.7) * 43758.5453;
  return n - Math.floor(n);
}

function loose(raw, parent, x, z) {
  const turn = (tileNoise(x * 3.7 + 11.3, z * 5.9 + 7.1) - 0.5) * 0.5;
  parent.add(place(raw, x, 0.2, z, turn).group);
}

function deskVariant(x, z) {
  return Math.floor(tileNoise(x * 1.7 + 3.3, z * 2.9 + 5.1) * 3) % 3;
}

function rug(parent, x, z, w, d, tint) {
  const trim = new THREE.Mesh(
    new THREE.BoxGeometry(w + 0.3, 0.02, d + 0.3),
    lambert(shade(0x3a3129, tint, 0.12))
  );
  trim.position.set(x, 0.21, z);
  trim.receiveShadow = true;
  parent.add(trim);

  const pile = new THREE.Mesh(
    new THREE.BoxGeometry(w, 0.03, d),
    lambert(shade(0x483d33, tint, 0.08))
  );
  pile.position.set(x, 0.216, z);
  pile.receiveShadow = true;
  parent.add(pile);
}

function checkerFloor(parent, room, cx, cz, tint, hole = null) {
  const mesh = new THREE.InstancedMesh(tileGeo, vertexLit, room.w * room.h);
  mesh.receiveShadow = true;
  const m = new THREE.Matrix4();
  const c = new THREE.Color();
  let i = 0;
  for (let ix = 0; ix < room.w; ix++)
    for (let iz = 0; iz < room.h; iz++) {
      if (hole && insideRect(hole, room.x + ix, room.y + iz)) continue;
      m.makeTranslation(room.x - cx + ix + 0.5, 0.1, room.y - cz + iz + 0.5);
      mesh.setMatrixAt(i, m);
      c.setHex((ix + iz) % 2
        ? shade(0x373f4d, tint, 0.12)
        : shade(0x343c49, tint, 0.10));
      c.multiplyScalar(0.955 + tileNoise(room.x + ix, room.y + iz) * 0.09);
      mesh.setColorAt(i, c);
      i++;
    }
  mesh.count = i;
  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  parent.add(mesh);
}

function poi(x, z, tx, tz, emoji) {
  return { x, z, face: Math.atan2(tx - x, tz - z), emoji };
}

function roomProps(parent, room, cx, cz, tint) {
  const rx = room.x - cx, rz = room.y - cz;
  const back = rz + 0.62;
  const far = rx + room.w - 0.9;
  const pois = [];
  const lounge = { x: rx + room.w * 0.5, z: rz + room.h - 1.9 };

  rug(parent, lounge.x, lounge.z, Math.min(room.w - 3, 6.4), 3.2, tint);
  loose(buildPlant(), parent, rx + 0.75, rz + room.h - 0.8);
  loose(buildPlant(), parent, far, rz + room.h - 0.8);
  parent.add(place(buildWaterCooler(), rx + 0.7, 0.2, back).group);
  pois.push(poi(rx + 0.7, back + 0.6, rx + 0.7, back, "\u{1F4A7}"));

  switch (room.department) {
    case "leadership":
      parent.add(place(buildSofa(tint), rx + room.w * 0.5, 0.2, rz + room.h - 1.4, Math.PI).group);
      pois.push(poi(rx + room.w * 0.5, rz + room.h - 2.2, rx + room.w * 0.5, rz + room.h - 1.4, "\u{1F60C}"));
      parent.add(place(buildMeetingTable(tint), rx + room.w * 0.62, 0.2, rz + room.h * 0.55).group);
      parent.add(place(buildCoffeeBar(), far - 1.4, 0.2, back).group);
      pois.push(poi(far - 1.4, back + 0.65, far - 1.4, back, "☕"));
      break;
    case "production":
      parent.add(place(buildMeetingTable(tint), rx + room.w * 0.62, 0.2, rz + room.h - 1.5).group);
      parent.add(place(buildShelf(), far, 0.2, back).group);
      parent.add(place(buildCoffeeBar(), rx + 2.4, 0.2, back).group);
      pois.push(poi(rx + 2.4, back + 0.65, rx + 2.4, back, "☕"));
      break;
    case "design":
      parent.add(place(buildWhiteboard(tint), rx + room.w * 0.32, 1.05, back).group);
      pois.push(poi(rx + room.w * 0.32, back + 0.7, rx + room.w * 0.32, back, "\u{1F4A1}"));
      parent.add(place(buildShelf(), far, 0.2, back).group);
      parent.add(place(buildMeetingTable(tint), rx + room.w * 0.55, 0.2, rz + room.h - 1.5).group);
      break;
    case "engineering":
      parent.add(place(buildWhiteboard(tint), rx + room.w * 0.3, 1.05, back).group);
      pois.push(poi(rx + room.w * 0.3, back + 0.7, rx + room.w * 0.3, back, "\u{1F4A1}"));
      parent.add(place(buildServerRack(), far, 0.2, back).group);
      loose(buildBoxes(), parent, rx + 1.2, rz + room.h - 1.2);
      parent.add(place(buildSofa(tint), lounge.x - 1.3, 0.2, lounge.z + 0.5, Math.PI).group);
      parent.add(place(buildMeetingTable(tint), lounge.x + 1.3, 0.2, lounge.z).group);
      pois.push(poi(lounge.x - 1.3, lounge.z - 0.4, lounge.x - 1.3, lounge.z + 0.5, "\u{1F60C}"));
      break;
    case "art":
      parent.add(place(buildEasel(tint), rx + room.w * 0.3, 0.2, rz + room.h - 1.6).group);
      pois.push(poi(rx + room.w * 0.3, rz + room.h - 2.3, rx + room.w * 0.3, rz + room.h - 1.6, "\u{1F3A8}"));
      parent.add(place(buildEasel(tint), rx + room.w * 0.45, 0.2, rz + room.h - 1.6, 0.3).group);
      parent.add(place(buildShelf(), far, 0.2, back).group);
      break;
    case "audio":
      parent.add(place(buildCabinet(tint), far, 0.2, back).group);
      parent.add(place(buildSofa(tint), rx + room.w * 0.5, 0.2, rz + room.h - 1.4, Math.PI).group);
      pois.push(poi(rx + room.w * 0.5, rz + room.h - 2.2, rx + room.w * 0.5, rz + room.h - 1.4, "\u{1F3B5}"));
      break;
    case "qa":
      parent.add(place(buildTestBench(tint), rx + room.w * 0.35, 0.2, back).group);
      pois.push(poi(rx + room.w * 0.35, back + 0.65, rx + room.w * 0.35, back, "\u{1F3AE}"));
      parent.add(place(buildTestBench(tint), rx + room.w * 0.62, 0.2, back).group);
      loose(buildBoxes(), parent, far, rz + room.h - 1.2);
      parent.add(place(buildSofa(tint), lounge.x, 0.2, lounge.z + 0.5, Math.PI).group);
      parent.add(place(buildMeetingTable(tint), lounge.x, 0.2, lounge.z - 0.9).group);
      pois.push(poi(lounge.x, lounge.z - 0.3, lounge.x, lounge.z + 0.5, "\u{1F60C}"));
      break;
    case "infra":
      for (let i = 0; i < 5; i++) {
        parent.add(place(buildServerRack(), rx + 2.2 + i * 0.62, 0.2, back).group);
      }
      pois.push(poi(rx + 2.2 + 1.24, back + 0.6, rx + 2.2 + 1.24, back, "\u{1F5A5}"));
      parent.add(place(buildServerRack(), far, 0.2, rz + room.h - 1.3).group);
      loose(buildBoxes(), parent, rx + 1.1, rz + room.h - 1.2);
      parent.add(place(buildSofa(tint), lounge.x, 0.2, lounge.z + 0.5, Math.PI).group);
      parent.add(place(buildMeetingTable(tint), lounge.x, 0.2, lounge.z - 0.9).group);
      pois.push(poi(lounge.x, lounge.z - 0.3, lounge.x, lounge.z + 0.5, "\u{1F60C}"));
      break;
  }
  return pois;
}

function buildLobby(parent, lobby, cx, cz, tier) {
  const rx = lobby.x - cx, rz = lobby.y - cz;
  const mid = { x: rx + lobby.w / 2, z: rz + lobby.h / 2 };

  const mesh = new THREE.InstancedMesh(tileGeo, vertexLit, lobby.w * lobby.h);
  mesh.receiveShadow = true;
  const m = new THREE.Matrix4();
  const c = new THREE.Color();
  let i = 0;
  for (let ix = 0; ix < lobby.w; ix++)
    for (let iz = 0; iz < lobby.h; iz++) {
      m.makeTranslation(rx + ix + 0.5, 0.1, rz + iz + 0.5);
      mesh.setMatrixAt(i, m);
      const ring = Math.min(ix, iz, lobby.w - 1 - ix, lobby.h - 1 - iz);
      c.setHex(ring === 0 ? 0x424b5d : (ix + iz) % 2 ? 0x454d60 : 0x424a5a);
      c.multiplyScalar(0.955 + tileNoise(lobby.x + ix, lobby.y + iz) * 0.09);
      mesh.setColorAt(i, c);
      i++;
    }
  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  parent.add(mesh);

  const pois = [];
  parent.add(place(buildCoffeeBar(), mid.x - 2.4, 0.2, rz + 1.0).group);
  pois.push(poi(mid.x - 2.4, rz + 1.65, mid.x - 2.4, rz + 1.0, "☕"));
  parent.add(place(buildMeetingTable(0xa678ff), mid.x - 2.2, 0.2, mid.z + 0.4).group);
  parent.add(place(buildSofa(0xffc84a), mid.x - 4.1, 0.2, mid.z + 0.4, Math.PI / 2).group);
  pois.push(poi(mid.x - 3.4, mid.z + 0.4, mid.x - 2.2, mid.z + 0.4, "\u{1F60C}"));
  parent.add(place(buildSofa(0x3ce0c8), mid.x - 0.3, 0.2, mid.z + 0.4, -Math.PI / 2).group);
  pois.push(poi(mid.x - 1.0, mid.z + 0.4, mid.x - 2.2, mid.z + 0.4, "\u{1F60C}"));

  for (const [px, pz] of [
    [rx + 0.9, rz + 0.9], [rx + lobby.w - 0.9, rz + 0.9],
    [rx + 0.9, rz + lobby.h - 0.9], [rx + lobby.w - 0.9, rz + lobby.h - 0.9],
    [mid.x + 3.6, mid.z], [mid.x - 4.4, mid.z + 1.9],
  ]) {
    loose(buildPlant(), parent, px, pz);
  }

  parent.add(place(buildWaterCooler(), mid.x + 4.6, 0.2, rz + 1.0).group);
  loose(buildBoxes(), parent, rx + lobby.w - 1.6, rz + lobby.h - 1.9);

  if (tier.roomLights) {
    const lamp = new THREE.PointLight(0xffe9c8, 46, 24, 1.6);
    lamp.position.set(mid.x, WALL_H + 0.3, mid.z);
    parent.add(lamp);
  }

  const sign = makeLabel("LOBBY", 0xe8eef8, 1.5);
  sign.position.set(mid.x, 0.03, rz + 1.9);
  sign.rotation.x = -Math.PI / 2;
  parent.add(sign);

  return pois;
}

function buildMeetingRoom(parent, room, floor, cx, cz, tier) {
  const tint = 0x6fa8d1;
  const mx = room.x - cx + room.w / 2, mz = room.y - cz + room.h / 2;
  const low = partitionSides(room, floor).filter((k) => k !== "-z" && k !== "-x");

  checkerFloor(parent, room, cx, cz, tint);
  buildWalls(parent, room, cx, cz, "-x", "-x", tint, [], low);
  neonEdge(parent, room, cx, cz, tint);
  parent.add(place(buildMeetingTable(tint), mx, 0.2, mz).group);

  for (let i = 0; i < 6; i++) {
    const ang = (i / 6) * Math.PI * 2 + Math.PI / 6;
    const px = mx + Math.cos(ang) * 1.6, pz = mz + Math.sin(ang) * 1.25;
    parent.add(place(buildChair(0x3a4152), px, 0.2, pz, Math.atan2(mx - px, mz - pz)).group);
  }
  parent.add(place(buildWhiteboard(tint), mx, 1.05, room.y - cz + 0.62).group);
  loose(buildPlant(), parent, room.x - cx + 0.8, room.y - cz + room.h - 0.8);
  loose(buildPlant(), parent, room.x - cx + room.w - 0.9, room.y - cz + 0.9);

  if (tier.roomLights) {
    const lamp = new THREE.PointLight(0xfff0d8, 34, 18, 1.7);
    lamp.position.set(mx, WALL_H - 0.45, mz);
    parent.add(lamp);
  }

  const sign = makeLabel("WAR ROOM", tint, 1.2);
  sign.position.set(mx, 0.03, room.y - cz + room.h - 1.4);
  sign.rotation.x = -Math.PI / 2;
  parent.add(sign);

  return doorPoint(room, cx, cz, "-x");
}

function buildExtraRoom(parent, room, floor, cx, cz, hole = null) {
  const rx = room.x - cx, rz = room.y - cz;
  const low = partitionSides(room, floor);
  const mx = rx + room.w / 2, mz = rz + room.h / 2;
  const far = rx + room.w - 0.9;
  const back = rz + 0.62;

  switch (room.department) {
    case "break": {
      const tint = 0xe0a55a;
      checkerFloor(parent, room, cx, cz, tint);
      buildWalls(parent, room, cx, cz, doorSideFor(room, cx, cz), null, tint, [], low);
      neonEdge(parent, room, cx, cz, tint);
      rug(parent, mx - 1.2, mz - 0.1, 5.2, 3.6, tint);
      for (let i = 0; i < 3; i++) {
        const tx = rx + 3.0 + i * 2.6, tz = rz + room.h - 2.6;
        parent.add(place(buildMeetingTable(tint), tx, 0.2, tz).group);
        parent.add(place(buildChair(0x3a4152), tx - 1.0, 0.2, tz, Math.PI / 2).group);
        parent.add(place(buildChair(0x3a4152), tx + 1.0, 0.2, tz, -Math.PI / 2).group);
      }
      parent.add(place(buildCoffeeBar(), rx + 2.2, 0.2, back).group);
      parent.add(place(buildSofa(tint), mx - 1.2, 0.2, mz + 0.9, Math.PI).group);
      parent.add(place(buildSofa(0x3ce0c8), mx - 1.2, 0.2, mz - 1.1).group);
      parent.add(place(buildMeetingTable(tint), mx + 2.2, 0.2, mz).group);
      parent.add(place(buildWaterCooler(), far, 0.2, back).group);
      loose(buildPlant(), parent, rx + 0.75, rz + room.h - 0.8);
      loose(buildPlant(), parent, far, rz + room.h - 0.8);
      const sign = makeLabel("BREAK ROOM", tint, 1.1);
      sign.position.set(mx, 0.03, rz + room.h - 1.3);
      sign.rotation.x = -Math.PI / 2;
      parent.add(sign);
      break;
    }
    case "archive": {
      const tint = 0x8a94a4;
      checkerFloor(parent, room, cx, cz, tint);
      buildWalls(parent, room, cx, cz, doorSideFor(room, cx, cz), null, tint, [], low);
      parent.add(place(buildShelf(), rx + 1.4, 0.2, back).group);
      parent.add(place(buildShelf(), rx + 3.4, 0.2, back).group);
      for (let row = 0; row < 3; row++) {
        for (let s = 0; s < 4; s++) {
          parent.add(place(buildShelf(), rx + 1.9 + s * 1.35, 0.2, rz + 2.7 + row * 2.3).group);
        }
      }
      parent.add(place(buildCabinet(tint), far, 0.2, back).group);
      parent.add(place(buildServerRack(), far, 0.2, rz + room.h - 1.3).group);
      loose(buildBoxes(), parent, rx + 1.2, rz + room.h - 1.2);
      loose(buildBoxes(), parent, mx, mz);
      const sign = makeLabel("ARCHIVE", tint, 1.1);
      sign.position.set(mx, 0.03, rz + room.h - 1.3);
      sign.rotation.x = -Math.PI / 2;
      parent.add(sign);
      break;
    }
    case "landing": {
      const tint = 0x97a2b6;
      checkerFloor(parent, room, cx, cz, tint, hole);
      parent.add(place(buildSofa(0xffc84a), mx + 2.6, 0.2, mz - 0.9, Math.PI).group);
      parent.add(place(buildSofa(0x6fa8d1), mx + 2.6, 0.2, mz + 1.1).group);
      loose(buildPlant(), parent, rx + 0.9, rz + 0.9);
      loose(buildPlant(), parent, rx + room.w - 0.9, rz + room.h - 0.9);
      parent.add(place(buildWaterCooler(), mx + 5.4, 0.2, rz + 0.9).group);
      const sign = makeLabel("STUDIO LOFT", tint, 1.3);
      sign.position.set(mx, 0.03, rz + 2.1);
      sign.rotation.x = -Math.PI / 2;
      parent.add(sign);
      break;
    }
  }
}

function loftExtent(floor, cx, cz) {
  const on = [
    ...(floor.rooms || []).filter((r) => (r.level || 0) === 1),
    ...(floor.extras || []).filter((r) => (r.level || 0) === 1),
  ];
  if (!on.length) return null;
  const pad = 2;
  let x0 = Infinity, x1 = -Infinity, z0 = Infinity, z1 = -Infinity;
  for (const r of on) {
    x0 = Math.min(x0, r.x - pad); x1 = Math.max(x1, r.x + r.w + pad);
    z0 = Math.min(z0, r.y - pad); z1 = Math.max(z1, r.y + r.h + pad);
  }
  return { x0: x0 - cx, x1: x1 - cx, z0: z0 - cz, z1: z1 - cz };
}

function buildUpperDeck(parent, floor, cx, cz) {
  const mat = lambert(0x333b4a);
  const span = loftExtent(floor, cx, cz);
  const w = floor.width + 4, h = floor.height + 4;
  const x0 = span ? span.x0 : -w / 2, x1 = span ? span.x1 : w / 2;
  const z0 = span ? span.z0 : -h / 2, z1 = span ? span.z1 : h / 2;
  const a = floor.atrium;
  const ax0 = a ? a.x - cx : 0, ax1 = a ? ax0 + a.w : 0;
  const az0 = a ? a.y - cz : 0, az1 = a ? az0 + a.h : 0;

  const bands = a
    ? [
        [x0, x1, z0, az0],
        [x0, x1, az1, z1],
        [x0, ax0, az0, az1],
        [ax1, x1, az0, az1],
      ]
    : [[x0, x1, z0, z1]];

  for (const [bx0, bx1, bz0, bz1] of bands) {
    const bw = bx1 - bx0, bd = bz1 - bz0;
    if (bw <= 0.001 || bd <= 0.001) continue;
    const slab = new THREE.Mesh(new THREE.BoxGeometry(bw, 0.2, bd), mat);
    slab.position.set((bx0 + bx1) / 2, 0, (bz0 + bz1) / 2);
    slab.receiveShadow = true;
    parent.add(slab);
  }
}

function buildAtriumRail(parent, a, cx, cz) {
  const x0 = a.x - cx, x1 = x0 + a.w, z0 = a.y - cz, z1 = z0 + a.h;
  const t = 0.11, half = t / 2;
  const kerb = 0.3, pane = 0.56;
  const edges = [
    [(x0 + x1) / 2, z0 - half, a.w + t * 2, t],
    [(x0 + x1) / 2, z1 + half, a.w + t * 2, t],
    [x0 - half, (z0 + z1) / 2, t, a.h],
    [x1 + half, (z0 + z1) / 2, t, a.h],
  ];

  for (const [px, pz, ew, ed] of edges) {
    const base = new THREE.Mesh(new THREE.BoxGeometry(ew, kerb, ed), lambert(0x252b37));
    base.position.set(px, 0.1 + kerb / 2, pz);
    base.castShadow = true;
    base.receiveShadow = true;
    parent.add(base);

    const glass = new THREE.Mesh(new THREE.BoxGeometry(ew, pane, ed), glassMat);
    glass.position.set(px, 0.1 + kerb + pane / 2, pz);
    parent.add(glass);

    const cap = new THREE.Mesh(
      new THREE.BoxGeometry(ew + 0.05, 0.06, ed + 0.05),
      lambert(0x3d4553)
    );
    cap.position.set(px, 0.1 + kerb + pane + 0.04, pz);
    cap.castShadow = true;
    parent.add(cap);
  }

  const drop = 0.7, lip = 0.12;
  const skirt = [
    [(x0 + x1) / 2, z0 + lip / 2, a.w + lip * 2, lip],
    [(x0 + x1) / 2, z1 - lip / 2, a.w + lip * 2, lip],
    [x0 + lip / 2, (z0 + z1) / 2, lip, a.h],
    [x1 - lip / 2, (z0 + z1) / 2, lip, a.h],
  ];
  for (const [px, pz, ew, ed] of skirt) {
    const face = new THREE.Mesh(new THREE.BoxGeometry(ew, drop, ed), lambert(0x171c25));
    face.position.set(px, -0.1 - drop / 2, pz);
    parent.add(face);
  }
}

function buildElevatorStop(parent, rect, cx, cz) {
  const mx = rect.x - cx + rect.w / 2, mz = rect.y - cz + rect.h / 2;
  const frame = lambert(0x1c222d);

  const backW = new THREE.Mesh(new THREE.BoxGeometry(rect.w, WALL_H, 0.14), frame);
  backW.position.set(mx, WALL_H / 2, mz - rect.h / 2 + 0.07);
  backW.castShadow = true;
  parent.add(backW);
  for (const sx of [-1, 1]) {
    const side = new THREE.Mesh(new THREE.BoxGeometry(0.14, WALL_H, rect.h), frame);
    side.position.set(mx + sx * (rect.w / 2 - 0.07), WALL_H / 2, mz);
    side.castShadow = true;
    parent.add(side);
  }
  const beam = new THREE.Mesh(new THREE.BoxGeometry(rect.w, 0.5, rect.h), frame);
  beam.position.set(mx, WALL_H - 0.25, mz);
  parent.add(beam);

  const doors = new THREE.Mesh(
    new THREE.BoxGeometry(rect.w - 0.5, WALL_H - 0.6, 0.06),
    lambert(0x39404f)
  );
  doors.position.set(mx, (WALL_H - 0.6) / 2, mz + rect.h / 2 - 0.03);
  parent.add(doors);

  const lampStrip = new THREE.Mesh(
    new THREE.BoxGeometry(rect.w - 0.4, 0.12, 0.06),
    basic(0x4ad991)
  );
  lampStrip.position.set(mx, WALL_H - 0.4, mz + rect.h / 2 + 0.01);
  parent.add(lampStrip);

  const sign = makeLabel("LIFT", 0x4ad991, 0.9);
  sign.position.set(mx, 0.03, mz + rect.h / 2 + 0.6);
  sign.rotation.x = -Math.PI / 2;
  parent.add(sign);
}

export function buildOffice(floor, scene) {
  rng = 12345;
  screens.length = 0;
  screenByKey.clear();
  screenCursor = 0;
  screenPayload = null;
  const tier = budget();
  const cx = floor.width / 2, cz = floor.height / 2;
  const avatars = new Map();
  const world = new THREE.Group();
  scene.add(world);
  const fixtures = new THREE.Group();
  world.add(fixtures);

  const corridor = new THREE.Mesh(
    new THREE.BoxGeometry(floor.width + 4, 0.2, floor.height + 4),
    lambert(0x2a303c)
  );
  corridor.receiveShadow = true;
  fixtures.add(corridor);

  const skirt = new THREE.Mesh(
    new THREE.BoxGeometry(floor.width + 5, 0.55, floor.height + 5),
    lambert(0x12151c)
  );
  skirt.position.y = -0.28;
  fixtures.add(skirt);

  const levels = [new THREE.Group(), new THREE.Group()];
  levels[1].position.y = LEVEL_H;
  for (const g of levels) fixtures.add(g);


  if ((floor.levels || 1) > 1) {
    buildUpperDeck(levels[1], floor, cx, cz);
    if (floor.atrium) {
      buildAtriumRail(levels[1], floor.atrium, cx, cz);
      const wx = floor.atrium.x - cx + floor.atrium.w / 2;
      const wz = floor.atrium.y - cz + floor.atrium.h / 2;
      if (tier.roomLights) {
        const shaft = new THREE.PointLight(0xffe4bd, 42, 22, 1.5);
        shaft.position.set(wx, LEVEL_H - 0.7, wz);
        levels[0].add(shaft);

        const landing = (floor.extras || []).find((r) => r.department === "landing");
        const loft = new THREE.PointLight(0xffe9c8, 48, 32, 1.5);
        loft.position.set(
          landing ? landing.x - cx + landing.w / 2 : wx,
          WALL_H + 0.4,
          landing ? landing.y - cz + landing.h / 2 : wz
        );
        levels[1].add(loft);
      }
    }
  }

  const lobbyPois = floor.lobby ? buildLobby(levels[0], floor.lobby, cx, cz, tier) : [];
  const meetingDoor = floor.meeting ? buildMeetingRoom(levels[0], floor.meeting, floor, cx, cz, tier) : null;
  for (const extra of floor.extras || []) {
    buildExtraRoom(levels[extra.level || 0], extra, floor, cx, cz, floor.atrium);
  }
  if (floor.elevator) {
    for (const g of levels) buildElevatorStop(g, floor.elevator, cx, cz);
  }
  const lobbyRect = floor.lobby
    ? {
        x0: floor.lobby.x - cx + 1.6, x1: floor.lobby.x - cx + floor.lobby.w - 1.6,
        z0: floor.lobby.y - cz + (floor.elevator ? 3.6 : 1.6),
        z1: floor.lobby.y - cz + floor.lobby.h - 1.6,
      }
    : null;

  const roomsByDept = new Map();
  const doorsByDept = new Map();
  const doorSides = new Map();
  const poisByDept = new Map();
  for (const room of floor.rooms) {
    const side = doorSideFor(room, cx, cz);
    doorSides.set(room.department, side);
    doorsByDept.set(room.department, doorPoint(room, cx, cz, side));
  }

  for (const room of floor.rooms) {
    const tint = FAMILY_TINT[room.visual_family] || 0x4aa8ff;
    const rg = new THREE.Group();
    levels[room.level || 0].add(rg);
    roomsByDept.set(room.department, room);

    const door = doorSides.get(room.department);
    const glass = lobbyFacingSide(room, floor, cx, cz);
    const open = passThroughSides(room, cx, cz, doorsByDept, room.department);
    const low = partitionSides(room, floor).filter((k) => k !== glass);
    checkerFloor(rg, room, cx, cz, tint);
    buildWalls(rg, room, cx, cz, door, glass === door ? null : glass, tint, open, low);
    neonEdge(rg, room, cx, cz, tint);
    wallScreens(rg, room, cx, cz, tint, door, glass, open.concat(low));
    poisByDept.set(room.department, roomProps(rg, room, cx, cz, tint));

    const rx = room.x - cx, rz = room.y - cz;
    if (tier.roomLights) {
      const glow = new THREE.Color(0xfff0d8).lerp(new THREE.Color(tint), 0.2);
      const lamp = new THREE.PointLight(glow, 30, 17, 1.7);
      lamp.position.set(rx + room.w / 2, WALL_H - 0.45, rz + room.h / 2);
      rg.add(lamp);
    }

    for (const side of SIDES) {
      if (low.includes(side) || side === door) continue;
      const along = side === "-z" || side === "+z";
      const len = (along ? room.w : room.h) * 0.62;
      const fix = new THREE.Mesh(
        new THREE.BoxGeometry(along ? len : 0.2, 0.09, along ? 0.2 : len),
        basic(0xffeccd)
      );
      fix.position.set(
        side === "-x" ? rx + WALL_T + 0.13 : side === "+x" ? rx + room.w - WALL_T - 0.13 : rx + room.w / 2,
        WALL_H - 0.34,
        side === "-z" ? rz + WALL_T + 0.13 : side === "+z" ? rz + room.h - WALL_T - 0.13 : rz + room.h / 2
      );
      rg.add(fix);
    }

    const signY = low.includes("-z") ? LOW_WALL_H - 0.3 : WALL_H - 0.42;
    const sign = makeLabel(room.department.toUpperCase(), tint, 1.3);
    if (door === "-z") {
      sign.position.set(rx + room.w * 0.22, signY, rz + WALL_T + 0.02);
    } else {
      sign.position.set(rx + room.w / 2, signY, rz + WALL_T + 0.02);
    }
    rg.add(sign);
  }

  for (const s of floor.spares || []) {
    const tint = FAMILY_TINT[s.visual_family] || 0x4aa8ff;
    const g = new THREE.Group();
    g.position.set(s.x + s.w / 2 - cx, 0.2, s.y + s.h / 2 - cz);
    levels[s.level || 0].add(g);
    g.rotation.y = podFacing(s, roomsByDept.get(s.department));
    g.add(place(buildDesk(0x2c3240), 0, 0, 0.28).group);
    g.add(place(buildChair(tint), 0, 0, -0.42, rand() * 0.7 - 0.35).group);
  }

  for (const d of floor.desks) {
    const tint = FAMILY_TINT[d.visual_family] || 0x4aa8ff;
    const room = roomsByDept.get(d.department);

    const facing = podFacing(d, room);
    const fixed = new THREE.Group();
    fixed.position.set(d.x + d.w / 2 - cx, 0.2, d.y + d.h / 2 - cz);
    fixed.rotation.y = facing;
    levels[d.level || 0].add(fixed);
    fixed.add(place(buildDesk(tint, deskVariant(d.x, d.y)), 0, 0, 0.28).group);
    fixed.add(place(buildChair(tint), 0, 0, -0.42).group);

    const plate = makeLabel(d.role.replace(/_/g, " "), 0xbcc5d4, 0.6);
    plate.position.set(0, 0.03, 1.15);
    plate.rotation.x = -Math.PI / 2;
    fixed.add(plate);

    const person = new THREE.Group();
    const deckY = (d.level ? LEVEL_H : 0);
    const home = new THREE.Vector3(
      d.x + d.w / 2 - cx + Math.sin(facing) * -0.42,
      deckY + 0.22,
      d.y + d.h / 2 - cz + Math.cos(facing) * -0.42
    );
    person.position.copy(home);
    world.add(person);

    const ringMat = new THREE.MeshBasicMaterial({
      color: 0x394254, side: THREE.DoubleSide, transparent: true, opacity: 0.4,
    });
    const ring = new THREE.Mesh(RING_GEO, ringMat);
    ring.rotation.x = -Math.PI / 2;
    ring.position.y = -0.19;
    person.add(ring);

    const rig = buildAvatar(d.role, rand() * 10);
    person.add(rig.group);

    const proxy = new THREE.Mesh(proxyGeo(), PICK_MATERIAL);
    proxy.position.y = (characterBounds().h * VOX) / 2;
    proxy.visible = false;
    person.add(proxy);

    const lampY = 2.2;
    const lamp = new THREE.Group();
    lamp.visible = false;
    person.add(lamp);

    const shade = new THREE.Mesh(SHADE_GEO, SHADE_MATERIAL);
    shade.position.y = lampY + 0.1;
    lamp.add(shade);

    const bulb = new THREE.Mesh(
      BULB_GEO,
      new THREE.MeshBasicMaterial({ color: 0x4ad991, transparent: true, opacity: 0.95 })
    );
    bulb.position.y = lampY;
    lamp.add(bulb);

    const cone = new THREE.Mesh(
      CONE_GEO,
      new THREE.MeshBasicMaterial({
        color: 0x4ad991, transparent: true, opacity: 0.16,
        depthWrite: false, side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
      })
    );
    cone.position.y = lampY / 2;
    lamp.add(cone);

    const pool = new THREE.Mesh(
      POOL_GEO,
      new THREE.MeshBasicMaterial({
        color: 0x4ad991, transparent: true, opacity: 0.12,
        depthWrite: false, blending: THREE.AdditiveBlending,
      })
    );
    pool.rotation.x = -Math.PI / 2;
    pool.position.y = 0.015;
    lamp.add(pool);

    let alarm = null;
    let spot = null;
    if (tier.deskLights) {
      alarm = new THREE.PointLight(0xff3b30, 0, 3.4, 2);
      alarm.position.y = 1.55;
      person.add(alarm);

      spot = new THREE.SpotLight(0x4ad991, 0, 5, 0.55, 0.7, 1.5);
      spot.position.y = lampY;
      person.add(spot);
      person.add(spot.target);
    }

    const bubble = chatBubble();
    person.add(bubble.sprite);

    avatars.set(d.role, {
      person, body: rig.group, rig, hit: proxy, ringMat, alarm, lamp, bulb, cone, pool, spot,
      tier: d.tier, title: d.title, dept: d.department,
      home,
      deskLook: new THREE.Vector3(d.x + d.w / 2 - cx, deckY + 0.92, d.y + d.h / 2 - cz),
      homeFacing: facing,
      bounds: {
        x0: room.x - cx + 1.1, x1: room.x - cx + room.w - 1.1,
        z0: room.y - cz + 1.6, z1: room.y - cz + room.h - 1.1,
      },
      target: home.clone(),
      lobby: lobbyRect,
      door: doorsByDept.get(d.department) || home.clone(),
      route: floor.lobby ? routeToLobby(room, floor, cx, cz, doorsByDept) : [],
      path: [],
      inLobby: false,
      mode: "idle",
      meetingSeat: null,
      meetingFace: null,
      wait: rand() * 4,
      facing: facing,
      speed: 0,
      seed: rand() * 10,
      roomPois: poisByDept.get(d.department) || [],
      lobbyPois,
      bubble,
      visit: null,
      chatWith: null,
      chatUntil: 0,
    });
  }

  buildShell(levels[0], floor, cx, cz);
  buildGround(scene, floor);
  const table = floor.meeting
    ? new THREE.Vector3(
        floor.meeting.x - cx + floor.meeting.w / 2,
        0.22,
        floor.meeting.y - cz + floor.meeting.h / 2
      )
    : floor.lobby
      ? new THREE.Vector3(
          floor.lobby.x - cx + floor.lobby.w / 2 - 2.2,
          0.22,
          floor.lobby.y - cz + floor.lobby.h / 2 + 0.4
        )
      : new THREE.Vector3(0, 0.22, 0);
  const ambient = buildAmbient(world, floor, cx, cz, tier.ambientCrew);
  for (const a of ambient) a.lobbyPois = lobbyPois;
  buildBoard(levels[0], table);

  scene.updateMatrixWorld(true);
  scene.matrixAutoUpdate = false;
  world.matrixAutoUpdate = false;
  fixtures.matrixAutoUpdate = false;
  fixtures.matrixWorldAutoUpdate = false;

  return {
    world, fixtures, avatars, ambient, levels, meetingTable: table,
    meetingApproach: meetingDoor ? [meetingDoor] : [],
  };
}

function pointIn(rect, y) {
  return new THREE.Vector3(
    rect.x0 + rand() * Math.max(0.1, rect.x1 - rect.x0),
    y,
    rect.z0 + rand() * Math.max(0.1, rect.z1 - rect.z0)
  );
}

let boardMesh = null;
let boardCtx = null;
let boardTex = null;

export function buildBoard(parent, table) {
  const canvas = document.createElement("canvas");
  canvas.width = 340; canvas.height = 190;
  boardCtx = canvas.getContext("2d");
  boardTex = new THREE.CanvasTexture(canvas);
  boardTex.magFilter = THREE.NearestFilter;
  boardTex.colorSpace = THREE.SRGBColorSpace;

  const g = new THREE.Group();
  g.position.set(table.x, 0.2, table.z - 2.1);
  parent.add(g);

  const frame = new THREE.Mesh(new THREE.BoxGeometry(2.5, 1.5, 0.1), lambert(0x1a1f29));
  frame.position.y = 1.25;
  g.add(frame);

  const face = new THREE.Mesh(
    new THREE.PlaneGeometry(2.32, 1.32),
    new THREE.MeshBasicMaterial({ map: boardTex, transparent: true })
  );
  face.position.set(0, 1.25, 0.06);
  g.add(face);

  for (const x of [-1.2, 1.2]) {
    const leg = new THREE.Mesh(
      new THREE.BoxGeometry(0.09, 1.4, 0.09),
      lambert(0x2b3240)
    );
    leg.position.set(x, 0.7, 0);
    g.add(leg);
  }

  boardMesh = g;
  g.visible = false;
  return g;
}

let board = null;

function wrapAt(x, text, left, top, width, lineHeight, maxY) {
  let line = "", y = top;
  for (const w of String(text || "").split(/\s+/)) {
    if (!w) continue;
    if (line && (line + " " + w).length > width) {
      x.fillText(line, left, y);
      y += lineHeight;
      line = w;
      if (y > maxY) return y;
    } else {
      line = line ? line + " " + w : w;
    }
  }
  if (line && y <= maxY) {
    x.fillText(line, left, y);
    y += lineHeight;
  }
  return y;
}

function drawBoard() {
  if (!boardMesh || !boardCtx || !board) return;
  const x = boardCtx;

  x.fillStyle = "#e9ecf2";
  x.fillRect(0, 0, 340, 190);
  x.fillStyle = board.decided ? "#2aa96b" : "#c9502e";
  x.fillRect(0, 0, 340, 5);

  x.fillStyle = "#2b3240";
  x.font = "700 15px ui-monospace, monospace";
  x.fillText((board.kind || "meeting").toUpperCase(), 14, 28);

  x.font = "600 12px ui-monospace, monospace";
  x.fillStyle = "#3d4657";
  let y = wrapAt(x, board.topic, 14, 48, 40, 16, 80);

  x.strokeStyle = "#c8cdd8";
  x.lineWidth = 1;
  x.beginPath();
  x.moveTo(14, y);
  x.lineTo(326, y);
  x.stroke();
  y += 18;

  if (board.decided) {
    x.fillStyle = "#2aa96b";
    x.font = "700 10px ui-monospace, monospace";
    x.fillText("DECISION", 14, y);
    x.fillStyle = "#26303f";
    x.font = "600 12px ui-monospace, monospace";
    wrapAt(x, board.decided, 14, y + 17, 40, 15, 158);
  } else if (board.position) {
    x.fillStyle = "#c9502e";
    x.font = "700 10px ui-monospace, monospace";
    x.fillText(String(board.speaker || "").toUpperCase(), 14, y);
    x.fillStyle = "#4a5364";
    x.font = "500 12px ui-monospace, monospace";
    wrapAt(x, board.position, 14, y + 17, 40, 15, 158);
  } else {
    x.fillStyle = "#8a93a4";
    x.font = "500 12px ui-monospace, monospace";
    x.fillText("the room is sitting down", 14, y);
  }

  x.fillStyle = "#8d95a4";
  x.font = "600 10px ui-monospace, monospace";
  x.fillText(("chair: " + (board.chair || "-")).slice(0, 46), 14, 182);

  boardTex.needsUpdate = true;
  boardMesh.visible = true;
}

export function showBoard(kind, topic, participants, chair) {
  board = { kind, topic, participants, chair, speaker: null, position: null, decided: null };
  drawBoard();
}

export function boardSays(role, position) {
  if (!board) return;
  board.speaker = role;
  board.position = position;
  drawBoard();
}

export function boardDecided(claim) {
  if (!board) return;
  board.decided = claim;
  drawBoard();
}

export function hideBoard() {
  board = null;
  if (boardMesh) boardMesh.visible = false;
}

function chatBubble() {
  const c = document.createElement("canvas");
  c.width = 64; c.height = 64;
  const ctx = c.getContext("2d");
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  const sprite = new THREE.Sprite(
    new THREE.SpriteMaterial({ map: tex, transparent: true, depthTest: false })
  );
  sprite.scale.set(0.55, 0.55, 1);
  sprite.position.y = 2.05;
  sprite.visible = false;
  return {
    sprite,
    set(emoji) {
      ctx.clearRect(0, 0, 64, 64);
      ctx.font = "44px sans-serif";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(emoji, 32, 36);
      tex.needsUpdate = true;
    },
  };
}

function stopChat(a, now) {
  a.chatUntil = 0;
  a.chatWith = null;
  a.wait = now + 2 + rand() * 5;
  if (a.bubble) a.bubble.sprite.visible = false;
}

let nextMingle = 0;

export function mingle(list, now) {
  for (const a of list) {
    if (a.chatUntil && (!motion || now > a.chatUntil || a.meetingSeat || a.mode !== "idle")) {
      const b = a.chatWith;
      stopChat(a, now);
      if (b && b.chatWith === a) stopChat(b, now);
    }
  }
  if (!motion || now < nextMingle) return;
  nextMingle = now + 5 + rand() * 6;

  const free = list.filter(
    (a) => a.mode === "idle" && !a.meetingSeat && !a.chatWith && !a.path.length
  );
  const pool = [];
  const lobbyFree = free.filter((a) => a.inLobby);
  if (lobbyFree.length >= 2) pool.push(lobbyFree);
  const byRoom = new Map();
  for (const a of free) {
    if (a.inLobby || !a.dept) continue;
    if (!byRoom.has(a.dept)) byRoom.set(a.dept, []);
    byRoom.get(a.dept).push(a);
  }
  for (const g of byRoom.values()) if (g.length >= 2) pool.push(g);
  if (!pool.length) return;

  const group = pool[(rand() * pool.length) | 0];
  const i = (rand() * group.length) | 0;
  let j = (rand() * group.length) | 0;
  if (j === i) j = (i + 1) % group.length;
  const a = group[i], b = group[j];
  if (a === b) return;
  if (Math.abs(a.person.position.y - b.person.position.y) > 0.5) return;

  const mx = (a.person.position.x + b.person.position.x) / 2;
  const mz = (a.person.position.z + b.person.position.z) / 2;
  const until = now + 7 + rand() * 8;
  a.chatWith = b; b.chatWith = a;
  a.chatUntil = until; b.chatUntil = until;
  a.wait = until; b.wait = until;
  a.visit = null; b.visit = null;
  a.target.set(mx - 0.3, a.person.position.y, mz);
  b.target.set(mx + 0.3, b.person.position.y, mz);
}

function pathFrom(p, pts) {
  let best = 0, bd = Infinity;
  for (let i = 0; i < pts.length; i++) {
    const d = p.distanceTo(pts[i]);
    if (d < bd) { bd = d; best = i; }
  }
  return pts.slice(best);
}

export function seatAtTable(a, table, index, total, approach = []) {
  const angle = (index / Math.max(1, total)) * Math.PI * 2;
  a.meetingSeat = new THREE.Vector3(
    table.x + Math.cos(angle) * 1.15,
    table.y,
    table.z + Math.sin(angle) * 0.95
  );
  a.meetingFace = Math.atan2(table.x - a.meetingSeat.x, table.z - a.meetingSeat.z);
  a.meetingLook = new THREE.Vector3(table.x, table.y + 0.95, table.z);
  a.path = pathFrom(a.person.position, [
    ...a.route.map((v) => v.clone()),
    ...approach.map((v) => v.clone()),
    a.meetingSeat.clone(),
  ]);
  a.target.copy(a.path[0]);
  a.inLobby = true;
}

export function leaveTable(a, approach = []) {
  a.meetingSeat = null;
  a.meetingFace = null;
  a.meetingLook = null;
  a.inLobby = false;
  a.path = [
    ...approach.map((v) => v.clone()).reverse(),
    ...a.route.map((v) => v.clone()).reverse(),
    a.home.clone(),
  ];
  a.target.copy(a.path[0]);
}

let motion = true;

export function setMotion(on) {
  motion = !!on;
  return motion;
}

export function wanderStep(a, busy, dt, now) {
  if (!motion) {
    const rest = a.meetingSeat || a.home;
    if (rest) {
      a.person.position.copy(rest);
      a.target.copy(rest);
    }
    a.path.length = 0;
    a.inLobby = false;
    a.speed = 0;
    a.visit = null;
    if (a.bubble) a.bubble.sprite.visible = false;
    a.mode = a.meetingSeat ? "meeting" : a.homeFacing === undefined ? "idle" : "desk";
    if (a.meetingSeat && a.meetingFace !== null && a.meetingFace !== undefined) {
      a.facing = a.meetingFace;
    } else if (a.homeFacing !== undefined) {
      a.facing = a.homeFacing;
    }
    return false;
  }

  const p = a.person.position;
  const arrived = Math.hypot(p.x - a.target.x, p.z - a.target.z) < 0.14;
  if (arrived && Math.abs(p.y - a.target.y) > 0.5) p.y = a.target.y;

  if (a.meetingSeat) {
    if (a.mode !== "meeting") {
      a.mode = "meeting";
    }
    if (arrived && a.path.length) {
      a.path.shift();
      a.target.copy(a.path.length ? a.path[0] : a.meetingSeat);
    } else if (arrived && !a.path.length) {
      a.target.copy(a.meetingSeat);
    }
  } else if (busy) {
    if (a.mode !== "returning" && a.mode !== "desk") {
      a.mode = "returning";
      a.visit = null;
      a.path = a.inLobby
        ? pathFrom(p, [...a.route.map((v) => v.clone()).reverse(), a.home.clone()])
        : [a.home.clone()];
      a.inLobby = false;
      a.target.copy(a.path[0]);
    } else if (arrived) {
      if (a.path.length) a.path.shift();
      if (a.path.length) {
        a.target.copy(a.path[0]);
      } else {
        a.mode = "desk";
        a.target.copy(a.home);
      }
    }
  } else {
    if (a.mode === "meeting" || a.mode === "returning") a.mode = "idle";
    if (arrived) {
      if (a.path.length) {
        a.path.shift();
        if (a.path.length) a.target.copy(a.path[0]);
        else a.target.copy(pointIn(a.inLobby ? a.lobby : a.bounds, a.inLobby ? 0.22 : p.y));
      } else if (now > a.wait) {
        const goLobby = a.lobby && !a.inLobby && rand() < 0.35;
        const comeBack = a.inLobby && rand() < 0.45;
        a.mode = "idle";

        if (goLobby) {
          a.inLobby = true;
          a.visit = null;
          a.path = [...a.route.map((v) => v.clone()), pointIn(a.lobby, 0.22)];
          a.target.copy(a.path[0]);
        } else if (comeBack) {
          a.inLobby = false;
          a.visit = null;
          a.path = [...a.route.map((v) => v.clone()).reverse(), a.home.clone()];
          a.target.copy(a.path[0]);
        } else {
          const pois = a.inLobby ? a.lobbyPois : a.roomPois;
          if (pois && pois.length && rand() < 0.4) {
            a.visit = pois[(rand() * pois.length) | 0];
            a.target.set(a.visit.x, p.y, a.visit.z);
          } else {
            a.visit = null;
            a.target.copy(pointIn(a.inLobby ? a.lobby : a.bounds, a.inLobby ? 0.22 : p.y));
          }
        }
        a.wait = now + 2 + rand() * 7;
      }
    }
  }

  const dx = a.target.x - p.x;
  const dz = a.target.z - p.z;
  const dist = Math.hypot(dx, dz);
  const cruise = a.mode === "returning" ? 2.9 : a.mode === "meeting" ? 1.7 : 0.9;
  const want = dist > 0.06 ? Math.min(cruise, 0.22 + dist * 2.4) : 0;
  const gain = want > a.speed ? 3.6 : 7.0;
  a.speed += (want - a.speed) * Math.min(1, gain * dt);
  if (a.speed < 0.03) a.speed = 0;

  if (a.speed > 0 && dist > 0.001) {
    const step = Math.min(dist, a.speed * dt);
    p.x += (dx / dist) * step;
    p.z += (dz / dist) * step;
    if (dist > 0.16) a.facing = Math.atan2(dx, dz);
    if (a.bubble && a.bubble.sprite.visible) a.bubble.sprite.visible = false;
    return true;
  }

  if (a.meetingSeat && a.meetingFace !== null && a.meetingFace !== undefined) {
    a.facing = a.meetingFace;
  } else if (a.chatWith && a.chatUntil) {
    const q = a.chatWith.person.position;
    a.facing = Math.atan2(q.x - p.x, q.z - p.z);
    if (a.bubble && !a.bubble.sprite.visible) {
      a.bubble.set("\u{1F4AC}");
      a.bubble.sprite.visible = true;
    }
  } else if (a.visit && a.mode === "idle" && !busy) {
    a.facing = a.visit.face;
    if (a.bubble && !a.bubble.sprite.visible) {
      a.bubble.set(a.visit.emoji);
      a.bubble.sprite.visible = true;
    }
  } else if (a.mode === "desk" && a.homeFacing !== undefined) {
    a.facing = a.homeFacing;
  }
  return false;
}

export function avatarPose(a, ring, t) {
  const still = a.speed === 0;
  const seated = still && a.mode === "desk";
  const p = a.pose || (a.pose = {});
  p.speed = a.speed;
  p.facing = a.facing;
  p.sitting = seated;
  p.working = ring === "running";
  p.stuck = ring === "error" || ring === "blocked";
  p.talking = (a.talkUntil || 0) > t || (a.chatUntil || 0) > t;
  p.recline = seated && Math.sin(t * 0.17 + a.seed) > 0.84 ? 1 : 0;
  p.at = a.person.position;
  p.lookAt = !still ? null : seated ? a.deskLook : a.meetingSeat ? a.meetingLook : null;
  return p;
}

export function makeLabel(text, color, scale = 1) {
  const c = document.createElement("canvas");
  const fs = 44;
  const probe = c.getContext("2d");
  probe.font = `700 ${fs}px ui-monospace, monospace`;
  c.width = Math.ceil(probe.measureText(text).width) + 20;
  c.height = fs + 20;
  const ctx = c.getContext("2d");
  ctx.font = `700 ${fs}px ui-monospace, monospace`;
  ctx.fillStyle = "#" + color.toString(16).padStart(6, "0");
  ctx.textBaseline = "middle";
  ctx.fillText(text, 10, c.height / 2);
  const tex = new THREE.CanvasTexture(c);
  tex.magFilter = THREE.NearestFilter;
  tex.minFilter = THREE.LinearFilter;
  tex.generateMipmaps = false;
  tex.colorSpace = THREE.SRGBColorSpace;
  const mat = new THREE.MeshBasicMaterial({ map: tex, transparent: true, depthWrite: false });
  const h = 0.34 * scale;
  return new THREE.Mesh(new THREE.PlaneGeometry(h * (c.width / c.height), h), mat);
}

function wp(x, z, y = 0.22) {
  return new THREE.Vector3(x, y, z);
}

export function routeToLobby(room, floor, cx, cz, doorsByDept) {
  const own = doorsByDept.get(room.department);
  if (!own) return [];
  const half = (floor.corridor || 3) / 2;
  const L = floor.lobby;
  const lx0 = L.x - cx, lx1 = lx0 + L.w, lz0 = L.y - cz, lz1 = lz0 + L.h;
  const lmx = (lx0 + lx1) / 2, lmz = (lz0 + lz1) / 2;
  const side = doorSideFor(room, cx, cz);

  if (room.level && floor.elevator) {
    const uy = LEVEL_H + 0.22;
    const ex = floor.elevator.x - cx + floor.elevator.w / 2;
    const ez = floor.elevator.y - cz + floor.elevator.h / 2;
    const front = ez + floor.elevator.h / 2 + 0.5;
    const vx = own.x + (side === "+x" ? half : -half);
    return [
      wp(own.x, own.z, uy),
      wp(vx, own.z, uy),
      wp(vx, front, uy),
      wp(ex, front, uy),
      wp(ex, ez, uy),
      wp(ex, ez),
    ];
  }

  const pts = [own.clone()];

  if (side === "-x" || side === "+x") {
    const vx = own.x + (side === "+x" ? half : -half);
    pts.push(wp(vx, own.z));
    if (Math.abs(vx - lx0) <= half + 0.01) {
      if (Math.abs(own.z - lmz) > 0.5) pts.push(wp(vx, lmz));
      pts.push(wp(lx0 + 0.9, lmz));
    } else {
      const hz = own.z < lmz ? lz0 - half : lz1 + half;
      pts.push(wp(vx, hz), wp(lmx, hz), wp(lmx, hz < lmz ? lz0 + 0.9 : lz1 - 0.9));
    }
  } else {
    const hz = own.z + (side === "+z" ? half : -half);
    pts.push(wp(own.x, hz));
    if (Math.abs(own.x - lmx) > 0.5) pts.push(wp(lmx, hz));
    pts.push(wp(lmx, hz < lmz ? lz0 + 0.9 : lz1 - 0.9));
  }
  return pts;
}

function passThroughSides(room, cx, cz, doorsByDept, own) {
  const x0 = room.x - cx, z0 = room.y - cz;
  const x1 = x0 + room.w, z1 = z0 + room.h;
  const eps = 0.01, sides = [];
  for (const [dept, p] of doorsByDept) {
    if (dept === own) continue;
    if (Math.abs(p.x - x0) < eps && p.z > z0 && p.z < z1) sides.push("-x");
    else if (Math.abs(p.x - x1) < eps && p.z > z0 && p.z < z1) sides.push("+x");
    else if (Math.abs(p.z - z0) < eps && p.x > x0 && p.x < x1) sides.push("-z");
    else if (Math.abs(p.z - z1) < eps && p.x > x0 && p.x < x1) sides.push("+z");
  }
  return sides;
}

function doorPoint(room, cx, cz, side) {
  const x0 = room.x - cx, z0 = room.y - cz;
  const x1 = x0 + room.w, z1 = z0 + room.h;
  const mx = (x0 + x1) / 2, mz = (z0 + z1) / 2;
  switch (side) {
    case "-z": return new THREE.Vector3(mx, 0.22, z0);
    case "+z": return new THREE.Vector3(mx, 0.22, z1);
    case "-x": return new THREE.Vector3(x0, 0.22, mz);
    default: return new THREE.Vector3(x1, 0.22, mz);
  }
}

function lobbyFacingSide(room, floor, cx, cz) {
  const L = floor.lobby;
  if (!L) return null;
  if (room.y + room.h === L.y) return "+z";
  if (room.y === L.y + L.h) return "-z";
  if (room.x + room.w === L.x) return "+x";
  if (room.x === L.x + L.w) return "-x";
  return null;
}

const AMBIENT_PALETTES = [
  "producer", "ux_designer", "qa_engineer", "audio_designer",
  "tech_artist", "gameplay_engineer",
];

export function buildAmbient(parent, floor, cx, cz, count = 5) {
  if (!floor.lobby) return [];
  const L = floor.lobby;
  const rect = {
    x0: L.x - cx + 1.8, x1: L.x - cx + L.w - 1.8,
    z0: L.y - cz + (floor.elevator ? 3.6 : 1.8), z1: L.y - cz + L.h - 1.8,
  };

  const out = [];
  for (let i = 0; i < count; i++) {
    const person = new THREE.Group();
    const start = pointIn(rect, 0.22);
    person.position.copy(start);
    parent.add(person);

    const rig = buildAvatar(AMBIENT_PALETTES[i % AMBIENT_PALETTES.length], rand() * 10);
    person.add(rig.group);
    const bubble = chatBubble();
    person.add(bubble.sprite);

    out.push({
      person, body: rig.group, rig,
      home: start.clone(), bounds: rect, lobby: rect,
      target: start.clone(), route: [], path: [], inLobby: true,
      mode: "idle", meetingSeat: null, meetingFace: null,
      door: start.clone(),
      wait: rand() * 6, facing: 0, speed: 0, seed: rand() * 10,
      roomPois: [], lobbyPois: [], bubble,
      visit: null, chatWith: null, chatUntil: 0,
    });
  }
  return out;
}
