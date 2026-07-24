import * as THREE from "/vendor/three.module.js";
import {
  buildCharacter, buildDesk, buildChair, buildPlant, buildCabinet,
  buildWhiteboard, buildServerRack, buildEasel, buildSofa, buildTestBench,
  buildMeetingTable, buildCoffeeBar, buildWaterCooler, buildShelf, buildBoxes,
  characterBounds,
} from "/voxel.js";

export const VOX = 0.085;
const PICK_MATERIAL = new THREE.MeshBasicMaterial({ visible: false });
export const WALL_H = 2.6;
export const WALL_T = 0.16;
export const LEVEL_H = 3.6;
const DOOR_W = 2.6;

export const FAMILY_TINT = {
  leadership: 0xffc84a, design: 0xa678ff, engineering: 0x4aa8ff,
  art: 0xff6fae, audio: 0x3ce0c8, qa: 0xff8a3c,
};

const SCREEN_STYLE = {
  leadership: "chart", production: "chart", design: "swatch",
  engineering: "code", art: "swatch", audio: "wave",
  qa: "code", infra: "graph",
};

const cube = new THREE.BoxGeometry(1, 1, 1);
const tileGeo = new THREE.BoxGeometry(1, 0.2, 1);
for (const g of [cube, tileGeo]) {
  const n = g.attributes.position.count;
  g.setAttribute("color", new THREE.BufferAttribute(new Float32Array(n * 3).fill(1), 3));
}

let rng = 1;
function rand() {
  rng = (rng * 1664525 + 1013904223) % 4294967296;
  return rng / 4294967296;
}

export function voxelMesh(voxels, opts = {}) {
  const mesh = new THREE.InstancedMesh(
    cube,
    new THREE.MeshLambertMaterial({ vertexColors: true }),
    voxels.length
  );
  mesh.castShadow = opts.castShadow !== false;
  mesh.receiveShadow = true;
  const m = new THREE.Matrix4();
  const c = new THREE.Color();
  voxels.forEach((v, i) => {
    m.makeTranslation(v.x + 0.5, v.y + 0.5, v.z + 0.5);
    mesh.setMatrixAt(i, m);
    mesh.setColorAt(i, c.setHex(v.c));
  });
  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  return mesh;
}

function place(voxels, x, y, z, rotY = 0) {
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

function drawScreen(x, style, tint, data) {
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
    (data.crew || []).slice(0, 5).forEach((c, i) => {
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

  for (let i = 0; i < count; i++) {
    const w = 2.0, h = 1.25;
    const t = (i + 1) / (count + 1);
    const along = m.a0 + span * t;

    const px = m.along === "x" ? along : m.fixed;
    const pz = m.along === "x" ? m.fixed : along;

    const group = new THREE.Group();
    group.position.set(px, 1.68, pz);
    group.rotation.y = m.rot;
    parent.add(group);

    const frame = new THREE.Mesh(
      new THREE.BoxGeometry(w + 0.16, h + 0.16, 0.08),
      new THREE.MeshLambertMaterial({ color: 0x11151d })
    );
    frame.position.z = -0.03;
    group.add(frame);

    const canvas = document.createElement("canvas");
    canvas.width = 256; canvas.height = 160;
    const ctx = canvas.getContext("2d");
    const tex = new THREE.CanvasTexture(canvas);
    tex.magFilter = THREE.NearestFilter;
    tex.colorSpace = THREE.SRGBColorSpace;

    const panel = new THREE.Mesh(
      new THREE.PlaneGeometry(w, h),
      new THREE.MeshBasicMaterial({ map: tex })
    );
    panel.position.z = 0.03;
    group.add(panel);

    screens.push({ ctx, tex, style, tint, department: room.department });
  }

}

export function refreshScreens(data) {
  for (const s of screens) {
    drawScreen(s.ctx, s.style, s.tint, {
      ...data,
      crew: (data.crewByDept && data.crewByDept[s.department]) || [],
    });
    s.tex.needsUpdate = true;
  }
}

function neonEdge(parent, room, cx, cz, tint) {
  const mat = new THREE.MeshBasicMaterial({ color: tint });
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
      new THREE.MeshBasicMaterial({ color: tint, transparent: true, opacity: 0.16, depthWrite: false })
    );
    halo.position.set(px, 0.215, pz);
    parent.add(halo);
  }
}

function wallSegment(parent, x, z, w, d, color) {
  const m = new THREE.Mesh(new THREE.BoxGeometry(w, WALL_H, d), new THREE.MeshLambertMaterial({ color }));
  m.position.set(x, WALL_H / 2, z);
  m.castShadow = true;
  m.receiveShadow = true;
  parent.add(m);
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
      new THREE.MeshLambertMaterial({ color: y > 1 ? 0x2b3242 : tint })
    );
    rail.position.set(x, y, z);
    rail.castShadow = y > 1;
    parent.add(rail);
  }
}

function buildWalls(parent, room, cx, cz, doorSide, glassSide, tint, openThrough = []) {
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
    const draw = (px, pz, pw, pd) =>
      s.key === glassSide
        ? glassSegment(parent, px, pz, pw, pd, tint)
        : wallSegment(parent, px, pz, pw, pd, s.shade);

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
  const t = 0.5, y = WALL_H + 0.5;
  const shade = 0x171b24;
  const segs = [
    [0, -h / 2 - t / 2, w + t * 2, t],
    [0, h / 2 + t / 2, w + t * 2, t],
    [-w / 2 - t / 2, 0, t, h + t * 2],
    [w / 2 + t / 2, 0, t, h + t * 2],
  ];
  for (const [px, pz, sw, sd] of segs) {
    const m = new THREE.Mesh(
      new THREE.BoxGeometry(sw, y, sd),
      new THREE.MeshLambertMaterial({ color: shade })
    );
    m.position.set(px, y / 2 - 0.4, pz);
    m.castShadow = true;
    m.receiveShadow = true;
    parent.add(m);

    const strip = new THREE.Mesh(
      new THREE.BoxGeometry(sw + 0.1, 0.16, sd + 0.1),
      new THREE.MeshBasicMaterial({ color: 0x5fd8ff })
    );
    strip.position.set(px, -0.12, pz);
    parent.add(strip);
  }
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

function checkerFloor(parent, room, cx, cz, tint) {
  const mesh = new THREE.InstancedMesh(
    tileGeo,
    new THREE.MeshLambertMaterial({ vertexColors: true }),
    room.w * room.h
  );
  mesh.receiveShadow = true;
  const m = new THREE.Matrix4();
  const c = new THREE.Color();
  let i = 0;
  for (let ix = 0; ix < room.w; ix++)
    for (let iz = 0; iz < room.h; iz++) {
      m.makeTranslation(room.x - cx + ix + 0.5, 0.1, room.y - cz + iz + 0.5);
      mesh.setMatrixAt(i, m);
      mesh.setColorAt(i, c.setHex((ix + iz) % 2
        ? shade(0x39414f, tint, 0.13)
        : shade(0x323947, tint, 0.09)));
      i++;
    }
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

  parent.add(place(buildPlant(), rx + 0.75, 0.2, rz + room.h - 0.8).group);
  parent.add(place(buildPlant(), far, 0.2, rz + room.h - 0.8).group);
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
      parent.add(place(buildBoxes(), rx + 1.2, 0.2, rz + room.h - 1.2).group);
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
      parent.add(place(buildBoxes(), far, 0.2, rz + room.h - 1.2).group);
      break;
    case "infra":
      for (let i = 0; i < 5; i++) {
        parent.add(place(buildServerRack(), rx + 2.2 + i * 0.62, 0.2, back).group);
      }
      pois.push(poi(rx + 2.2 + 1.24, back + 0.6, rx + 2.2 + 1.24, back, "\u{1F5A5}"));
      parent.add(place(buildServerRack(), far, 0.2, rz + room.h - 1.3).group);
      parent.add(place(buildBoxes(), rx + 1.1, 0.2, rz + room.h - 1.2).group);
      break;
  }
  return pois;
}

function buildLobby(parent, lobby, cx, cz) {
  const rx = lobby.x - cx, rz = lobby.y - cz;
  const mid = { x: rx + lobby.w / 2, z: rz + lobby.h / 2 };

  const mesh = new THREE.InstancedMesh(
    tileGeo,
    new THREE.MeshLambertMaterial({ vertexColors: true }),
    lobby.w * lobby.h
  );
  mesh.receiveShadow = true;
  const m = new THREE.Matrix4();
  const c = new THREE.Color();
  let i = 0;
  for (let ix = 0; ix < lobby.w; ix++)
    for (let iz = 0; iz < lobby.h; iz++) {
      m.makeTranslation(rx + ix + 0.5, 0.1, rz + iz + 0.5);
      mesh.setMatrixAt(i, m);
      const ring = Math.min(ix, iz, lobby.w - 1 - ix, lobby.h - 1 - iz);
      mesh.setColorAt(i, c.setHex(ring === 0 ? 0x3f4757 : (ix + iz) % 2 ? 0x465062 : 0x404859));
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
    parent.add(place(buildPlant(), px, 0.2, pz).group);
  }

  parent.add(place(buildWaterCooler(), mid.x + 4.6, 0.2, rz + 1.0).group);
  parent.add(place(buildBoxes(), rx + lobby.w - 1.6, 0.2, rz + lobby.h - 1.9).group);

  const lamp = new THREE.PointLight(0xffe9c8, 46, 24, 1.6);
  lamp.position.set(mid.x, WALL_H + 0.3, mid.z);
  parent.add(lamp);

  const sign = makeLabel("LOBBY", 0xe8eef8, 1.5);
  sign.position.set(mid.x, 0.03, rz + 1.9);
  sign.rotation.x = -Math.PI / 2;
  parent.add(sign);

  return pois;
}

function buildMeetingRoom(parent, room, cx, cz) {
  const tint = 0x6fa8d1;
  const mx = room.x - cx + room.w / 2, mz = room.y - cz + room.h / 2;

  checkerFloor(parent, room, cx, cz, tint);
  buildWalls(parent, room, cx, cz, "-x", "-x", tint);
  neonEdge(parent, room, cx, cz, tint);
  parent.add(place(buildMeetingTable(tint), mx, 0.2, mz).group);

  for (let i = 0; i < 6; i++) {
    const ang = (i / 6) * Math.PI * 2 + Math.PI / 6;
    const px = mx + Math.cos(ang) * 1.6, pz = mz + Math.sin(ang) * 1.25;
    parent.add(place(buildChair(0x3a4152), px, 0.2, pz, Math.atan2(mx - px, mz - pz)).group);
  }
  parent.add(place(buildWhiteboard(tint), mx, 1.05, room.y - cz + 0.62).group);
  parent.add(place(buildPlant(), room.x - cx + 0.8, 0.2, room.y - cz + room.h - 0.8).group);
  parent.add(place(buildPlant(), room.x - cx + room.w - 0.9, 0.2, room.y - cz + 0.9).group);

  const lamp = new THREE.PointLight(0xfff0d8, 34, 18, 1.7);
  lamp.position.set(mx, WALL_H - 0.45, mz);
  parent.add(lamp);

  const sign = makeLabel("WAR ROOM", tint, 1.2);
  sign.position.set(mx, 0.03, room.y - cz + room.h - 1.4);
  sign.rotation.x = -Math.PI / 2;
  parent.add(sign);

  return doorPoint(room, cx, cz, "-x");
}

function buildExtraRoom(parent, room, cx, cz) {
  const rx = room.x - cx, rz = room.y - cz;
  const mx = rx + room.w / 2, mz = rz + room.h / 2;
  const far = rx + room.w - 0.9;
  const back = rz + 0.62;

  switch (room.department) {
    case "break": {
      const tint = 0xe0a55a;
      checkerFloor(parent, room, cx, cz, tint);
      buildWalls(parent, room, cx, cz, doorSideFor(room, cx, cz), null, tint);
      neonEdge(parent, room, cx, cz, tint);
      parent.add(place(buildCoffeeBar(), rx + 2.2, 0.2, back).group);
      parent.add(place(buildSofa(tint), mx - 1.2, 0.2, mz + 0.9, Math.PI).group);
      parent.add(place(buildSofa(0x3ce0c8), mx - 1.2, 0.2, mz - 1.1).group);
      parent.add(place(buildMeetingTable(tint), mx + 2.2, 0.2, mz).group);
      parent.add(place(buildWaterCooler(), far, 0.2, back).group);
      parent.add(place(buildPlant(), rx + 0.75, 0.2, rz + room.h - 0.8).group);
      parent.add(place(buildPlant(), far, 0.2, rz + room.h - 0.8).group);
      const sign = makeLabel("BREAK ROOM", tint, 1.1);
      sign.position.set(mx, 0.03, rz + room.h - 1.3);
      sign.rotation.x = -Math.PI / 2;
      parent.add(sign);
      break;
    }
    case "archive": {
      const tint = 0x8a94a4;
      checkerFloor(parent, room, cx, cz, tint);
      buildWalls(parent, room, cx, cz, doorSideFor(room, cx, cz), null, tint);
      parent.add(place(buildShelf(), rx + 1.4, 0.2, back).group);
      parent.add(place(buildShelf(), rx + 3.4, 0.2, back).group);
      parent.add(place(buildCabinet(tint), far, 0.2, back).group);
      parent.add(place(buildServerRack(), far, 0.2, rz + room.h - 1.3).group);
      parent.add(place(buildBoxes(), rx + 1.2, 0.2, rz + room.h - 1.2).group);
      parent.add(place(buildBoxes(), mx, 0.2, mz).group);
      const sign = makeLabel("ARCHIVE", tint, 1.1);
      sign.position.set(mx, 0.03, rz + room.h - 1.3);
      sign.rotation.x = -Math.PI / 2;
      parent.add(sign);
      break;
    }
    case "landing": {
      const tint = 0x97a2b6;
      checkerFloor(parent, room, cx, cz, tint);
      parent.add(place(buildSofa(0xffc84a), mx + 2.6, 0.2, mz - 0.9, Math.PI).group);
      parent.add(place(buildSofa(0x6fa8d1), mx + 2.6, 0.2, mz + 1.1).group);
      parent.add(place(buildPlant(), rx + 0.9, 0.2, rz + 0.9).group);
      parent.add(place(buildPlant(), rx + room.w - 0.9, 0.2, rz + room.h - 0.9).group);
      parent.add(place(buildWaterCooler(), mx + 5.4, 0.2, rz + 0.9).group);
      const sign = makeLabel("STUDIO LOFT", tint, 1.3);
      sign.position.set(mx, 0.03, rz + 2.1);
      sign.rotation.x = -Math.PI / 2;
      parent.add(sign);
      break;
    }
  }
}

function buildElevatorStop(parent, rect, cx, cz) {
  const mx = rect.x - cx + rect.w / 2, mz = rect.y - cz + rect.h / 2;
  const frame = new THREE.MeshLambertMaterial({ color: 0x1c222d });

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
    new THREE.MeshLambertMaterial({ color: 0x39404f })
  );
  doors.position.set(mx, (WALL_H - 0.6) / 2, mz + rect.h / 2 - 0.03);
  parent.add(doors);

  const lampStrip = new THREE.Mesh(
    new THREE.BoxGeometry(rect.w - 0.4, 0.12, 0.06),
    new THREE.MeshBasicMaterial({ color: 0x4ad991 })
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
  const cx = floor.width / 2, cz = floor.height / 2;
  const avatars = new Map();
  const world = new THREE.Group();
  scene.add(world);

  const corridor = new THREE.Mesh(
    new THREE.BoxGeometry(floor.width + 4, 0.2, floor.height + 4),
    new THREE.MeshLambertMaterial({ color: 0x2a303c })
  );
  corridor.receiveShadow = true;
  world.add(corridor);

  const skirt = new THREE.Mesh(
    new THREE.BoxGeometry(floor.width + 5, 0.55, floor.height + 5),
    new THREE.MeshLambertMaterial({ color: 0x12151c })
  );
  skirt.position.y = -0.28;
  world.add(skirt);

  const levels = [new THREE.Group(), new THREE.Group()];
  levels[1].position.y = LEVEL_H;
  for (const g of levels) world.add(g);

  if ((floor.levels || 1) > 1) {
    const deck = new THREE.Mesh(
      new THREE.BoxGeometry(floor.width + 4, 0.2, floor.height + 4),
      new THREE.MeshLambertMaterial({ color: 0x2a303c })
    );
    deck.receiveShadow = true;
    levels[1].add(deck);
  }

  const lobbyPois = floor.lobby ? buildLobby(levels[0], floor.lobby, cx, cz) : [];
  const meetingDoor = floor.meeting ? buildMeetingRoom(levels[0], floor.meeting, cx, cz) : null;
  for (const extra of floor.extras || []) {
    buildExtraRoom(levels[extra.level || 0], extra, cx, cz);
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
    checkerFloor(rg, room, cx, cz, tint);
    buildWalls(rg, room, cx, cz, door, glass === door ? null : glass, tint, open);
    neonEdge(rg, room, cx, cz, tint);
    wallScreens(rg, room, cx, cz, tint, door, glass, open);
    poisByDept.set(room.department, roomProps(rg, room, cx, cz, tint));

    const rx = room.x - cx, rz = room.y - cz;
    const lamp = new THREE.PointLight(0xfff0d8, 30, 17, 1.7);
    lamp.position.set(rx + room.w / 2, WALL_H - 0.45, rz + room.h / 2);
    rg.add(lamp);

    for (const fx of [0.28, 0.72]) {
      const fix = new THREE.Mesh(
        new THREE.BoxGeometry(1.5, 0.07, 0.28),
        new THREE.MeshBasicMaterial({ color: 0xfff4e0 })
      );
      fix.position.set(rx + room.w * fx, WALL_H - 0.12, rz + room.h / 2);
      rg.add(fix);
    }

    const sign = makeLabel(room.department.toUpperCase(), tint, 1.3);
    if (door === "-z") {
      sign.position.set(rx + room.w * 0.22, WALL_H - 0.42, rz + WALL_T + 0.02);
    } else {
      sign.position.set(rx + room.w / 2, WALL_H - 0.42, rz + WALL_T + 0.02);
    }
    rg.add(sign);
  }

  for (const s of floor.spares || []) {
    const tint = FAMILY_TINT[s.visual_family] || 0x4aa8ff;
    const g = new THREE.Group();
    g.position.set(s.x + s.w / 2 - cx, 0.2, s.y + s.h / 2 - cz);
    levels[s.level || 0].add(g);
    g.add(place(buildDesk(0x2c3240), 0, 0, 0.28, Math.PI).group);
    g.add(place(buildChair(tint), 0, 0, -0.42).group);
  }

  for (const d of floor.desks) {
    const tint = FAMILY_TINT[d.visual_family] || 0x4aa8ff;
    const room = roomsByDept.get(d.department);

    const fixed = new THREE.Group();
    fixed.position.set(d.x + d.w / 2 - cx, 0.2, d.y + d.h / 2 - cz);
    levels[d.level || 0].add(fixed);
    fixed.add(place(buildDesk(tint), 0, 0, 0.28, Math.PI).group);
    fixed.add(place(buildChair(tint), 0, 0, -0.42).group);

    const plate = makeLabel(d.role.replace(/_/g, " "), 0xbcc5d4, 0.6);
    plate.position.set(0, 0.03, 1.15);
    plate.rotation.x = -Math.PI / 2;
    fixed.add(plate);

    const person = new THREE.Group();
    const home = new THREE.Vector3(
      d.x + d.w / 2 - cx,
      (d.level ? LEVEL_H : 0) + 0.22,
      d.y + d.h / 2 - cz - 0.42
    );
    person.position.copy(home);
    world.add(person);

    const ringMat = new THREE.MeshBasicMaterial({
      color: 0x394254, side: THREE.DoubleSide, transparent: true, opacity: 0.4,
    });
    const ring = new THREE.Mesh(new THREE.RingGeometry(0.4, 0.56, 40), ringMat);
    ring.rotation.x = -Math.PI / 2;
    ring.position.y = -0.19;
    person.add(ring);

    const body = place(buildCharacter(d.role), 0, 0, 0);
    person.add(body.group);

    const cb = characterBounds();
    const proxy = new THREE.Mesh(
      new THREE.BoxGeometry(cb.w * VOX, cb.h * VOX, cb.d * VOX),
      PICK_MATERIAL
    );
    proxy.position.y = (cb.h * VOX) / 2;
    proxy.visible = false;
    person.add(proxy);

    const alarm = new THREE.PointLight(0xff3b30, 0, 3.4, 2);
    alarm.position.y = 1.55;
    alarm.visible = false;
    person.add(alarm);

    const lampY = 2.2;
    const lamp = new THREE.Group();
    lamp.visible = false;
    person.add(lamp);

    const shade = new THREE.Mesh(
      new THREE.ConeGeometry(0.2, 0.18, 14, 1, true),
      new THREE.MeshLambertMaterial({ color: 0x161c26, side: THREE.DoubleSide })
    );
    shade.position.y = lampY + 0.1;
    lamp.add(shade);

    const bulb = new THREE.Mesh(
      new THREE.SphereGeometry(0.07, 10, 8),
      new THREE.MeshBasicMaterial({ color: 0x4ad991, transparent: true, opacity: 0.95 })
    );
    bulb.position.y = lampY;
    lamp.add(bulb);

    const cone = new THREE.Mesh(
      new THREE.CylinderGeometry(0.16, 0.75, lampY, 18, 1, true),
      new THREE.MeshBasicMaterial({
        color: 0x4ad991, transparent: true, opacity: 0.16,
        depthWrite: false, side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
      })
    );
    cone.position.y = lampY / 2;
    lamp.add(cone);

    const pool = new THREE.Mesh(
      new THREE.CircleGeometry(0.75, 24),
      new THREE.MeshBasicMaterial({
        color: 0x4ad991, transparent: true, opacity: 0.12,
        depthWrite: false, blending: THREE.AdditiveBlending,
      })
    );
    pool.rotation.x = -Math.PI / 2;
    pool.position.y = 0.015;
    lamp.add(pool);

    const spot = new THREE.SpotLight(0x4ad991, 0, 5, 0.55, 0.7, 1.5);
    spot.position.y = lampY;
    lamp.add(spot);
    lamp.add(spot.target);

    const bubble = chatBubble();
    person.add(bubble.sprite);

    avatars.set(d.role, {
      person, body: body.group, hit: proxy, ringMat, alarm, lamp, bulb, cone, pool, spot,
      tier: d.tier, title: d.title, dept: d.department,
      home,
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
      facing: 0,
      seed: rand() * 10,
      roomPois: poisByDept.get(d.department) || [],
      lobbyPois,
      bubble,
      visit: null,
      chatWith: null,
      chatUntil: 0,
    });
  }

  buildShell(world, floor, cx, cz);
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
  const ambient = buildAmbient(world, floor, cx, cz, 5);
  for (const a of ambient) a.lobbyPois = lobbyPois;
  buildBoard(levels[0], table);

  return {
    world, avatars, ambient, levels, meetingTable: table,
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

  const frame = new THREE.Mesh(
    new THREE.BoxGeometry(2.5, 1.5, 0.1),
    new THREE.MeshLambertMaterial({ color: 0x1a1f29 })
  );
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
      new THREE.MeshLambertMaterial({ color: 0x2b3240 })
    );
    leg.position.set(x, 0.7, 0);
    g.add(leg);
  }

  boardMesh = g;
  g.visible = false;
  return g;
}

export function showBoard(kind, topic, participants, chair) {
  if (!boardMesh || !boardCtx) return;
  const x = boardCtx;
  x.fillStyle = "#e9ecf2";
  x.fillRect(0, 0, 340, 190);
  x.fillStyle = "#c9502e";
  x.fillRect(0, 0, 340, 5);

  x.fillStyle = "#2b3240";
  x.font = "700 17px ui-monospace, monospace";
  x.fillText((kind || "meeting").toUpperCase(), 14, 32);

  x.font = "600 14px ui-monospace, monospace";
  x.fillStyle = "#3d4657";
  const words = String(topic || "").split(/\s+/);
  let line = "", y = 60;
  for (const w of words) {
    if ((line + " " + w).length > 34) { x.fillText(line, 14, y); y += 20; line = w; }
    else line = line ? line + " " + w : w;
    if (y > 132) break;
  }
  if (line && y <= 132) x.fillText(line, 14, y);

  x.fillStyle = "#6a7385";
  x.font = "600 12px ui-monospace, monospace";
  x.fillText((participants || []).join(", ").slice(0, 44), 14, 162);
  if (chair) { x.fillStyle = "#c9502e"; x.fillText("chair: " + chair, 14, 180); }

  boardTex.needsUpdate = true;
  boardMesh.visible = true;
}

export function hideBoard() {
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
    if (a.chatUntil && (now > a.chatUntil || a.meetingSeat || a.mode !== "idle")) {
      const b = a.chatWith;
      stopChat(a, now);
      if (b && b.chatWith === a) stopChat(b, now);
    }
  }
  if (now < nextMingle) return;
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
  a.inLobby = false;
  a.path = [
    ...approach.map((v) => v.clone()).reverse(),
    ...a.route.map((v) => v.clone()).reverse(),
    a.home.clone(),
  ];
  a.target.copy(a.path[0]);
}

export function wanderStep(a, busy, dt, now) {
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
    if (a.mode !== "idle") a.mode = "idle";
    if (arrived) {
      if (a.path.length) {
        a.path.shift();
        if (a.path.length) a.target.copy(a.path[0]);
        else a.target.copy(pointIn(a.inLobby ? a.lobby : a.bounds, a.inLobby ? 0.22 : p.y));
      } else if (now > a.wait) {
        const goLobby = a.lobby && !a.inLobby && rand() < 0.35;
        const comeBack = a.inLobby && rand() < 0.45;

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
  if (dist > 0.06) {
    const speed = a.mode === "returning" ? 5.4 : a.mode === "meeting" ? 2.4 : 0.85;
    const step = Math.min(dist, speed * dt);
    p.x += (dx / dist) * step;
    p.z += (dz / dist) * step;
    a.facing = Math.atan2(dx, dz);
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
  }
  return false;
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

    const body = place(buildCharacter(AMBIENT_PALETTES[i % AMBIENT_PALETTES.length]), 0, 0, 0);
    person.add(body.group);
    const bubble = chatBubble();
    person.add(bubble.sprite);

    out.push({
      person, body: body.group,
      home: start.clone(), bounds: rect, lobby: rect,
      target: start.clone(), route: [], path: [], inLobby: true,
      mode: "idle", meetingSeat: null, meetingFace: null,
      door: start.clone(),
      wait: rand() * 6, facing: 0, seed: rand() * 10,
      roomPois: [], lobbyPois: [], bubble,
      visit: null, chatWith: null, chatUntil: 0,
    });
  }
  return out;
}
