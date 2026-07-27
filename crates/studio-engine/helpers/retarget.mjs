export const MIXAMO_PREFIX = "mixamorig";

export const MIXAMO_MAP = {
  Hips: "hips",
  Spine: "spine",
  Spine1: "chest",
  Spine2: "chest",
  Neck: "neck",
  Head: "head",
  LeftShoulder: "shoulder_l",
  LeftArm: "upper_arm_l",
  LeftForeArm: "forearm_l",
  LeftHand: "hand_l",
  RightShoulder: "shoulder_r",
  RightArm: "upper_arm_r",
  RightForeArm: "forearm_r",
  RightHand: "hand_r",
  LeftUpLeg: "thigh_l",
  LeftLeg: "shin_l",
  LeftFoot: "foot_l",
  LeftToeBase: "toe_l",
  RightUpLeg: "thigh_r",
  RightLeg: "shin_r",
  RightFoot: "foot_r",
  RightToeBase: "toe_r",
};

const RIG_PREFIX = /^mixamorig\d*:?/i;

export function strip(name) {
  const said = String(name);
  const at = said.indexOf(":");
  const bare = at < 0 ? said : said.slice(at + 1);
  return RIG_PREFIX.test(said) ? said.replace(RIG_PREFIX, "") : bare;
}

export function mapping(sourceRoot, targetRoot, extra = {}) {
  const table = { ...MIXAMO_MAP, ...extra };
  const pairs = [];
  const taken = new Set();
  sourceRoot.traverse((node) => {
    if (!node.name) return;
    const bare = strip(node.name);
    const want = table[bare] || table[node.name];
    if (!want || taken.has(want)) return;
    const target = targetRoot.getObjectByName(want);
    if (!target) return;
    taken.add(want);
    pairs.push({ source: node, target, name: want });
  });
  return pairs;
}

function worldQuaternion(THREE, node, out) {
  node.getWorldQuaternion(out);
  return out;
}

export function restOf(THREE, root, pairs, which) {
  root.updateMatrixWorld(true);
  const rest = new Map();
  for (const pair of pairs) {
    const node = pair[which];
    rest.set(pair.name, {
      world: worldQuaternion(THREE, node, new THREE.Quaternion()),
      local: node.quaternion.clone(),
      position: node.position.clone(),
      worldPosition: node.getWorldPosition(new THREE.Vector3()),
    });
  }
  return rest;
}

export function boneAim(THREE, pairs, which) {
  const held = new Map();
  for (const pair of pairs) held.set(pair[which], pair);
  const aim = new Map();
  for (const pair of pairs) {
    const node = pair[which];
    let child = null;
    for (const kid of node.children) {
      if (held.has(kid)) {
        child = kid;
        break;
      }
    }
    if (!child) {
      for (const other of pairs) {
        if (other === pair) continue;
        let up = other[which].parent;
        while (up && up !== node) up = up.parent;
        if (up === node) {
          child = other[which];
          break;
        }
      }
    }
    if (!child) continue;
    const from = node.getWorldPosition(new THREE.Vector3());
    const to = child.getWorldPosition(new THREE.Vector3());
    const along = to.sub(from);
    if (along.lengthSq() < 1e-12) continue;
    aim.set(pair.name, along.normalize());
  }
  return aim;
}

export function alignments(THREE, pairs, sourceAim, targetAim) {
  const align = new Map();
  for (const pair of pairs) {
    const mine = targetAim.get(pair.name);
    const theirs = sourceAim.get(pair.name);
    if (!mine || !theirs) continue;
    align.set(pair.name, new THREE.Quaternion().setFromUnitVectors(mine, theirs));
  }
  for (const pair of pairs) {
    if (align.has(pair.name)) continue;
    let up = pair.target.parent;
    while (up && !align.has(up.name)) up = up.parent;
    align.set(
      pair.name,
      up && align.has(up.name) ? align.get(up.name).clone() : new THREE.Quaternion()
    );
  }
  return align;
}

export function heightOf(THREE, root) {
  root.updateMatrixWorld(true);
  return new THREE.Box3().setFromObject(root).getSize(new THREE.Vector3()).y;
}

export function retargetClip(THREE, options) {
  const { sourceRoot, targetRoot, clip, pairs, fps = 30, name } = options;
  if (!pairs.length) {
    throw new Error("no joint in the animation matches a joint in this model");
  }

  const sourceRest = restOf(THREE, sourceRoot, pairs, "source");
  const targetRest = restOf(THREE, targetRoot, pairs, "target");
  const align = alignments(
    THREE,
    pairs,
    boneAim(THREE, pairs, "source"),
    boneAim(THREE, pairs, "target")
  );

  const sourceHeight = heightOf(THREE, sourceRoot) || 1;
  const targetHeight = heightOf(THREE, targetRoot) || 1;
  const scale = targetHeight / sourceHeight;

  const steps = Math.max(2, Math.round(clip.duration * fps) + 1);
  const times = [];
  const rotations = new Map();
  const roots = [];
  for (const pair of pairs) rotations.set(pair.name, []);

  const mixer = new THREE.AnimationMixer(sourceRoot);
  const action = mixer.clipAction(clip);
  action.play();

  const carry = new THREE.Quaternion();
  const worldNow = new THREE.Quaternion();
  const parentWorld = new THREE.Quaternion();
  const wanted = new Map();
  const hips = pairs.find((pair) => pair.name === "hips");

  for (let step = 0; step < steps; step++) {
    const at = (clip.duration * step) / (steps - 1);
    times.push(at);
    mixer.setTime(at);
    sourceRoot.updateMatrixWorld(true);

    wanted.clear();
    for (const pair of pairs) {
      const rest = sourceRest.get(pair.name);
      const mine = targetRest.get(pair.name);
      const onto = align.get(pair.name) || carry.identity();
      worldQuaternion(THREE, pair.source, worldNow);
      carry.copy(rest.world).invert().premultiply(worldNow);
      const swung = onto.clone().invert().multiply(carry).multiply(onto);
      wanted.set(pair.name, swung.multiply(mine.world));
    }

    for (const pair of pairs) {
      const world = wanted.get(pair.name);
      let parent = pair.target.parent;
      while (parent && !wanted.has(parent.name)) parent = parent.parent;
      if (parent && wanted.has(parent.name)) {
        parentWorld.copy(wanted.get(parent.name));
      } else {
        pair.target.parent
          ? worldQuaternion(THREE, pair.target.parent, parentWorld)
          : parentWorld.identity();
      }
      const local = parentWorld.clone().invert().multiply(world);
      rotations.get(pair.name).push(local.x, local.y, local.z, local.w);
    }

    if (hips) {
      const rest = sourceRest.get("hips");
      const mine = targetRest.get("hips");
      const drift = hips.source
        .getWorldPosition(new THREE.Vector3())
        .sub(rest.worldPosition);
      roots.push(
        mine.position.x + drift.x * scale,
        mine.position.y + drift.y * scale,
        mine.position.z + drift.z * scale
      );
    }
  }

  const tracks = [];
  for (const pair of pairs) {
    tracks.push(
      new THREE.QuaternionKeyframeTrack(`${pair.name}.quaternion`, times, rotations.get(pair.name))
    );
  }
  if (hips && roots.length === times.length * 3) {
    tracks.push(new THREE.VectorKeyframeTrack("hips.position", times, roots));
  }

  return new THREE.AnimationClip(name || clip.name || "clip", clip.duration, tracks);
}
