/**
 * particle-layer.js — unit tests for the shared flow-particle renderer and
 * the Canvas 2D → WebGL2 switch (ui-layout.md §4 "Rendering": "Canvas 2D
 * overlay on top of the SVG node/edge layer. For >100 concurrent
 * particles, switch to WebGL (via a `<canvas>` element with
 * getContext('webgl2'))").
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  WEBGL_PARTICLE_THRESHOLD,
  drawParticles2D,
  ParticleLayer,
} from '../lib/particle-layer.js';

function makeCtx2d() {
  return {
    clearRect: vi.fn(), save: vi.fn(), restore: vi.fn(),
    beginPath: vi.fn(), arc: vi.fn(), fill: vi.fn(), stroke: vi.fn(),
    moveTo: vi.fn(), lineTo: vi.fn(),
    setTransform: vi.fn(),
    strokeStyle: '', fillStyle: '', lineWidth: 0, globalAlpha: 1,
    lineCap: '', shadowColor: '', shadowBlur: 0,
  };
}

function makeGl() {
  return {
    // constants used by the renderer
    VERTEX_SHADER: 1, FRAGMENT_SHADER: 2, ARRAY_BUFFER: 3, DYNAMIC_DRAW: 4,
    FLOAT: 5, COLOR_BUFFER_BIT: 16, BLEND: 17, SRC_ALPHA: 18,
    ONE_MINUS_SRC_ALPHA: 19, LINES: 20, POINTS: 21,
    // methods
    createShader: vi.fn(() => ({})),
    shaderSource: vi.fn(),
    compileShader: vi.fn(),
    createProgram: vi.fn(() => ({})),
    attachShader: vi.fn(),
    linkProgram: vi.fn(),
    useProgram: vi.fn(),
    getUniformLocation: vi.fn(() => ({})),
    uniform2f: vi.fn(),
    uniform1f: vi.fn(),
    getAttribLocation: vi.fn(() => 0),
    createBuffer: vi.fn(() => ({})),
    bindBuffer: vi.fn(),
    bufferData: vi.fn(),
    enableVertexAttribArray: vi.fn(),
    vertexAttribPointer: vi.fn(),
    enable: vi.fn(),
    blendFunc: vi.fn(),
    viewport: vi.fn(),
    clearColor: vi.fn(),
    clear: vi.fn(),
    drawArrays: vi.fn(),
    getExtension: vi.fn(() => undefined),
  };
}

const DRAWABLE = {
  x: 100, y: 80, radius: 5, color: '#3b82f6', alpha: 0.8,
  glowColor: '#3b82f6', glowBlur: 10, highlight: false,
  trail: [{ x1: 90, y1: 75, x2: 100, y2: 80, alpha: 0.5 }],
};

describe('drawParticles2D', () => {
  it('draws the fading trail under the particle body, then the glow, core', () => {
    const ctx = makeCtx2d();
    const trailStrokes = [];
    const bodyFills = [];
    ctx.stroke = vi.fn(() => {
      if (ctx.strokeStyle === '#3b82f6') trailStrokes.push({ alpha: ctx.globalAlpha, cap: ctx.lineCap });
    });
    ctx.fill = vi.fn(() => {
      if (ctx.fillStyle === '#3b82f6') bodyFills.push(ctx.globalAlpha);
    });
    drawParticles2D(ctx, [DRAWABLE]);
    // Trail: alpha = segment alpha * 0.6, round cap, particle color.
    expect(trailStrokes.length).toBe(1);
    expect(trailStrokes[0].alpha).toBeCloseTo(0.3);
    expect(trailStrokes[0].cap).toBe('round');
    // Body glow fill in the particle color at body alpha.
    expect(bodyFills).toEqual([0.8]);
    // White core drawn after the body.
    expect(ctx.fillStyle).toBe('#ffffff');
  });

  it('draws the amber highlight ring only for highlighted particles', () => {
    const ctx = makeCtx2d();
    drawParticles2D(ctx, [{ ...DRAWABLE, highlight: true, color: '#f59e0b' }]);
    // Two strokes: trail + ring.
    expect(ctx.stroke).toHaveBeenCalledTimes(2);
    expect(ctx.strokeStyle).toBe('#fbbf24');
    const plain = makeCtx2d();
    drawParticles2D(plain, [DRAWABLE]);
    expect(plain.stroke).toHaveBeenCalledTimes(1);
  });
});

describe('ParticleLayer', () => {
  let container;
  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });
  afterEach(() => {
    container.remove();
    vi.restoreAllMocks();
  });

  it('renders ≤100 particles on a Canvas 2D context', () => {
    const ctx = makeCtx2d();
    const spy = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(ctx);
    const layer = new ParticleLayer(container);
    const drawables = Array.from({ length: WEBGL_PARTICLE_THRESHOLD }, (_, i) => ({
      ...DRAWABLE, x: i, trail: [],
    }));
    layer.render(drawables, { width: 400, height: 300, dpr: 1 });
    expect(layer.mode).toBe('2d');
    expect(spy).toHaveBeenCalledWith('2d');
    expect(ctx.clearRect).toHaveBeenCalled();
    expect(ctx.fill).toHaveBeenCalled();
    expect(layer.canvas.getAttribute('data-particle-mode')).toBe('2d');
    layer.destroy();
  });

  it('switches to WebGL2 above 100 concurrent particles and draws points + trail lines', () => {
    const ctx = makeCtx2d();
    const gl = makeGl();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation((type) =>
      type === 'webgl2' ? gl : ctx);
    const layer = new ParticleLayer(container);
    const drawables = Array.from({ length: WEBGL_PARTICLE_THRESHOLD + 1 }, () => ({
      ...DRAWABLE, trail: [{ x1: 1, y1: 1, x2: 2, y2: 2, alpha: 0.5 }],
    }));
    layer.render(drawables, { width: 400, height: 300, dpr: 1 });
    expect(layer.mode).toBe('webgl');
    expect(layer.isWebgl).toBe(true);
    // A fresh canvas element was created for the WebGL context.
    expect(layer.canvas.getAttribute('data-particle-mode')).toBe('webgl');
    expect(gl.createShader).toHaveBeenCalled();
    expect(gl.linkProgram).toHaveBeenCalled();
    // Trails (LINES) drawn before bodies (POINTS).
    const kinds = gl.drawArrays.mock.calls.map((c) => c[0]);
    expect(kinds).toContain(gl.LINES);
    expect(kinds).toContain(gl.POINTS);
    expect(kinds.indexOf(gl.LINES)).toBeLessThan(kinds.indexOf(gl.POINTS));
    // Trail vertices: 2 per drawable; point vertices: 3 per drawable (glow,
    // body, core).
    const lineCall = gl.drawArrays.mock.calls.find((c) => c[0] === gl.LINES);
    const pointCall = gl.drawArrays.mock.calls.find((c) => c[0] === gl.POINTS);
    expect(lineCall[2]).toBe((WEBGL_PARTICLE_THRESHOLD + 1) * 2);
    expect(pointCall[2]).toBe((WEBGL_PARTICLE_THRESHOLD + 1) * 3);
    layer.destroy();
  });

  it('falls back to Canvas 2D when WebGL2 is unavailable', () => {
    const ctx = makeCtx2d();
    // getContext('webgl2') returns a 2D-style object without GL methods
    // (what a blanket test double yields) — must not crash, must stay 2D.
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(ctx);
    const layer = new ParticleLayer(container);
    const drawables = Array.from({ length: 500 }, (_, i) => ({ ...DRAWABLE, x: i, trail: [] }));
    layer.render(drawables, { width: 400, height: 300, dpr: 1 });
    expect(layer.mode).toBe('2d');
    expect(layer.webglUnavailable).toBe(true);
    expect(ctx.fill).toHaveBeenCalled();
    layer.destroy();
  });

  it('switches back to Canvas 2D when the count drops to the threshold', () => {
    const ctx = makeCtx2d();
    const gl = makeGl();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation((type) =>
      type === 'webgl2' ? gl : ctx);
    const layer = new ParticleLayer(container);
    const many = Array.from({ length: 150 }, (_, i) => ({ ...DRAWABLE, x: i, trail: [] }));
    layer.render(many, { width: 400, height: 300, dpr: 1 });
    expect(layer.mode).toBe('webgl');
    layer.render(many.slice(0, 100), { width: 400, height: 300, dpr: 1 });
    expect(layer.mode).toBe('2d');
    layer.destroy();
  });

  it('clear() blanks the current canvas in both modes', () => {
    const ctx = makeCtx2d();
    const gl = makeGl();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation((type) =>
      type === 'webgl2' ? gl : ctx);
    const layer = new ParticleLayer(container);
    layer.render([{ ...DRAWABLE, trail: [] }], { width: 100, height: 100, dpr: 1 });
    layer.clear();
    expect(ctx.clearRect).toHaveBeenCalled();
    const many = Array.from({ length: 150 }, (_, i) => ({ ...DRAWABLE, x: i, trail: [] }));
    layer.render(many, { width: 100, height: 100, dpr: 1 });
    layer.clear();
    expect(gl.clear).toHaveBeenCalledWith(gl.COLOR_BUFFER_BIT);
    layer.destroy();
  });
});
