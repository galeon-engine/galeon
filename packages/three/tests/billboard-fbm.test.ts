// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { createBillboardFbmMaterial } from "../src/materials/billboard-fbm.js";

describe("createBillboardFbmMaterial — defaults", () => {
  test("returns a ShaderMaterial", () => {
    const m = createBillboardFbmMaterial();
    expect(m).toBeInstanceOf(THREE.ShaderMaterial);
  });

  test("warm off-white default color (0.95, 0.92, 0.85)", () => {
    const m = createBillboardFbmMaterial();
    const c = m.uniforms.uColor!.value as THREE.Color;
    expect(c.r).toBeCloseTo(0.95, 5);
    expect(c.g).toBeCloseTo(0.92, 5);
    expect(c.b).toBeCloseTo(0.85, 5);
  });

  test("default scalar uniforms match documented defaults", () => {
    const m = createBillboardFbmMaterial();
    expect(m.uniforms.uSize!.value).toBe(1.0);
    expect(m.uniforms.uLifetimeProgress!.value).toBe(0.0);
    expect(m.uniforms.uNoiseScale!.value).toBe(2.0);
    expect(m.uniforms.uDensityFalloff!.value).toBe(0.6);
    expect(m.uniforms.uSoftEdgeDistance!.value).toBe(0.0);
    expect(m.uniforms.uCameraNear!.value).toBe(0.1);
    expect(m.uniforms.uCameraFar!.value).toBe(1000);
  });

  test("OCTAVES define defaults to 4", () => {
    const m = createBillboardFbmMaterial();
    expect(m.defines).toMatchObject({ OCTAVES: 4 });
  });

  test("default depth fade is off (no depth texture)", () => {
    const m = createBillboardFbmMaterial();
    expect(m.uniforms.uHasDepthFade!.value).toBe(0.0);
    expect(m.uniforms.uDepthTexture!.value).toBeNull();
  });
});

describe("createBillboardFbmMaterial — option overrides", () => {
  test("color accepts hex number", () => {
    const m = createBillboardFbmMaterial({ color: 0xff0000 });
    const c = m.uniforms.uColor!.value as THREE.Color;
    expect(c.r).toBeCloseTo(1.0, 5);
    expect(c.g).toBeCloseTo(0.0, 5);
    expect(c.b).toBeCloseTo(0.0, 5);
  });

  test("color accepts THREE.Color instance", () => {
    const m = createBillboardFbmMaterial({ color: new THREE.Color(0, 1, 0) });
    const c = m.uniforms.uColor!.value as THREE.Color;
    expect(c.g).toBeCloseTo(1.0, 5);
  });

  test("size, lifetimeProgress, noiseScale, densityFalloff override", () => {
    const m = createBillboardFbmMaterial({
      size: 2.5,
      lifetimeProgress: 0.7,
      noiseScale: 4.0,
      densityFalloff: 0.2,
    });
    expect(m.uniforms.uSize!.value).toBe(2.5);
    expect(m.uniforms.uLifetimeProgress!.value).toBe(0.7);
    expect(m.uniforms.uNoiseScale!.value).toBe(4.0);
    expect(m.uniforms.uDensityFalloff!.value).toBe(0.2);
  });

  test("noiseOctaves clamps to [1, 8] and floors fractional input", () => {
    expect(createBillboardFbmMaterial({ noiseOctaves: 0 }).defines).toMatchObject({
      OCTAVES: 1,
    });
    expect(createBillboardFbmMaterial({ noiseOctaves: 999 }).defines).toMatchObject({
      OCTAVES: 8,
    });
    expect(createBillboardFbmMaterial({ noiseOctaves: 5.7 }).defines).toMatchObject({
      OCTAVES: 5,
    });
  });

  test("resolution override produces a Vector2 uniform", () => {
    const m = createBillboardFbmMaterial({ resolution: [1920, 1080] });
    const v = m.uniforms.uResolution!.value as THREE.Vector2;
    expect(v).toBeInstanceOf(THREE.Vector2);
    expect(v.x).toBe(1920);
    expect(v.y).toBe(1080);
  });
});

describe("createBillboardFbmMaterial — soft-edge fade", () => {
  test("disabled when softEdgeDistance is 0 even with depthTexture", () => {
    const depthTexture = new THREE.DepthTexture(256, 256);
    const m = createBillboardFbmMaterial({
      softEdgeDistance: 0,
      depthTexture,
    });
    expect(m.uniforms.uHasDepthFade!.value).toBe(0.0);
  });

  test("disabled when depthTexture is missing even with softEdgeDistance > 0", () => {
    const m = createBillboardFbmMaterial({ softEdgeDistance: 0.5 });
    expect(m.uniforms.uHasDepthFade!.value).toBe(0.0);
  });

  test("enabled when both softEdgeDistance > 0 and depthTexture present", () => {
    const depthTexture = new THREE.DepthTexture(256, 256);
    const m = createBillboardFbmMaterial({
      softEdgeDistance: 0.5,
      depthTexture,
      cameraNear: 0.5,
      cameraFar: 100,
    });
    expect(m.uniforms.uHasDepthFade!.value).toBe(1.0);
    expect(m.uniforms.uDepthTexture!.value).toBe(depthTexture);
    expect(m.uniforms.uSoftEdgeDistance!.value).toBe(0.5);
    expect(m.uniforms.uCameraNear!.value).toBe(0.5);
    expect(m.uniforms.uCameraFar!.value).toBe(100);
  });
});

describe("createBillboardFbmMaterial — render flags", () => {
  test("transparent with no depth write (typical for billboards)", () => {
    const m = createBillboardFbmMaterial();
    expect(m.transparent).toBe(true);
    expect(m.depthWrite).toBe(false);
  });

  test("uses premultiplied-alpha custom blending", () => {
    const m = createBillboardFbmMaterial();
    expect(m.blending).toBe(THREE.CustomBlending);
    expect(m.blendSrc).toBe(THREE.OneFactor);
    expect(m.blendDst).toBe(THREE.OneMinusSrcAlphaFactor);
    expect(m.blendEquation).toBe(THREE.AddEquation);
  });
});

describe("createBillboardFbmMaterial — shader source", () => {
  test("vertex shader is a camera-facing billboard", () => {
    const m = createBillboardFbmMaterial();
    expect(m.vertexShader).toContain("modelViewMatrix");
    expect(m.vertexShader).toContain("uSize");
    expect(m.vertexShader).toContain("vEyeDepth");
  });

  test("fragment shader contains FBM, lifetime fade, and depth-fade branch", () => {
    const m = createBillboardFbmMaterial();
    expect(m.fragmentShader).toContain("fbm");
    expect(m.fragmentShader).toContain("uLifetimeProgress");
    expect(m.fragmentShader).toContain("uHasDepthFade");
    expect(m.fragmentShader).toContain("linearizeDepth");
    // Premultiplied output.
    expect(m.fragmentShader).toContain("uColor * alpha, alpha");
  });

  test("FBM loop honours OCTAVES define", () => {
    const m = createBillboardFbmMaterial();
    expect(m.fragmentShader).toContain("i < OCTAVES");
  });
});

describe("createBillboardFbmMaterial — runtime mutation", () => {
  test("uniform mutations persist on the returned material", () => {
    const m = createBillboardFbmMaterial();
    m.uniforms.uLifetimeProgress!.value = 0.4;
    (m.uniforms.uColor!.value as THREE.Color).setRGB(0.1, 0.2, 0.3);
    expect(m.uniforms.uLifetimeProgress!.value).toBe(0.4);
    const c = m.uniforms.uColor!.value as THREE.Color;
    expect(c.r).toBeCloseTo(0.1, 5);
    expect(c.b).toBeCloseTo(0.3, 5);
  });

  test("each call returns an independent material (no shared uniform state)", () => {
    const a = createBillboardFbmMaterial({ size: 1.0 });
    const b = createBillboardFbmMaterial({ size: 3.0 });
    a.uniforms.uSize!.value = 99;
    expect(b.uniforms.uSize!.value).toBe(3.0);
  });
});
