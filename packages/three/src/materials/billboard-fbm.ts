// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";

/**
 * Reference billboard material for Galeon particle effects.
 *
 * Pairs a camera-facing quad (vertex shader) with a 2D-FBM density
 * fragment shader. Uniforms cover color, size, lifetime fade, noise scale,
 * and an optional soft-edge depth fade against a scene depth texture.
 *
 * This is meant as a starting point for downstream effect work — copy it,
 * adapt the FBM weights, swap the noise function for one that better fits
 * your art direction. The point is to ship a *generic* reference, not a
 * domain-specific shader.
 *
 * # Geometry assumption
 *
 * The mesh that wears this material should use a unit `THREE.PlaneGeometry(1, 1)`
 * (positions in `[-0.5, 0.5]^2`). The vertex shader treats `position.xy` as
 * the quad-local offset and expects `uv` in `[0, 1]`.
 *
 * # Blending
 *
 * The fragment shader writes premultiplied alpha (`rgb * a, a`) and the
 * material configures custom blending (`OneFactor` × `OneMinusSrcAlphaFactor`)
 * so it composites correctly regardless of `WebGLRenderer`'s
 * `premultipliedAlpha` flag.
 *
 * # Soft-particle depth fade
 *
 * Set `softEdgeDistance > 0` and supply `depthTexture` + `cameraNear` /
 * `cameraFar` to fade the particle alpha as it approaches scene geometry.
 * Avoids the hard "card cutting through wall" artifact common to billboards.
 */
export interface BillboardFbmMaterialOptions {
  /**
   * Base color (linear RGB). Default: warm off-white `[0.95, 0.92, 0.85]`.
   */
  color?: THREE.ColorRepresentation;
  /**
   * Particle world-space size (scales the quad). Default: `1.0`.
   */
  size?: number;
  /**
   * Lifetime progress in `[0, 1]`. `0` is just-spawned (full alpha); `1`
   * is dying (zero alpha). Default: `0.0`.
   */
  lifetimeProgress?: number;
  /**
   * UV multiplier for the FBM noise field. Higher = finer detail.
   * Default: `2.0`.
   */
  noiseScale?: number;
  /**
   * Number of FBM octaves. Compiled into the shader via `defines` — changing
   * this rebuilds the program. Default: `4`. Range: `[1, 8]`.
   */
  noiseOctaves?: number;
  /**
   * Density falloff from the quad edge. `0` = uniform fill, `1` = sharp
   * radial cutoff. Default: `0.6`.
   */
  densityFalloff?: number;
  /**
   * Soft-edge fade distance in world units against `depthTexture`. `0`
   * disables soft fading. Default: `0.0`.
   */
  softEdgeDistance?: number;
  /**
   * Scene depth texture for soft-edge fading. Required when
   * `softEdgeDistance > 0`. Default: `null`.
   */
  depthTexture?: THREE.DepthTexture | null;
  /** Camera near plane (linearizes the depth sample). Default: `0.1`. */
  cameraNear?: number;
  /** Camera far plane (linearizes the depth sample). Default: `1000`. */
  cameraFar?: number;
  /**
   * Render target resolution for screen-UV depth sampling. Updated by the
   * caller on viewport resize. Default: `[1, 1]` (caller MUST update before
   * the first render if soft-edge fading is enabled).
   */
  resolution?: [number, number];
}

/** Defaults applied when an option is omitted. Color default is set inline. */
const DEFAULTS = {
  size: 1.0,
  lifetimeProgress: 0.0,
  noiseScale: 2.0,
  noiseOctaves: 4,
  densityFalloff: 0.6,
  softEdgeDistance: 0.0,
  cameraNear: 0.1,
  cameraFar: 1000,
} as const;

const VERTEX_SHADER = /* glsl */ `
uniform float uSize;
varying vec2 vUv;
varying float vEyeDepth;

void main() {
  vUv = uv;

  // Camera-facing billboard: place the quad center at the model origin in
  // view space, then offset by the quad-local position scaled by uSize.
  vec4 mvCenter = modelViewMatrix * vec4(0.0, 0.0, 0.0, 1.0);
  vec3 offset = vec3(position.xy * uSize, 0.0);
  vec4 mvPosition = mvCenter + vec4(offset, 0.0);
  vEyeDepth = -mvPosition.z;
  gl_Position = projectionMatrix * mvPosition;
}
`;

const FRAGMENT_SHADER = /* glsl */ `
uniform vec3 uColor;
uniform float uLifetimeProgress;
uniform float uNoiseScale;
uniform float uDensityFalloff;
uniform float uSoftEdgeDistance;
uniform float uHasDepthFade;
uniform sampler2D uDepthTexture;
uniform vec2 uResolution;
uniform float uCameraNear;
uniform float uCameraFar;

varying vec2 vUv;
varying float vEyeDepth;

// Compact 2D hash → [0, 1).
float hash21(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

// Smoothed value noise.
float valueNoise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(hash21(i + vec2(0.0, 0.0)), hash21(i + vec2(1.0, 0.0)), u.x),
    mix(hash21(i + vec2(0.0, 1.0)), hash21(i + vec2(1.0, 1.0)), u.x),
    u.y
  );
}

float fbm(vec2 p) {
  float sum = 0.0;
  float amp = 0.5;
  float freq = 1.0;
  for (int i = 0; i < OCTAVES; i++) {
    sum += amp * valueNoise(p * freq);
    freq *= 2.0;
    amp *= 0.5;
  }
  return sum;
}

// Convert a non-linear depth-buffer sample to view-space (eye) distance.
float linearizeDepth(float depthSample, float near, float far) {
  float z = depthSample * 2.0 - 1.0;
  return (2.0 * near * far) / (far + near - z * (far - near));
}

void main() {
  // Radial fall-off from quad center.
  float r = length(vUv - 0.5);
  float radial = 1.0 - smoothstep(0.0, 0.5, r);

  // FBM density in [0, 1].
  float density = fbm(vUv * uNoiseScale);

  // densityFalloff = 0 → density ignored (uniform); 1 → density only.
  float alpha = radial * mix(1.0, density, clamp(uDensityFalloff, 0.0, 1.0));
  alpha = clamp(alpha * (1.0 - clamp(uLifetimeProgress, 0.0, 1.0)), 0.0, 1.0);

  // Soft-particle fade against scene depth.
  if (uHasDepthFade > 0.5) {
    vec2 screenUv = gl_FragCoord.xy / uResolution;
    float depthSample = texture2D(uDepthTexture, screenUv).r;
    float sceneDepth = linearizeDepth(depthSample, uCameraNear, uCameraFar);
    float diff = abs(sceneDepth - vEyeDepth);
    float fade = smoothstep(0.0, max(uSoftEdgeDistance, 1e-4), diff);
    alpha *= fade;
  }

  // Premultiplied alpha output — pairs with the CustomBlending below.
  gl_FragColor = vec4(uColor * alpha, alpha);
}
`;

/**
 * Construct a reference billboard ShaderMaterial.
 *
 * The returned material is wired for additive transparency with no depth
 * write. To change the color or lifetime progress at runtime, mutate the
 * uniform values directly:
 *
 * ```ts
 * material.uniforms.uLifetimeProgress.value = 0.7;
 * material.uniforms.uColor.value.set(1.0, 0.5, 0.0);
 * ```
 *
 * Changing `noiseOctaves` requires a fresh material — it is compiled into
 * the shader via `defines`.
 */
export function createBillboardFbmMaterial(
  opts: BillboardFbmMaterialOptions = {},
): THREE.ShaderMaterial {
  const octaves = Math.max(
    1,
    Math.min(8, Math.floor(opts.noiseOctaves ?? DEFAULTS.noiseOctaves)),
  );

  const color = new THREE.Color();
  if (opts.color !== undefined) {
    color.set(opts.color);
  } else {
    color.setRGB(0.95, 0.92, 0.85);
  }

  const size = opts.size ?? DEFAULTS.size;
  const lifetimeProgress = opts.lifetimeProgress ?? DEFAULTS.lifetimeProgress;
  const noiseScale = opts.noiseScale ?? DEFAULTS.noiseScale;
  const densityFalloff = opts.densityFalloff ?? DEFAULTS.densityFalloff;
  const softEdgeDistance = opts.softEdgeDistance ?? DEFAULTS.softEdgeDistance;
  const depthTexture = opts.depthTexture ?? null;
  const cameraNear = opts.cameraNear ?? DEFAULTS.cameraNear;
  const cameraFar = opts.cameraFar ?? DEFAULTS.cameraFar;
  const resolution: [number, number] = opts.resolution ?? [1, 1];

  const hasDepthFade = softEdgeDistance > 0 && depthTexture !== null;

  return new THREE.ShaderMaterial({
    defines: { OCTAVES: octaves },
    uniforms: {
      uColor: { value: color },
      uSize: { value: size },
      uLifetimeProgress: { value: lifetimeProgress },
      uNoiseScale: { value: noiseScale },
      uDensityFalloff: { value: densityFalloff },
      uSoftEdgeDistance: { value: softEdgeDistance },
      uHasDepthFade: { value: hasDepthFade ? 1.0 : 0.0 },
      uDepthTexture: { value: depthTexture },
      uResolution: { value: new THREE.Vector2(resolution[0], resolution[1]) },
      uCameraNear: { value: cameraNear },
      uCameraFar: { value: cameraFar },
    },
    vertexShader: VERTEX_SHADER,
    fragmentShader: FRAGMENT_SHADER,
    transparent: true,
    depthWrite: false,
    // Premultiplied alpha — independent of WebGLRenderer.premultipliedAlpha.
    blending: THREE.CustomBlending,
    blendSrc: THREE.OneFactor,
    blendDst: THREE.OneMinusSrcAlphaFactor,
    blendEquation: THREE.AddEquation,
  });
}
