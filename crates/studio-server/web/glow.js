import * as THREE from "/vendor/three.module.js";

export const GLOW_LAYER = 1;

const SCREEN_VERT = `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = vec4(position.xy, 0.0, 1.0);
}`;

const BLUR_FRAG = `
uniform sampler2D tSource;
uniform vec2 uStep;
varying vec2 vUv;
void main() {
  vec4 sum = texture2D(tSource, vUv) * 0.1633;
  sum += texture2D(tSource, vUv - uStep * 4.0) * 0.0510;
  sum += texture2D(tSource, vUv - uStep * 3.0) * 0.0918;
  sum += texture2D(tSource, vUv - uStep * 2.0) * 0.1224;
  sum += texture2D(tSource, vUv - uStep) * 0.1531;
  sum += texture2D(tSource, vUv + uStep) * 0.1531;
  sum += texture2D(tSource, vUv + uStep * 2.0) * 0.1224;
  sum += texture2D(tSource, vUv + uStep * 3.0) * 0.0918;
  sum += texture2D(tSource, vUv + uStep * 4.0) * 0.0510;
  gl_FragColor = sum;
}`;

const ADD_FRAG = `
uniform sampler2D tSource;
uniform float uStrength;
varying vec2 vUv;
void main() {
  vec3 lit = max(texture2D(tSource, vUv).rgb, 0.0);
  gl_FragColor = vec4(pow(lit, vec3(0.4545)) * uStrength, 1.0);
}`;

const QUAD = new THREE.PlaneGeometry(2, 2);
const BLACK = new THREE.Color(0x000000);

export function createGlow(renderer, options = {}) {
  const scale = options.scale || 0.5;
  const strength = options.strength === undefined ? 0.46 : options.strength;
  const passes = options.passes || 2;

  const make = () =>
    new THREE.WebGLRenderTarget(2, 2, {
      type: THREE.HalfFloatType,
      depthBuffer: false,
      stencilBuffer: false,
    });
  const front = make();
  const back = make();

  const blur = new THREE.ShaderMaterial({
    uniforms: { tSource: { value: null }, uStep: { value: new THREE.Vector2() } },
    vertexShader: SCREEN_VERT,
    fragmentShader: BLUR_FRAG,
    depthTest: false,
    depthWrite: false,
  });
  const add = new THREE.ShaderMaterial({
    uniforms: { tSource: { value: null }, uStrength: { value: strength } },
    vertexShader: SCREEN_VERT,
    fragmentShader: ADD_FRAG,
    blending: THREE.AdditiveBlending,
    transparent: true,
    depthTest: false,
    depthWrite: false,
  });

  const flat = new THREE.Camera();
  const stage = new THREE.Scene();
  const quad = new THREE.Mesh(QUAD, blur);
  quad.frustumCulled = false;
  stage.add(quad);

  let wide = 2;
  let high = 2;
  const clear = new THREE.Color();

  function resize(w, h) {
    wide = Math.max(2, Math.round(w * scale));
    high = Math.max(2, Math.round(h * scale));
    front.setSize(wide, high);
    back.setSize(wide, high);
  }

  function sweep(source, into, x, y) {
    blur.uniforms.tSource.value = source.texture;
    blur.uniforms.uStep.value.set(x / wide, y / high);
    quad.material = blur;
    renderer.setRenderTarget(into);
    renderer.render(stage, flat);
  }

  function render(scene, camera) {
    renderer.render(scene, camera);

    const sky = scene.background;
    const haze = scene.fog;
    const wasAlpha = renderer.getClearAlpha();
    renderer.getClearColor(clear);

    scene.background = null;
    scene.fog = null;
    camera.layers.set(GLOW_LAYER);
    renderer.setClearColor(BLACK, 0);
    renderer.setRenderTarget(front);
    renderer.clear();
    renderer.render(scene, camera);
    camera.layers.set(0);
    scene.background = sky;
    scene.fog = haze;

    for (let i = 0; i < passes; i++) {
      sweep(front, back, 1, 0);
      sweep(back, front, 0, 1);
    }

    renderer.setRenderTarget(null);
    renderer.setClearColor(clear, wasAlpha);
    add.uniforms.tSource.value = front.texture;
    quad.material = add;
    renderer.autoClear = false;
    renderer.render(stage, flat);
    renderer.autoClear = true;
  }

  function dispose() {
    front.dispose();
    back.dispose();
    blur.dispose();
    add.dispose();
  }

  return { resize, render, dispose, material: add };
}
