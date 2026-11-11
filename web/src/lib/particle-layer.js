// Particle rendering layer — Canvas 2D / WebGL2 switch for flow-layout
// particles (ui-layout.md §4 Flow Layout, "Rendering"):
//
//   "Canvas 2D overlay on top of the SVG node/edge layer. For >100
//    concurrent particles, switch to WebGL (via a `<canvas>` element with
//    getContext('webgl2')). ... The Canvas/WebGL layer handles particles
//    only (performance-critical, not interactive)."
//
// The 2D painter (`drawParticles2D`) is shared: ExplorerCanvas composites
// it into its main 2D context (≤100 particles) and FlowCanvas renders it
// on its overlay canvas. Above the threshold the ParticleLayer swaps its
// canvas element for a fresh one with a WebGL2 context — a canvas element
// permanently binds to the first context type it is given, so the switch
// requires a new element, not a second getContext on the same node.

// Spec threshold: ">100 concurrent particles, switch to WebGL".
export const WEBGL_PARTICLE_THRESHOLD = 100;

/**
 * Paint particles (bodies + fading trails) on a Canvas 2D context.
 *
 * Drawable shape (produced by the callers from flow-particles.js state):
 *   { x, y, radius, color, alpha, glowColor, glowBlur, highlight, trail }
 * `trail` is an array of {x1, y1, x2, y2, alpha} segments from
 * trailSegments() — opacity decay over 200ms, 20px length cap.
 *
 * @param {CanvasRenderingContext2D} ctx
 * @param {Array<any>} drawables
 */
export function drawParticles2D(ctx, drawables) {
  for (const d of drawables) {
    // Trail (under the body): fading polyline in the particle color.
    if (d.trail?.length > 0) {
      ctx.save();
      ctx.strokeStyle = d.color;
      ctx.lineCap = 'round';
      for (const sgm of d.trail) {
        ctx.globalAlpha = sgm.alpha * 0.6;
        ctx.lineWidth = Math.max(1, d.radius * 0.5);
        ctx.beginPath();
        ctx.moveTo(sgm.x1, sgm.y1);
        ctx.lineTo(sgm.x2, sgm.y2);
        ctx.stroke();
      }
      ctx.restore();
    }

    // Glow + body
    ctx.save();
    ctx.shadowColor = d.glowColor;
    ctx.shadowBlur = d.glowBlur;
    ctx.globalAlpha = d.alpha;
    ctx.fillStyle = d.color;
    ctx.beginPath();
    ctx.arc(d.x, d.y, d.radius, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    // Highlight ring for a selected span
    if (d.highlight) {
      ctx.save();
      ctx.globalAlpha = 0.6;
      ctx.strokeStyle = '#fbbf24';
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(d.x, d.y, d.radius + 4, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();
    }

    // Core
    ctx.save();
    ctx.globalAlpha = 1;
    ctx.fillStyle = '#ffffff';
    ctx.beginPath();
    ctx.arc(d.x, d.y, d.radius * 0.4, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }
}

function parseHexColor(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(String(hex));
  if (!m) return { r: 1, g: 1, b: 1 };
  const n = parseInt(m[1], 16);
  return { r: ((n >> 16) & 255) / 255, g: ((n >> 8) & 255) / 255, b: (n & 255) / 255 };
}

// One interleaved vertex layout for both points and lines:
// [x, y, size, r, g, b, a] — 7 floats. `size` is gl_PointSize for points
// and unused (0) for line vertices.
const VERT_SRC = `#version 300 es
uniform vec2 u_resolution;
uniform float u_dpr;
in vec2 a_pos;
in float a_size;
in vec4 a_color;
void main() {
  vec2 px = a_pos * u_dpr;
  gl_Position = vec4((px.x / u_resolution.x) * 2.0 - 1.0,
                     1.0 - (px.y / u_resolution.y) * 2.0,
                     0.0, 1.0);
  gl_PointSize = a_size * u_dpr;
}`;

const FRAG_SRC = `#version 300 es
precision mediump float;
in vec4 v_color;
out vec4 outColor;
void main() {
  vec2 d = gl_PointCoord - vec2(0.5);
  float r = length(d);
  if (r > 0.5) discard;
  outColor = vec4(v_color.rgb, v_color.a * smoothstep(0.5, 0.32, r));
}`;

/**
 * A particle overlay bound to a container element. Owns its `<canvas>`
 * (appended to the container); renders drawables via Canvas 2D up to the
 * spec threshold and via WebGL2 above it, creating a fresh canvas element
 * whenever the mode changes (a canvas keeps the first context type it is
 * given). When WebGL2 is unavailable (no context, or a non-GL object
 * returned — e.g. a test double), the layer stays on the 2D path.
 */
export class ParticleLayer {
  /**
   * @param {HTMLElement} container absolutely-positioned overlay host
   * @param {{className?: string}} [opts]
   */
  constructor(container, opts = {}) {
    this.container = container;
    this.className = opts.className ?? 'particle-layer-canvas';
    this.mode = null; // '2d' | 'webgl'
    this.canvas = null;
    this.ctx2d = null;
    this.gl = null;
    this.program = null;
    this.uniforms = null;
    this.pointBuf = null;
    this.lineBuf = null;
    this.webglUnavailable = false;
    this._switchMode('2d');
  }

  /** True when the layer currently renders through a WebGL2 context. */
  get isWebgl() {
    return this.mode === 'webgl';
  }

  _makeCanvas(mode) {
    const c = document.createElement('canvas');
    c.className = this.className;
    c.setAttribute('data-particle-mode', mode);
    c.style.position = 'absolute';
    c.style.inset = '0';
    c.style.width = '100%';
    c.style.height = '100%';
    c.style.pointerEvents = 'none';
    return c;
  }

  _switchMode(mode) {
    const canvas = this._makeCanvas(mode);
    if (this.canvas) {
      this.container.replaceChild(canvas, this.canvas);
    } else {
      this.container.appendChild(canvas);
    }
    this.canvas = canvas;
    this.gl = null;
    this.program = null;
    this.ctx2d = null;

    if (mode === 'webgl') {
      const gl = canvas.getContext('webgl2', { alpha: true, premultipliedAlpha: false });
      // Guard: getContext('webgl2') must return a real GL context. A mock
      // that returns a 2D-style object for any type (common in test
      // environments) would crash on shader calls — fall back to 2D.
      if (gl && typeof gl.createShader === 'function' && typeof gl.linkProgram === 'function') {
        this.gl = gl;
        this.mode = 'webgl';
        this._initGl();
        return;
      }
      // WebGL2 unavailable — replace the canvas again with a 2D one and
      // remember, so the threshold check stops asking.
      this.webglUnavailable = true;
      const fallback = this._makeCanvas('2d');
      this.container.replaceChild(fallback, canvas);
      this.canvas = fallback;
      this.ctx2d = fallback.getContext('2d');
      this.mode = '2d';
      return;
    }
    this.ctx2d = canvas.getContext('2d');
    this.mode = mode;
  }

  _initGl() {
    const gl = this.gl;
    const compile = (type, src) => {
      const sh = gl.createShader(type);
      gl.shaderSource(sh, src);
      gl.compileShader(sh);
      return sh;
    };
    const program = gl.createProgram();
    gl.attachShader(program, compile(gl.VERTEX_SHADER, VERT_SRC));
    gl.attachShader(program, compile(gl.FRAGMENT_SHADER, FRAG_SRC));
    gl.linkProgram(program);
    gl.useProgram(program);
    this.program = program;
    this.uniforms = {
      resolution: gl.getUniformLocation(program, 'u_resolution'),
      dpr: gl.getUniformLocation(program, 'u_dpr'),
    };
    this.pointBuf = gl.createBuffer();
    this.lineBuf = gl.createBuffer();
    const stride = 7 * 4;
    const posLoc = gl.getAttribLocation(program, 'a_pos');
    const sizeLoc = gl.getAttribLocation(program, 'a_size');
    const colorLoc = gl.getAttribLocation(program, 'a_color');
    this._attribs = { posLoc, sizeLoc, colorLoc, stride };
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
  }

  /**
   * Render a frame of drawables. Chooses the rendering mode from the
   * concurrent particle count (spec: >100 → WebGL) and sizes the backing
   * store to `width`×`height` CSS pixels at `dpr` device pixel ratio.
   *
   * @param {Array<any>} drawables
   * @param {{width: number, height: number, dpr?: number}} vp
   */
  render(drawables, vp) {
    const count = drawables?.length ?? 0;
    const want = count > WEBGL_PARTICLE_THRESHOLD && !this.webglUnavailable ? 'webgl' : '2d';
    if (want !== this.mode) this._switchMode(want);
    this._resize(vp);
    if (this.mode === 'webgl') {
      this._renderWebgl(drawables, vp);
    } else {
      this._render2d(drawables, vp);
    }
  }

  _resize(vp) {
    const dpr = vp.dpr ?? 1;
    const w = Math.max(1, Math.round(vp.width * dpr));
    const h = Math.max(1, Math.round(vp.height * dpr));
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
  }

  _render2d(drawables, vp) {
    const ctx = this.ctx2d;
    if (!ctx) return;
    const dpr = vp.dpr ?? 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, vp.width, vp.height);
    drawParticles2D(ctx, drawables ?? []);
  }

  _renderWebgl(drawables, vp) {
    const gl = this.gl;
    if (!gl) return;
    const dpr = vp.dpr ?? 1;
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform2f(this.uniforms.resolution, this.canvas.width, this.canvas.height);
    gl.uniform1f(this.uniforms.dpr, dpr);

    const { posLoc, sizeLoc, colorLoc, stride } = this._attribs;
    const bindAttribs = () => {
      gl.enableVertexAttribArray(posLoc);
      gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, stride, 0);
      gl.enableVertexAttribArray(sizeLoc);
      gl.vertexAttribPointer(sizeLoc, 1, gl.FLOAT, false, stride, 2 * 4);
      gl.enableVertexAttribArray(colorLoc);
      gl.vertexAttribPointer(colorLoc, 4, gl.FLOAT, false, stride, 3 * 4);
    };

    // Trail line segments first (under the points), then the point
    // passes: glow (large, faint), body, white core — matching the 2D
    // painter's stacking order within each drawable.
    const lineVerts = [];
    const pointVerts = [];
    for (const d of drawables) {
      const c = parseHexColor(d.color);
      if (d.trail?.length > 0) {
        const alphaScale = 0.6; // parity with the 2D trail alpha
        for (const sgm of d.trail) {
          const a = Math.max(0, Math.min(1, sgm.alpha * alphaScale));
          lineVerts.push(sgm.x1, sgm.y1, 0, c.r, c.g, c.b, a);
          lineVerts.push(sgm.x2, sgm.y2, 0, c.r, c.g, c.b, a);
        }
      }
      const highlight = d.highlight ? parseHexColor('#f59e0b') : c;
      const glowColor = d.highlight ? highlight : parseHexColor(d.glowColor ?? d.color);
      const alpha = d.alpha ?? 0.8;
      // glow
      pointVerts.push(d.x, d.y, d.radius * 2.2, glowColor.r, glowColor.g, glowColor.b, alpha * 0.35);
      // body
      pointVerts.push(d.x, d.y, d.radius * 2, highlight.r, highlight.g, highlight.b, alpha);
      // white core
      pointVerts.push(d.x, d.y, d.radius * 0.8, 1, 1, 1, 1);
      // highlight ring approximated by an amber halo
      if (d.highlight) {
        pointVerts.push(d.x, d.y, (d.radius + 4) * 2, 0.984, 0.753, 0.141, 0.6);
      }
    }

    if (lineVerts.length > 0) {
      gl.bindBuffer(gl.ARRAY_BUFFER, this.lineBuf);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(lineVerts), gl.DYNAMIC_DRAW);
      bindAttribs();
      gl.drawArrays(gl.LINES, 0, lineVerts.length / 7);
    }
    if (pointVerts.length > 0) {
      gl.bindBuffer(gl.ARRAY_BUFFER, this.pointBuf);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(pointVerts), gl.DYNAMIC_DRAW);
      bindAttribs();
      gl.drawArrays(gl.POINTS, 0, pointVerts.length / 7);
    }
  }

  /** Clear the overlay (keeps the current mode/context). */
  clear() {
    if (this.mode === 'webgl' && this.gl) {
      this.gl.clearColor(0, 0, 0, 0);
      this.gl.clear(this.gl.COLOR_BUFFER_BIT);
    } else if (this.ctx2d && this.canvas.width > 0 && this.canvas.height > 0) {
      this.ctx2d.setTransform(1, 0, 0, 1, 0, 0);
      this.ctx2d.clearRect(0, 0, this.canvas.width, this.canvas.height);
    }
  }

  destroy() {
    if (this.gl) {
      const lose = this.gl.getExtension('WEBGL_lose_context');
      lose?.loseContext();
      this.gl = null;
    }
    this.canvas?.remove();
    this.canvas = null;
    this.ctx2d = null;
  }
}
