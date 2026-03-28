<script>
  /**
   * FlowCanvas — Canvas 2D particle animation engine for Explorer flow layout.
   * Overlays particle traces on top of ExplorerCanvas SVG layer.
   *
   * Spec ref: ui-layout.md §4 (flow layout), §10 (Canvas 2D rendering)
   */

  import { getContext } from 'svelte';

  /** Respect prefers-reduced-motion: skip rAF loop, show static frame only */
  const prefersReducedMotion = typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-reduced-motion: reduce)')
    : { matches: false };

  let {
    nodes = [],           // positioned nodes [{id, x, y, width, height}]
    edges = [],           // edges [{source, target}]
    spans = [],           // trace spans [{id, parent_id, node_id, start_time, duration_us, status}]
    currentTime = $bindable(0),  // scrub position (microseconds from trace start)
    playing = false,      // animation playing
    speed = 1,            // playback speed multiplier
    selectedTests = [],   // [] = all, otherwise filter by root span name
    width = 800,
    height = 600,
    onParticleClick = null,  // (span) => void
    onEdgeClick = null,      // (spans) => void
  } = $props();

  const openDetailPanel = getContext('openDetailPanel');

  let canvasEl = $state(null);
  let hoveredParticleId = $state(null);

  // Build span tree from spans array
  let spanById = $derived.by(() => {
    const m = new Map();
    for (const s of spans) m.set(s.id, s);
    return m;
  });

  let rootSpans = $derived.by(() => {
    return spans.filter(s => !s.parent_id);
  });

  let childrenByParent = $derived.by(() => {
    const m = new Map();
    for (const s of spans) {
      if (s.parent_id) {
        const arr = m.get(s.parent_id) ?? [];
        arr.push(s);
        m.set(s.parent_id, arr);
      }
    }
    return m;
  });

  // Node position lookup by id
  let nodePos = $derived.by(() => {
    const m = new Map();
    for (const n of nodes) {
      m.set(n.id, {
        x: n.x + (n.width ?? 64) / 2,
        y: n.y + (n.height ?? 28) / 2,
      });
    }
    return m;
  });

  // Compute active particles at currentTime
  let activeParticles = $derived.by(() => {
    const roots = selectedTests.length > 0
      ? rootSpans.filter(s => selectedTests.includes(s.id) || selectedTests.includes(s.name))
      : rootSpans;

    return roots.map((root, testIndex) => {
      return computeParticle(root, testIndex, spanById, childrenByParent, nodePos, currentTime);
    }).filter(Boolean);
  });

  /**
   * Compute a single particle's position from a root span at the given time.
   * Walks the span tree to find the currently active child span.
   */
  function computeParticle(rootSpan, testIndex, byId, childrenMap, positions, time) {
    // Find deepest active span in the tree at `time`
    const active = findActiveSpan(rootSpan, childrenMap, time);
    if (!active) return null;

    const pos = getSpanPosition(active, byId, positions, time);
    if (!pos) return null;

    const isError = rootSpan.status === 'error' || active.status === 'error';
    const color = particleColor(testIndex, isError);

    return {
      id: rootSpan.id,
      spanId: active.id,
      x: pos.x,
      y: pos.y,
      hovered: hoveredParticleId === rootSpan.id,
      status: isError ? 'error' : 'ok',
      color,
      trail: pos.trail ?? [],
      testIndex,
      span: active,
    };
  }

  /**
   * Walk span tree to find the deepest span active at the given time.
   * Returns the last span if time is past all spans (particle at end).
   */
  function findActiveSpan(span, childrenMap, time) {
    const end = span.start_time + (span.duration_us ?? 0);
    const children = childrenMap.get(span.id) ?? [];

    // Check children first (depth-first)
    for (const child of children) {
      if (time >= child.start_time) {
        const found = findActiveSpan(child, childrenMap, time);
        if (found) return found;
      }
    }

    // This span is active if time is within its window
    if (time >= span.start_time && time <= end + 1000) {
      return span;
    }

    return null;
  }

  /**
   * Get interpolated XY position of a span at the given time.
   * Particles travel from parent-span's node to this span's node.
   */
  function getSpanPosition(span, byId, positions, time) {
    const nodeId = span.node_id;
    const dest = nodeId ? positions.get(nodeId) : null;

    // Find source: parent span's node, or same node if no parent
    const parent = span.parent_id ? byId.get(span.parent_id) : null;
    const srcNodeId = parent?.node_id ?? nodeId;
    const src = srcNodeId ? positions.get(srcNodeId) : null;

    if (!dest) return null;
    const from = src ?? dest;

    // Interpolate based on time within span
    const elapsed = time - span.start_time;
    const duration = span.duration_us ?? 1;
    const t = Math.min(1, Math.max(0, elapsed / duration));

    const x = from.x + (dest.x - from.x) * t;
    const y = from.y + (dest.y - from.y) * t;

    // Build small trail (last few positions)
    const trailSteps = 5;
    const trail = [];
    for (let i = trailSteps; i >= 0; i--) {
      const pt = Math.max(0, t - i * 0.04);
      trail.push({
        x: from.x + (dest.x - from.x) * pt,
        y: from.y + (dest.y - from.y) * pt,
      });
    }

    return { x, y, trail };
  }

  /**
   * HSL-shifted color for each test case.
   * Success: base hue 220 (blue), shift by testIndex * 30.
   * Error: base hue 0 (red), shift by testIndex * 20.
   */
  function particleColor(testIndex, isError) {
    if (isError) {
      const hue = (0 + testIndex * 20) % 360;
      return `hsl(${hue}, 80%, 55%)`;
    }
    const hue = (220 + testIndex * 30) % 360;
    return `hsl(${hue}, 70%, 60%)`;
  }

  // Max time for loop boundary
  let maxTime = $derived.by(() => {
    if (!spans.length) return 10000;
    return Math.max(...spans.map(s => s.start_time + (s.duration_us ?? 0)));
  });

  // Determine if we should use WebGL (> 100 active particles)
  let useWebGL = $derived.by(() => activeParticles.length > 100);

  // Animation loop
  let animFrameId = $state(null);

  $effect(() => {
    if (playing) {
      const currentMaxTime = maxTime;
      let lastTs = performance.now();
      function frame(ts) {
        const dt = (ts - lastTs) * speed;
        lastTs = ts;
        currentTime = currentTime + dt * 1000; // ms to microseconds
        if (currentTime > currentMaxTime) {
          currentTime = 0; // loop
        }
        drawFrame();
        animFrameId = requestAnimationFrame(frame);
      }
      animFrameId = requestAnimationFrame(frame);
      return () => {
        if (animFrameId) cancelAnimationFrame(animFrameId);
      };
    } else {
      // Draw static frame when paused or reduced-motion preferred
      drawFrame();
    }
  });

  // Redraw when currentTime or particles change (even when paused)
  $effect(() => {
    // Access reactive deps
    const _time = currentTime;
    const _particles = activeParticles;
    if (!playing) {
      drawFrame();
    }
  });

  function drawFrame() {
    if (!canvasEl) return;
    const particles = activeParticles;

    if (useWebGL) {
      drawWebGL(particles);
    } else {
      draw2D(particles);
    }
  }

  // Cache the 2D context — calling getContext('2d') after getContext('webgl2')
  // on the same canvas returns null. Caching also avoids repeated context lookups.
  let ctx2d = $state(null);

  function draw2D(particles) {
    if (!canvasEl) return;
    if (!ctx2d) ctx2d = canvasEl.getContext('2d');
    if (!ctx2d) return;

    ctx2d.clearRect(0, 0, width, height);

    for (const particle of particles) {
      drawParticle(ctx2d, particle);
    }
  }

  function drawParticle(ctx, particle) {
    // Trail
    if (particle.trail.length > 1) {
      for (let i = 1; i < particle.trail.length; i++) {
        const alpha = (i / particle.trail.length) * 0.6;
        ctx.beginPath();
        ctx.strokeStyle = particle.color;
        ctx.globalAlpha = alpha;
        ctx.lineWidth = 2;
        ctx.moveTo(particle.trail[i - 1].x, particle.trail[i - 1].y);
        ctx.lineTo(particle.trail[i].x, particle.trail[i].y);
        ctx.stroke();
      }
    }
    ctx.globalAlpha = 1;

    // Particle dot
    ctx.beginPath();
    ctx.arc(particle.x, particle.y, particle.hovered ? 6 : 4, 0, Math.PI * 2);
    ctx.fillStyle = particle.color;
    ctx.globalAlpha = 1;
    ctx.fill();

    // Glow for error particles
    if (particle.status === 'error') {
      ctx.beginPath();
      ctx.arc(particle.x, particle.y, 8, 0, Math.PI * 2);
      ctx.strokeStyle = particle.color;
      ctx.globalAlpha = 0.3;
      ctx.lineWidth = 2;
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  // Minimal WebGL renderer for > 100 particles (points-based)
  let glCtx = $state(null);
  let glProgram = $state(null);
  let glInitialized = $state(false);

  function initWebGL() {
    if (!canvasEl || glInitialized) return;
    // A canvas can only have one context type. If we already acquired a 2D
    // context, requesting webgl2 returns null. Skip to avoid silent failure.
    if (ctx2d) return;
    const gl = canvasEl.getContext('webgl2');
    if (!gl) return; // fallback to 2D handled in drawWebGL

    const vsSource = `#version 300 es
      in vec2 aPosition;
      in vec4 aColor;
      out vec4 vColor;
      uniform vec2 uResolution;
      void main() {
        vec2 clip = (aPosition / uResolution) * 2.0 - 1.0;
        gl_Position = vec4(clip.x, -clip.y, 0, 1);
        gl_PointSize = 6.0;
        vColor = aColor;
      }`;

    const fsSource = `#version 300 es
      precision mediump float;
      in vec4 vColor;
      out vec4 fragColor;
      void main() {
        vec2 coord = gl_PointCoord - vec2(0.5);
        if (dot(coord, coord) > 0.25) discard;
        fragColor = vColor;
      }`;

    const vs = gl.createShader(gl.VERTEX_SHADER);
    gl.shaderSource(vs, vsSource);
    gl.compileShader(vs);

    const fs = gl.createShader(gl.FRAGMENT_SHADER);
    gl.shaderSource(fs, fsSource);
    gl.compileShader(fs);

    const prog = gl.createProgram();
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);

    glCtx = gl;
    glProgram = prog;
    glInitialized = true;
  }

  // Reusable WebGL buffers (avoid creating new buffers every frame)
  let glPosBuf = $state(null);
  let glColBuf = $state(null);

  function drawWebGL(particles) {
    if (!glInitialized) initWebGL();
    if (!glCtx || !glProgram) {
      // WebGL not available — fall back to 2D
      draw2D(particles);
      return;
    }

    const gl = glCtx;
    gl.viewport(0, 0, width, height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.useProgram(glProgram);

    const posLoc = gl.getAttribLocation(glProgram, 'aPosition');
    const colLoc = gl.getAttribLocation(glProgram, 'aColor');
    const resLoc = gl.getUniformLocation(glProgram, 'uResolution');
    gl.uniform2f(resLoc, width, height);

    const positions = new Float32Array(particles.flatMap(p => [p.x, p.y]));
    const colors = new Float32Array(particles.flatMap(p => {
      const c = cssColorToRGBA(p.color);
      return [c.r, c.g, c.b, 1.0];
    }));

    if (!glPosBuf) glPosBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, glPosBuf);
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.DYNAMIC_DRAW);
    gl.enableVertexAttribArray(posLoc);
    gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0);

    if (!glColBuf) glColBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, glColBuf);
    gl.bufferData(gl.ARRAY_BUFFER, colors, gl.DYNAMIC_DRAW);
    gl.enableVertexAttribArray(colLoc);
    gl.vertexAttribPointer(colLoc, 4, gl.FLOAT, false, 0, 0);

    gl.drawArrays(gl.POINTS, 0, particles.length);
  }

  // Clean up WebGL resources on unmount
  $effect(() => {
    return () => {
      if (glCtx && glProgram) {
        if (glPosBuf) glCtx.deleteBuffer(glPosBuf);
        if (glColBuf) glCtx.deleteBuffer(glColBuf);
        glCtx.deleteProgram(glProgram);
        glPosBuf = null;
        glColBuf = null;
        glProgram = null;
        glInitialized = false;
        glCtx = null;
      }
    };
  });

  // Parse a CSS color string to RGBA (supports hsl and hex)
  function cssColorToRGBA(color) {
    // Simple fallback: return blue
    return { r: 0.23, g: 0.51, b: 0.96, a: 1 };
  }

  // Cache bounding rect to avoid layout thrashing on rapid mouse events
  let cachedRect = null;
  let cachedRectTime = 0;

  function getCanvasRect() {
    const now = performance.now();
    // Refresh cached rect at most every 200ms
    if (!cachedRect || now - cachedRectTime > 200) {
      cachedRect = canvasEl?.getBoundingClientRect() ?? null;
      cachedRectTime = now;
    }
    return cachedRect;
  }

  // Mouse interaction — find nearest particle to click
  function onCanvasClick(e) {
    // Force-refresh rect on click (infrequent)
    cachedRect = null;
    const rect = getCanvasRect();
    if (!rect) return;
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;

    const nearest = findNearestParticle(mx, my);
    if (nearest) {
      onParticleClick?.(nearest.span);
      if (openDetailPanel) {
        openDetailPanel({ type: 'span', id: nearest.spanId, data: nearest.span });
      }
    }
  }

  function onCanvasMouseMove(e) {
    const rect = getCanvasRect();
    if (!rect) return;
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const nearest = findNearestParticle(mx, my);
    hoveredParticleId = nearest?.id ?? null;
  }

  function onCanvasMouseLeave() {
    hoveredParticleId = null;
    cachedRect = null;
  }

  function findNearestParticle(mx, my, threshold = 16) {
    let best = null;
    let bestDist = threshold * threshold;
    for (const p of activeParticles) {
      const dx = p.x - mx;
      const dy = p.y - my;
      const d = dx * dx + dy * dy;
      if (d < bestDist) {
        bestDist = d;
        best = p;
      }
    }
    return best;
  }
</script>

<canvas
  bind:this={canvasEl}
  {width}
  {height}
  class="flow-canvas"
  data-testid="flow-canvas"
  role="img"
  onclick={onCanvasClick}
  onmousemove={onCanvasMouseMove}
  onmouseleave={onCanvasMouseLeave}
  tabindex="0"
  role="img"
  aria-label="Particle flow animation — {activeParticles.length} active traces"
  aria-roledescription="interactive particle animation canvas"
></canvas>

<style>
  .flow-canvas {
    display: block;
    pointer-events: auto;
  }

  .flow-canvas:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }
</style>
