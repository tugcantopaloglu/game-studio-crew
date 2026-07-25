import * as THREE from "/vendor/three.module.js";
import { buildCharacterParts, characterBounds, HIP_Y, KNEE_Y, NECK_Y, SHOULDER_Y } from "/voxel.js";

export const VOX = 0.085;

const CYCLE = 1.06;
const STRIDE = CYCLE / 2;
const LEG_LEN = HIP_Y * VOX;
const FOOT_LIFT = 0.075;
const KNEE_BEND = 0.95;
const THIGH_LEAN = 0.4;
const ARM_SWING = 0.6;
const BOB = 0.026;
const ROLL = 0.05;
const SEAT_DROP = 0.29;
const TURN_RATE = 7.5;
const RUN_SPEED = 1.5;

const cube = new THREE.BoxGeometry(1, 1, 1);
cube.setAttribute(
  "color",
  new THREE.BufferAttribute(new Float32Array(cube.attributes.position.count * 3).fill(1), 3)
);

const voxelMaterial = new THREE.MeshLambertMaterial({ vertexColors: true });
const matrix = new THREE.Matrix4();
const tint = new THREE.Color();

export function voxelMesh(voxels, opts = {}) {
  const mesh = new THREE.InstancedMesh(cube, voxelMaterial, voxels.length);
  mesh.castShadow = opts.castShadow !== false;
  mesh.receiveShadow = true;
  voxels.forEach((v, i) => {
    matrix.makeTranslation(v.x + 0.5, v.y + 0.5, v.z + 0.5);
    mesh.setMatrixAt(i, matrix);
    mesh.setColorAt(i, tint.setHex(v.c));
  });
  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  return mesh;
}

function joint(part) {
  const g = new THREE.Group();
  if (!part.voxels.length) return g;
  const mesh = voxelMesh(part.voxels);
  mesh.scale.setScalar(VOX);
  mesh.position.set(-part.pivot[0] * VOX, -part.pivot[1] * VOX, -part.pivot[2] * VOX);
  g.add(mesh);
  return g;
}

function wrapAngle(a) {
  while (a > Math.PI) a -= Math.PI * 2;
  while (a < -Math.PI) a += Math.PI * 2;
  return a;
}

function clamp(v, lo, hi) {
  return v < lo ? lo : v > hi ? hi : v;
}

function approach(current, target, rate, dt) {
  return current + (target - current) * Math.min(1, rate * dt);
}

function smooth(w) {
  return (1 - Math.cos(Math.PI * w)) / 2;
}

class Rig {
  constructor(roleId, seed) {
    const parts = buildCharacterParts(roleId);
    this.group = new THREE.Group();
    this.seed = seed;

    this.hips = new THREE.Group();
    this.hips.position.y = HIP_Y * VOX;
    this.group.add(this.hips);

    this.torso = joint(parts.torso);
    this.hips.add(this.torso);

    this.head = joint(parts.head);
    this.head.position.y = (NECK_Y - HIP_Y) * VOX;
    this.torso.add(this.head);

    this.armL = joint(parts.armL);
    this.armL.position.set(-3 * VOX, (SHOULDER_Y - HIP_Y) * VOX, 0);
    this.torso.add(this.armL);

    this.armR = joint(parts.armR);
    this.armR.position.set(3 * VOX, (SHOULDER_Y - HIP_Y) * VOX, 0);
    this.torso.add(this.armR);

    this.prop = joint(parts.prop);
    this.armR.add(this.prop);
    this.carrying = parts.prop.voxels.length > 0;

    this.thighL = joint(parts.thighL);
    this.thighL.position.x = -1.5 * VOX;
    this.hips.add(this.thighL);
    this.shinL = joint(parts.shinL);
    this.shinL.position.y = (KNEE_Y - HIP_Y) * VOX;
    this.thighL.add(this.shinL);

    this.thighR = joint(parts.thighR);
    this.thighR.position.x = 1.5 * VOX;
    this.hips.add(this.thighR);
    this.shinR = joint(parts.shinR);
    this.shinR.position.y = (KNEE_Y - HIP_Y) * VOX;
    this.thighR.add(this.shinR);

    this.phase = seed % 1;
    this.tilt = [0, 0];
    this.knee = [0, 0];
    this.yaw = 0;
    this.sit = 0;
    this.headYaw = 0;
    this.headPitch = 0;
    this.glance = 0;
    this.glanceAt = 0;
    this.lean = 0;
    this.owed = 0;
  }

  step(i, thigh, u, blend, dt) {
    if (blend < 0.002) {
      this.tilt[i] = approach(this.tilt[i], 0, 9, dt);
      this.knee[i] = approach(this.knee[i], 0, 9, dt);
      thigh.position.y = approach(thigh.position.y, 0, 9, dt);
      thigh.position.z = approach(thigh.position.z, 0, 9, dt);
      return;
    }
    const stance = u < 0.5;
    const w = (u - 0.5) * 2;
    const z = stance ? STRIDE * (0.5 - 2 * u) : STRIDE * (-0.5 + smooth(w));
    const lift = stance ? 0 : Math.sin(Math.PI * w) * FOOT_LIFT;
    const tilt = -THIGH_LEAN * (z / (STRIDE * 0.5));

    this.tilt[i] = tilt * blend;
    this.knee[i] = stance ? 0 : Math.sin(Math.PI * w) * KNEE_BEND * blend;
    thigh.position.y = lift * blend;
    thigh.position.z = (z + LEG_LEN * Math.sin(tilt)) * blend;
  }

  update(m, dt, t) {
    this.sit = approach(this.sit, m.sitting ? 1 : 0, 5, dt);
    const sit = this.sit;
    const speed = m.speed || 0;
    const walk = clamp(speed / 0.7, 0, 1) * (1 - sit);
    const stuck = m.stuck ? 1 : 0;

    this.phase = (this.phase + (speed * dt) / CYCLE) % 1;
    this.yaw = wrapAngle(
      this.yaw + wrapAngle((m.facing || 0) - this.yaw) * Math.min(1, TURN_RATE * dt)
    );
    this.group.rotation.y = this.yaw;

    this.step(0, this.thighL, this.phase, walk, dt);
    this.step(1, this.thighR, (this.phase + 0.5) % 1, walk, dt);

    const seat = sit * 1.42;
    this.thighL.rotation.x = this.tilt[0] - seat;
    this.thighR.rotation.x = this.tilt[1] - seat;
    this.shinL.rotation.x = this.knee[0] + seat;
    this.shinR.rotation.x = this.knee[1] + seat;
    this.hips.position.y = HIP_Y * VOX - sit * SEAT_DROP;

    this.lean = approach(this.lean, m.recline || 0, 2.2, dt);
    const sway = Math.sin(t * 0.45 + this.seed) * 0.055;
    const breath = Math.sin(t * 1.5 + this.seed) * 0.008;

    this.torso.position.y = BOB * Math.cos(4 * Math.PI * this.phase) * walk + breath * (1 - walk);
    this.torso.rotation.z =
      ROLL * Math.sin(2 * Math.PI * this.phase) * walk + sway * (1 - walk) * (1 - sit);
    this.torso.rotation.x =
      clamp(speed / RUN_SPEED, 0, 1) * 0.16 + stuck * 0.2 + sit * 0.12 - this.lean * 0.36;
    this.torso.scale.y = 1 + breath * 0.7 * (1 - walk);

    const swing = ARM_SWING * Math.sin(2 * Math.PI * this.phase) * walk;
    const desk = m.working && sit > 0.5 ? 1 - this.lean : 0;
    const type = desk ? Math.sin(t * 11 + this.seed) * 0.06 : 0;

    this.armL.rotation.x = swing - desk * 1.05 + type - stuck * 0.25;
    this.armR.rotation.x =
      -swing * (this.carrying ? 0.35 : 1) - desk * 1.05 - type - stuck * 0.25;
    this.armL.rotation.z = 0.06 + sway * 0.4;
    this.armR.rotation.z = -0.06 - sway * 0.4;
    this.prop.rotation.x = -this.armR.rotation.x * 0.75;

    if (t > this.glanceAt) {
      this.glance = ((Math.sin((t + this.seed) * 91.7) * 43758.5) % 1) * 0.8;
      this.glanceAt = t + 2.5 + Math.abs(Math.sin(t * 0.7 + this.seed)) * 5;
    }

    let wantYaw = this.glance * (1 - walk);
    let wantPitch = -stuck * 0.32 + this.lean * 0.3;
    if (m.lookAt && m.at) {
      const dx = m.lookAt.x - m.at.x;
      const dz = m.lookAt.z - m.at.z;
      const flat = Math.hypot(dx, dz);
      if (flat > 0.05) {
        wantYaw = clamp(wrapAngle(Math.atan2(dx, dz) - this.yaw), -1.0, 1.0);
        wantPitch += clamp(Math.atan2(m.lookAt.y - m.at.y - NECK_Y * VOX, flat), -0.4, 0.4);
      }
    } else if (walk > 0.2) {
      wantYaw = 0;
    }

    this.headYaw = approach(this.headYaw, wantYaw, 4, dt);
    this.headPitch = approach(this.headPitch, wantPitch, 4, dt);
    const talk = m.talking ? 1 : 0;
    this.head.rotation.y = this.headYaw + talk * Math.sin(t * 6.3 + this.seed) * 0.07;
    this.head.rotation.x = this.headPitch + talk * Math.sin(t * 7.9 + this.seed) * 0.05;
    this.head.rotation.z = -this.torso.rotation.z * 0.45;
  }
}

export function buildAvatar(roleId, seed = 0) {
  return new Rig(roleId, seed);
}

export { characterBounds };
