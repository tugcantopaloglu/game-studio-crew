import * as THREE from "/vendor/three.module.js";
import {
  buildCharacter, buildDesk, buildChair, buildPlant, buildCabinet,
  buildWhiteboard, buildServerRack, buildEasel, buildSofa, buildTestBench,
} from "/voxel.js";

export const VOX = 0.085;
export const WALL_H = 2.9;
export const WALL_T = 0.18;
const DOOR_W = 2.4;

export const FAMILY_TINT = {
  leadership: 0xc8a24a, design: 0x8a6fd1, engineering: 0x4a90d9,
  art: 0xd16f9a, audio: 0x4fb3a5, qa: 0xd97a4a,
};

const ROOM_EXTRA = {
  leadership: "sofa",
  production: "cabinet",
  design: "whiteboard",
  engineering: "whiteboard",
  art: "easel",
  audio: "cabinet",
  qa: "testbench",
  infra: "serverrack",
};

const cube = new THREE.BoxGeometry(1, 1, 1);
{
  const n = cube.attributes.position.count;
  cube.setAttribute("color", new THREE.BufferAttribute(new Float32Array(n * 3).fill(1), 3));
}

export function voxelMesh(voxels, opts = {}) {
  const mat = new THREE.MeshLambertMaterial({ vertexColors: true });
  const mesh = new THREE.InstancedMesh(cube, mat, voxels.length);
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

function place(voxels, x, y, z, rotY = 0, opts = {}) {
  const mesh = voxelMesh(voxels, opts);
  const g = new THREE.Group();
  const w = Math.max(...voxels.map((v) => v.x)) + 1;
  const d = Math.max(...voxels.map((v) => v.z)) + 1;
  mesh.scale.setScalar(VOX);
  mesh.position.set((-w * VOX) / 2, 0, (-d * VOX) / 2);
  g.add(mesh);
  g.position.set(x, y, z);
  g.rotation.y = rotY;
  return { group: g, mesh };
}

function wallMat(shade) {
  return new THREE.MeshLambertMaterial({ color: shade });
}

function wallSegment(parent, x, z, w, d, color) {
  const m = new THREE.Mesh(new THREE.BoxGeometry(w, WALL_H, d), wallMat(color));
  m.position.set(x, WALL_H / 2, z);
  m.castShadow = true;
  m.receiveShadow = true;
  parent.add(m);
  return m;
}

function stripe(parent, x, z, w, d, tint) {
  const m = new THREE.Mesh(
    new THREE.BoxGeometry(w, 0.12, d),
    new THREE.MeshBasicMaterial({ color: tint })
  );
  m.position.set(x, 0.34, z);
  parent.add(m);
}

function buildWalls(parent, room, cx, cz, tint, doorSide) {
  const x0 = room.x - cx, z0 = room.y - cz;
  const x1 = x0 + room.w, z1 = z0 + room.h;
  const mx = (x0 + x1) / 2, mz = (z0 + z1) / 2;
  const light = 0x2b3140, dark = 0x232936;

  const sides = [
    { key: "-z", x: mx, z: z0, w: room.w, d: WALL_T, axis: "x" },
    { key: "+z", x: mx, z: z1, w: room.w, d: WALL_T, axis: "x" },
    { key: "-x", x: x0, z: mz, w: WALL_T, d: room.h, axis: "z" },
    { key: "+x", x: x1, z: mz, w: WALL_T, d: room.h, axis: "z" },
  ];

  for (const s of sides) {
    const shade = s.axis === "x" ? light : dark;
    if (s.key !== doorSide) {
      wallSegment(parent, s.x, s.z, s.w, s.d, shade);
      if (s.axis === "x") stripe(parent, s.x, s.z, s.w, WALL_T + 0.02, tint);
      else stripe(parent, s.x, s.z, WALL_T + 0.02, s.d, tint);
      continue;
    }
    const span = s.axis === "x" ? room.w : room.h;
    const side = (span - DOOR_W) / 2;
    if (s.axis === "x") {
      wallSegment(parent, s.x - (DOOR_W / 2 + side / 2), s.z, side, s.d, shade);
      wallSegment(parent, s.x + (DOOR_W / 2 + side / 2), s.z, side, s.d, shade);
      const lintel = new THREE.Mesh(new THREE.BoxGeometry(DOOR_W, 0.5, WALL_T), wallMat(shade));
      lintel.position.set(s.x, WALL_H - 0.25, s.z);
      parent.add(lintel);
    } else {
      wallSegment(parent, s.x, s.z - (DOOR_W / 2 + side / 2), s.w, side, shade);
      wallSegment(parent, s.x, s.z + (DOOR_W / 2 + side / 2), s.w, side, shade);
      const lintel = new THREE.Mesh(new THREE.BoxGeometry(WALL_T, 0.5, DOOR_W), wallMat(shade));
      lintel.position.set(s.x, WALL_H - 0.25, s.z);
      parent.add(lintel);
    }
  }
}

function doorSideFor(room, cx, cz) {
  const dx = room.x + room.w / 2 - cx;
  const dz = room.y + room.h / 2 - cz;
  if (Math.abs(dx) > Math.abs(dz)) return dx > 0 ? "-x" : "+x";
  return dz > 0 ? "-z" : "+z";
}

const tileGeo = new THREE.BoxGeometry(1, 0.2, 1);
{
  const n = tileGeo.attributes.position.count;
  tileGeo.setAttribute("color", new THREE.BufferAttribute(new Float32Array(n * 3).fill(1), 3));
}

function checkerFloor(parent, room, cx, cz) {
  const count = room.w * room.h;
  const mesh = new THREE.InstancedMesh(
    tileGeo,
    new THREE.MeshLambertMaterial({ vertexColors: true }),
    count
  );
  mesh.receiveShadow = true;
  const m = new THREE.Matrix4();
  const c = new THREE.Color();
  let i = 0;
  for (let ix = 0; ix < room.w; ix++)
    for (let iz = 0; iz < room.h; iz++) {
      m.makeTranslation(room.x - cx + ix + 0.5, 0.1, room.y - cz + iz + 0.5);
      mesh.setMatrixAt(i, m);
      mesh.setColorAt(i, c.setHex((ix + iz) % 2 ? 0x2a2f3b : 0x232834));
      i++;
    }
  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  parent.add(mesh);
}

function ceilingFixture(parent, x, z) {
  const fix = new THREE.Mesh(
    new THREE.BoxGeometry(1.8, 0.12, 0.5),
    new THREE.MeshBasicMaterial({ color: 0xe8eef8 })
  );
  fix.position.set(x, WALL_H - 0.18, z);
  parent.add(fix);
}

function roomLight(parent, x, z, tint) {
  const lamp = new THREE.PointLight(0xfff2dd, 26, 16, 1.8);
  lamp.position.set(x, WALL_H - 0.6, z);
  parent.add(lamp);

  const glow = new THREE.PointLight(tint, 5, 9, 2);
  glow.position.set(x, 1.0, z);
  parent.add(glow);
}

export function buildOffice(floor, scene) {
  const cx = floor.width / 2, cz = floor.height / 2;
  const avatars = new Map();
  const world = new THREE.Group();
  scene.add(world);

  const corridor = new THREE.Mesh(
    new THREE.BoxGeometry(floor.width + 4, 0.2, floor.height + 4),
    new THREE.MeshLambertMaterial({ color: 0x1a1e27 })
  );
  corridor.position.set(0, 0, 0);
  corridor.receiveShadow = true;
  world.add(corridor);

  const skirt = new THREE.Mesh(
    new THREE.BoxGeometry(floor.width + 4.6, 0.5, floor.height + 4.6),
    new THREE.MeshLambertMaterial({ color: 0x11141b })
  );
  skirt.position.set(0, -0.25, 0);
  world.add(skirt);

  for (const room of floor.rooms) {
    const tint = FAMILY_TINT[room.visual_family] || 0x4a90d9;
    const rg = new THREE.Group();
    world.add(rg);

    checkerFloor(rg, room, cx, cz);
    buildWalls(rg, room, cx, cz, tint, doorSideFor(room, cx, cz));

    const rx = room.x - cx, rz = room.y - cz;
    ceilingFixture(rg, rx + room.w * 0.3, rz + room.h * 0.5);
    ceilingFixture(rg, rx + room.w * 0.72, rz + room.h * 0.5);
    roomLight(rg, rx + room.w * 0.5, rz + room.h * 0.5, tint);

    const sign = makeLabel(room.department.toUpperCase(), tint, 1.15);
    sign.position.set(rx + room.w / 2, WALL_H - 0.75, rz + 0.14);
    rg.add(sign);

    const plant = place(buildPlant(), rx + room.w - 0.75, 0.2, rz + room.h - 0.75);
    rg.add(plant.group);

    const extra = ROOM_EXTRA[room.department];
    if (extra === "whiteboard") {
      const wb = place(buildWhiteboard(tint), rx + room.w - 1.6, 1.15, rz + 0.35);
      rg.add(wb.group);
    } else if (extra === "cabinet") {
      const cab = place(buildCabinet(tint), rx + room.w - 2.2, 0.2, rz + 0.6);
      rg.add(cab.group);
    } else if (extra === "serverrack") {
      for (let i = 0; i < 3; i++) {
        const r = place(buildServerRack(), rx + room.w - 1.2 - i * 0.75, 0.2, rz + 1.4);
        rg.add(r.group);
      }
    } else if (extra === "easel") {
      const e = place(buildEasel(tint), rx + room.w - 1.4, 0.2, rz + 1.6);
      rg.add(e.group);
    } else if (extra === "sofa") {
      const s = place(buildSofa(tint), rx + room.w - 1.9, 0.2, rz + room.h - 2.1, Math.PI);
      rg.add(s.group);
    } else if (extra === "testbench") {
      const t = place(buildTestBench(tint), rx + room.w - 1.6, 0.2, rz + 1.5);
      rg.add(t.group);
    }
  }

  for (const d of floor.desks) {
    const tint = FAMILY_TINT[d.visual_family] || 0x4a90d9;
    const g = new THREE.Group();
    g.position.set(d.x + d.w / 2 - cx, 0.2, d.y + d.h / 2 - cz);
    world.add(g);

    const desk = place(buildDesk(tint), 0, 0, 0.28);
    g.add(desk.group);

    const chair = place(buildChair(tint), 0, 0, -0.42);
    g.add(chair.group);

    const ringMat = new THREE.MeshBasicMaterial({
      color: 0x2f3644, side: THREE.DoubleSide, transparent: true, opacity: 0.4,
    });
    const ring = new THREE.Mesh(new THREE.RingGeometry(0.44, 0.6, 40), ringMat);
    ring.rotation.x = -Math.PI / 2;
    ring.position.set(0, 0.015, -0.42);
    g.add(ring);

    const body = place(buildCharacter(d.role), 0, 0.02, -0.42);
    g.add(body.group);

    const plate = makeLabel(d.role.replace(/_/g, " "), 0xaab3c2, 0.62);
    plate.position.set(0, 0.025, 1.05);
    plate.rotation.x = -Math.PI / 2;
    g.add(plate);

    avatars.set(d.role, {
      group: g, body: body.group, hit: body.mesh, ring, ringMat,
      tier: d.tier, title: d.title, dept: d.department,
      seed: (d.role.charCodeAt(0) * 7 + d.role.length * 13) % 100 / 10,
    });
  }

  return { world, avatars };
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
