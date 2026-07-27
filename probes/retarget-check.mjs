import { join } from "node:path";
import { pathToFileURL } from "node:url";

const helpers = join(process.cwd(), "crates", "studio-engine", "helpers");
const THREE = await import(pathToFileURL(join(helpers, "three.module.js")).href);
const { retargetClip, mapping, MIXAMO_MAP, strip } = await import(
  pathToFileURL(join(helpers, "retarget.mjs")).href
);

const failures = [];

function check(what, passed, detail) {
  if (passed) {
    console.log(`  ok   ${what}`);
    return;
  }
  failures.push(what);
  console.log(`  FAIL ${what}${detail ? ` (${detail})` : ""}`);
}

function joint(parent, name, x, y, z) {
  const node = new THREE.Object3D();
  node.name = name;
  node.position.set(x, y, z);
  parent.add(node);
  return node;
}

function mixamoLike() {
  const root = new THREE.Object3D();
  root.name = "Armature";
  const hips = joint(root, "mixamorigHips", 0, 100, 0);
  const spine = joint(hips, "mixamorigSpine", 0, 10, 0);
  const chest = joint(spine, "mixamorigSpine1", 0, 12, 0);
  joint(chest, "mixamorigHead", 0, 25, 0);

  const shoulder = joint(chest, "mixamorigLeftShoulder", 5, 15, 0);
  const arm = joint(shoulder, "mixamorigLeftArm", 10, 0, 0);
  const fore = joint(arm, "mixamorigLeftForeArm", 25, 0, 0);
  joint(fore, "mixamorigLeftHand", 25, 0, 0);
  arm.updateMatrixWorld(true);

  const thigh = joint(hips, "mixamorigLeftUpLeg", 8, 0, 0);
  const shin = joint(thigh, "mixamorigLeftLeg", 0, -45, 0);
  joint(shin, "mixamorigLeftFoot", 0, -45, 0);
  root.updateMatrixWorld(true);
  return root;
}

function studioLike() {
  const root = new THREE.Group();
  root.name = "walker";
  const hips = joint(root, "hips", 0, 0.95, 0);
  const spine = joint(hips, "spine", 0, 0.1, 0);
  const chest = joint(spine, "chest", 0, 0.12, 0);
  joint(chest, "head", 0, 0.25, 0);

  const shoulder = joint(chest, "shoulder_l", 0.05, 0.15, 0);
  const arm = joint(shoulder, "upper_arm_l", 0.1, 0, 0);
  const fore = joint(arm, "forearm_l", 0, -0.25, 0);
  joint(fore, "hand_l", 0, -0.25, 0);

  const thigh = joint(hips, "thigh_l", 0.08, 0, 0);
  const shin = joint(thigh, "shin_l", 0, -0.45, 0);
  joint(shin, "foot_l", 0, -0.45, 0);

  const body = new THREE.Mesh(
    new THREE.BoxGeometry(0.4, 0.5, 0.2),
    new THREE.MeshStandardMaterial()
  );
  body.name = "torso_mesh";
  chest.add(body);
  root.updateMatrixWorld(true);
  return root;
}

function swing(name, axis, degrees, duration) {
  const at = (deg) => {
    const euler = new THREE.Euler(
      axis === "x" ? (deg * Math.PI) / 180 : 0,
      axis === "y" ? (deg * Math.PI) / 180 : 0,
      axis === "z" ? (deg * Math.PI) / 180 : 0
    );
    return new THREE.Quaternion().setFromEuler(euler);
  };
  const values = [];
  for (const q of [at(0), at(degrees), at(0)]) values.push(q.x, q.y, q.z, q.w);
  return new THREE.QuaternionKeyframeTrack(
    `${name}.quaternion`,
    [0, duration / 2, duration],
    values
  );
}

function angleBetween(root, a, b, c) {
  const first = root.getObjectByName(a).getWorldPosition(new THREE.Vector3());
  const middle = root.getObjectByName(b).getWorldPosition(new THREE.Vector3());
  const last = root.getObjectByName(c).getWorldPosition(new THREE.Vector3());
  const one = first.sub(middle).normalize();
  const two = last.sub(middle).normalize();
  return (Math.acos(Math.min(1, Math.max(-1, one.dot(two)))) * 180) / Math.PI;
}

function poseAt(root, clip, at) {
  const mixer = new THREE.AnimationMixer(root);
  mixer.clipAction(clip).play();
  mixer.setTime(at);
  root.updateMatrixWorld(true);
  return mixer;
}

console.log("mixamo retargeting");

const source = mixamoLike();
const target = studioLike();
const pairs = mapping(source, target);

check(
  "every mixamo bone with a studio joint is paired",
  pairs.length === 11,
  `${pairs.length} pairs: ${pairs.map((p) => p.name).join(",")}`
);
check(
  "the map covers the bones mixamo actually emits",
  Object.keys(MIXAMO_MAP).length >= 20 && MIXAMO_MAP.LeftUpLeg === "thigh_l"
);
check(
  "the prefix comes off however the loader spelled it",
  strip("mixamorig:LeftUpLeg") === "LeftUpLeg" &&
    strip("mixamorigLeftUpLeg") === "LeftUpLeg" &&
    strip("mixamorig5:LeftUpLeg") === "LeftUpLeg" &&
    strip("Armature") === "Armature",
  [strip("mixamorig:LeftUpLeg"), strip("mixamorigLeftUpLeg"), strip("mixamorig5:LeftUpLeg")].join("/")
);

function boneDirection(root, from, to) {
  const a = root.getObjectByName(from).getWorldPosition(new THREE.Vector3());
  const b = root.getObjectByName(to).getWorldPosition(new THREE.Vector3());
  return b.sub(a).normalize();
}

const mixamoArm = boneDirection(source, "mixamorigLeftArm", "mixamorigLeftForeArm");
const studioArm = boneDirection(target, "upper_arm_l", "forearm_l");
const apart = (Math.acos(Math.min(1, Math.max(-1, mixamoArm.dot(studioArm)))) * 180) / Math.PI;
check(
  "the two rigs really do rest in different poses",
  apart > 45,
  `mixamo arm points ${mixamoArm.toArray().map((n) => n.toFixed(2))}, studio ${studioArm
    .toArray()
    .map((n) => n.toFixed(2))}, ${apart.toFixed(1)}deg apart`
);

const restElbow = angleBetween(source, "mixamorigLeftArm", "mixamorigLeftForeArm", "mixamorigLeftHand");
const restStudioElbow = angleBetween(target, "upper_arm_l", "forearm_l", "hand_l");

const bend = new THREE.AnimationClip("bend", 1, [
  swing("mixamorigLeftForeArm", "y", -80, 1),
  swing("mixamorigLeftUpLeg", "x", 40, 1),
]);

const landed = retargetClip(THREE, {
  sourceRoot: source,
  targetRoot: target,
  clip: bend,
  pairs,
  name: "bend",
});

check("the retargeted clip keeps its name and length", landed.name === "bend" && landed.duration === 1);
check(
  "and only carries tracks a glb can hold",
  landed.tracks.every((t) => t.name.endsWith(".quaternion") || t.name.endsWith(".position")),
  landed.tracks.map((t) => t.name).join(" ")
);

poseAt(source, bend, 0.5);
poseAt(target, landed, 0.5);

const sourceElbow = angleBetween(source, "mixamorigLeftArm", "mixamorigLeftForeArm", "mixamorigLeftHand");
const targetElbow = angleBetween(target, "upper_arm_l", "forearm_l", "hand_l");
check(
  "the elbow actually bent on the mixamo rig",
  Math.abs(sourceElbow - restElbow) > 30,
  `${restElbow.toFixed(1)}deg at rest, ${sourceElbow.toFixed(1)}deg mid-clip`
);
check(
  "the elbow bends to the same angle it does on the mixamo rig",
  Math.abs(sourceElbow - targetElbow) < 2,
  `mixamo ${sourceElbow.toFixed(1)}deg vs studio ${targetElbow.toFixed(1)}deg`
);

const sourceKnee = angleBetween(source, "mixamorigHips", "mixamorigLeftUpLeg", "mixamorigLeftLeg");
const targetKnee = angleBetween(target, "hips", "thigh_l", "shin_l");
check(
  "and the hip swings to the same angle",
  Math.abs(sourceKnee - targetKnee) < 2,
  `mixamo ${sourceKnee.toFixed(1)}deg vs studio ${targetKnee.toFixed(1)}deg`
);

poseAt(source, bend, 0);
poseAt(target, landed, 0);
const restAgain = angleBetween(target, "upper_arm_l", "forearm_l", "hand_l");
check(
  "at rest the studio rig is back in its own pose, not the mixamo one",
  Math.abs(restAgain - restStudioElbow) < 2,
  `${restAgain.toFixed(1)}deg, rest is ${restStudioElbow.toFixed(1)}deg`
);

const noPairs = () => retargetClip(THREE, { sourceRoot: source, targetRoot: target, clip: bend, pairs: [] });
let refused = "";
try {
  noPairs();
} catch (err) {
  refused = err.message;
}
check("a rig with nothing in common is refused rather than silently empty", refused.includes("no joint"), refused);

console.log(failures.length ? `\n${failures.length} failed` : "\nall retarget checks passed");
process.exit(failures.length ? 1 : 0);
