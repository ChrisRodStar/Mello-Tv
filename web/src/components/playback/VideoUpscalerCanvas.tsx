import { useEffect, useRef, type RefObject } from 'react';

const VERTEX_SHADER = `#version 300 es
precision highp float;
out vec2 vUv;
void main() {
  vec2 pos = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
  vUv = vec2(pos.x, 1.0 - pos.y);
  gl_Position = vec4(pos * 2.0 - 1.0, 0.0, 1.0);
}
`;

// Pass 1 (RGBA16F intermediate):
// 1. BT.709 YCbCr separation + Luma-guided bilateral deblocking & 4:2:0 chroma denoising (KrigBilateral principle)
// 2. libplacebo / f3kdb 2-iteration randomized 4-point compass debanding for dark gradients
const RESTORE_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 fragColor;
uniform sampler2D uSource;
uniform vec2 uTexelSize;

vec3 rgbToYCbCr(vec3 rgb) {
  float y  = dot(rgb, vec3(0.2126, 0.7152, 0.0722));
  float cb = (rgb.b - y) / 1.8556;
  float cr = (rgb.r - y) / 1.5748;
  return vec3(y, cb, cr);
}

vec3 yCbCrToRgb(vec3 ycc) {
  float r = ycc.x + 1.5748 * ycc.z;
  float b = ycc.x + 1.8556 * ycc.y;
  float g = (ycc.x - 0.2126 * r - 0.0722 * b) / 0.7152;
  return clamp(vec3(r, g, b), 0.0, 1.0);
}

vec2 hash22(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * vec3(0.1031, 0.1030, 0.0973));
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.xx + p3.yz) * p3.zy);
}

void main() {
  vec3 centerRgb = texture(uSource, vUv).rgb;
  vec3 center = rgbToYCbCr(centerRgb);
  float centerY = center.x;

  // Dark scenes (Y < 0.30) suffer heaviest H.264 8x8 macroblocking & 4:2:0 chroma blotches
  float darkFactor = smoothstep(0.32, 0.02, centerY);
  float sigmaY = mix(0.020, 0.042, darkFactor);
  float sigmaC = mix(0.038, 0.075, darkFactor);
  float inv2SigmaY2 = 1.0 / (2.0 * sigmaY * sigmaY);
  float inv2SigmaC2 = 1.0 / (2.0 * sigmaC * sigmaC);

  // 1. Luma-guided bilateral deblocking + stronger chroma denoising
  const vec2 offsets[12] = vec2[12](
    vec2(-2.0,  0.0), vec2( 2.0,  0.0), vec2( 0.0, -2.0), vec2( 0.0,  2.0),
    vec2(-2.0, -2.0), vec2( 2.0, -2.0), vec2(-2.0,  2.0), vec2( 2.0,  2.0),
    vec2(-5.0,  0.0), vec2( 5.0,  0.0), vec2( 0.0, -5.0), vec2( 0.0,  5.0)
  );
  const float weights[12] = float[12](
    0.85, 0.85, 0.85, 0.85,
    0.70, 0.70, 0.70, 0.70,
    0.45, 0.45, 0.45, 0.45
  );

  float sumY = center.x;
  float wSumY = 1.0;
  vec2 sumC = center.yz;
  float wSumC = 1.0;

  for (int i = 0; i < 12; i++) {
    vec2 sampleUv = clamp(vUv + offsets[i] * uTexelSize, vec2(0.0), vec2(1.0));
    vec3 s = rgbToYCbCr(texture(uSource, sampleUv).rgb);
    float dy = s.x - center.x;
    vec2 dc = s.yz - center.yz;
    float wy = exp(-(dy * dy) * inv2SigmaY2) * weights[i];
    // Guide chroma weights by BOTH luma edge agreement and chroma similarity (KrigBilateral)
    float wc = exp(-(dy * dy) * inv2SigmaY2 * 0.7 - dot(dc, dc) * inv2SigmaC2) * weights[i];
    sumY += s.x * wy;
    wSumY += wy;
    sumC += s.yz * wc;
    wSumC += wc;
  }

  vec3 res = vec3(sumY / wSumY, sumC / wSumC);

  // 2. libplacebo / f3kdb 2-iteration randomized compass debanding
  // Expanding radius (12px -> 24px) with diminishing threshold to eliminate contour steps
  vec2 pixelCoord = vUv / uTexelSize;
  vec3 baseBound = vec3(mix(0.016, 0.028, darkFactor), 0.022, 0.022);

  for (int iter = 1; iter <= 2; iter++) {
    float fi = float(iter);
    vec2 rnd = hash22(pixelCoord + vec2(fi * 19.37, fi * 47.13));
    float radius = (0.25 + 0.75 * rnd.x) * (12.0 * fi);
    float angle = rnd.y * 6.2831853;
    vec2 d = vec2(cos(angle), sin(angle)) * radius * uTexelSize;
    vec2 dOrtho = vec2(-d.y, d.x);

    vec3 p1 = rgbToYCbCr(texture(uSource, clamp(vUv + d,      vec2(0.0), vec2(1.0))).rgb);
    vec3 p2 = rgbToYCbCr(texture(uSource, clamp(vUv - d,      vec2(0.0), vec2(1.0))).rgb);
    vec3 p3 = rgbToYCbCr(texture(uSource, clamp(vUv + dOrtho, vec2(0.0), vec2(1.0))).rgb);
    vec3 p4 = rgbToYCbCr(texture(uSource, clamp(vUv - dOrtho, vec2(0.0), vec2(1.0))).rgb);

    vec3 avg = 0.25 * (p1 + p2 + p3 + p4);
    vec3 maxDiff = max(
      max(abs(p1 - avg), abs(p2 - avg)),
      max(abs(p3 - avg), abs(p4 - avg))
    );
    vec3 diff = abs(res - avg);
    vec3 bound = baseBound / fi;

    // Smoothly blend toward compass average only when both center diff and local spread are within gradient threshold
    vec3 factor = (1.0 - smoothstep(bound * 0.5, bound, diff)) *
                  (1.0 - smoothstep(bound * 0.8, bound * 1.6, maxDiff));
    res = mix(res, avg, factor * 0.85);
  }

  fragColor = vec4(yCbCrToRgb(res), 1.0);
}
`;

// Pass 2 (Output 4K):
// 1. Jorge Jimenez 5-tap hardware-folded Catmull-Rom Bicubic upscale with 3x3 anti-ringing clamp
// 2. AMD FidelityFX RCAS (Robust Contrast-Adaptive Sharpening) in Luma space (prevents chroma halos)
// 3. Perceptual dark-aware triangular blue-noise dither (suppressed near pure black so letterboxing stays clean)
const UPSCALE_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 fragColor;
uniform sampler2D uRestored;
uniform vec2 uTexSize;
uniform vec2 uTexelSize;

float luma(vec3 c) {
  return dot(c, vec3(0.2126, 0.7152, 0.0722));
}

float ign(vec2 p) {
  return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}

vec3 sampleCatmullRom5(vec2 uv) {
  vec2 samplePos = uv * uTexSize;
  vec2 texPos1 = floor(samplePos - 0.5) + 0.5;
  vec2 f = samplePos - texPos1;

  vec2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
  vec2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
  vec2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
  vec2 w3 = f * f * (-0.5 + 0.5 * f);

  vec2 w12 = w1 + w2;
  vec2 offset12 = w2 / max(w12, vec2(0.0001));

  vec2 tc12 = (texPos1 + offset12) * uTexelSize;
  vec2 tc0  = (texPos1 - 1.0) * uTexelSize;
  vec2 tc3  = (texPos1 + 2.0) * uTexelSize;

  float wc = w12.x * w12.y;
  float wl = w0.x  * w12.y;
  float wr = w3.x  * w12.y;
  float wt = w12.x * w0.y;
  float wb = w12.x * w3.y;

  vec3 color =
    texture(uRestored, tc12).rgb * wc +
    texture(uRestored, vec2(tc0.x, tc12.y)).rgb * wl +
    texture(uRestored, vec2(tc3.x, tc12.y)).rgb * wr +
    texture(uRestored, vec2(tc12.x, tc0.y)).rgb * wt +
    texture(uRestored, vec2(tc12.x, tc3.y)).rgb * wb;

  return color / max(wc + wl + wr + wt + wb, 0.0001);
}

void main() {
  vec2 uv = vec2(vUv.x, 1.0 - vUv.y);

  // 1. 5-tap Catmull-Rom bicubic reconstruction
  vec3 bicubic = sampleCatmullRom5(uv);

  // 2. Fetch 3x3 neighborhood for anti-ringing clamp + Luma RCAS
  vec3 c  = texture(uRestored, uv).rgb;
  vec3 n  = texture(uRestored, uv + vec2( 0.0, -1.0) * uTexelSize).rgb;
  vec3 s  = texture(uRestored, uv + vec2( 0.0,  1.0) * uTexelSize).rgb;
  vec3 e  = texture(uRestored, uv + vec2( 1.0,  0.0) * uTexelSize).rgb;
  vec3 w  = texture(uRestored, uv + vec2(-1.0,  0.0) * uTexelSize).rgb;
  vec3 nw = texture(uRestored, uv + vec2(-1.0, -1.0) * uTexelSize).rgb;
  vec3 ne = texture(uRestored, uv + vec2( 1.0, -1.0) * uTexelSize).rgb;
  vec3 sw = texture(uRestored, uv + vec2(-1.0,  1.0) * uTexelSize).rgb;
  vec3 se = texture(uRestored, uv + vec2( 1.0,  1.0) * uTexelSize).rgb;

  vec3 minCross = min(c, min(min(n, s), min(e, w)));
  vec3 maxCross = max(c, max(max(n, s), max(e, w)));
  vec3 minBox = min(minCross, min(min(nw, ne), min(sw, se)));
  vec3 maxBox = max(maxCross, max(max(nw, ne), max(sw, se)));

  // Clamp Catmull-Rom negative lobes to prevent bright/dark ringing halos
  vec3 base = clamp(bicubic, minBox, maxBox);

  // 3. AMD FidelityFX RCAS in Luma space (prevents color fringes and noise amplification in deep shadows)
  float bY = luma(base);
  float nY = luma(n);
  float sY = luma(s);
  float eY = luma(e);
  float wY = luma(w);

  float minY = min(bY, min(min(nY, sY), min(eY, wY)));
  float maxY = max(bY, max(max(nY, sY), max(eY, wY)));
  float contrast = maxY - minY;

  // Attenuate sharpening in near-black or flat gradient areas so compression grain is never sharpened
  float edgeMask = smoothstep(0.018, 0.085, contrast) * smoothstep(0.015, 0.12, bY);
  float amp = clamp(min(minY, 1.0 - maxY) / max(maxY, 0.0001), 0.0, 1.0);
  amp = sqrt(amp) * edgeMask;

  float wght = -amp * 0.18;
  float sharpY = clamp(((nY + sY + eY + wY) * wght + bY) / (1.0 + 4.0 * wght), minY, maxY);
  vec3 sharpened = clamp(base + (sharpY - bY), minBox, maxBox);

  // 4. Perceptual dark-aware triangular dither (fades to 0 at pitch black Y < 0.008 so black bars stay pure)
  float ditherMask = smoothstep(0.004, 0.025, sharpY);
  float triNoise = ign(gl_FragCoord.xy) - ign(gl_FragCoord.xy + vec2(17.0, 31.0));
  float dither = triNoise * (1.1 / 255.0) * ditherMask;

  fragColor = vec4(clamp(sharpened + dither, 0.0, 1.0), 1.0);
}
`;

function compileProgram(gl: WebGL2RenderingContext, vsSource: string, fsSource: string) {
  const vs = gl.createShader(gl.VERTEX_SHADER);
  const fs = gl.createShader(gl.FRAGMENT_SHADER);
  const prog = gl.createProgram();
  if (!vs || !fs || !prog) return null;
  gl.shaderSource(vs, vsSource);
  gl.compileShader(vs);
  if (!gl.getShaderParameter(vs, gl.COMPILE_STATUS)) return null;
  gl.shaderSource(fs, fsSource);
  gl.compileShader(fs);
  if (!gl.getShaderParameter(fs, gl.COMPILE_STATUS)) return null;
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return null;
  gl.deleteShader(vs);
  gl.deleteShader(fs);
  return prog;
}

type VideoWithRVFC = HTMLVideoElement & {
  requestVideoFrameCallback?: (cb: () => void) => number;
  cancelVideoFrameCallback?: (id: number) => void;
};

export function VideoUpscalerCanvas({
  video,
  enabled,
}: {
  video: RefObject<HTMLVideoElement | null>;
  enabled: boolean;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!enabled) return;
    const canvas = canvasRef.current;
    const vid = video.current as VideoWithRVFC | null;
    if (!canvas || !vid) return;

    const gl = canvas.getContext('webgl2', {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      powerPreference: 'high-performance',
      preserveDrawingBuffer: false,
    });
    if (!gl) return;

    // Enable 16-bit floating-point render targets so Pass 1 debanding never re-quantizes to 8-bit
    const hasFloatFbo = !!gl.getExtension('EXT_color_buffer_float');

    const restoreProg = compileProgram(gl, VERTEX_SHADER, RESTORE_FRAGMENT_SHADER);
    const upscaleProg = compileProgram(gl, VERTEX_SHADER, UPSCALE_FRAGMENT_SHADER);
    if (!restoreProg || !upscaleProg) return;

    const vao = gl.createVertexArray();
    gl.bindVertexArray(vao);

    const srcTex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, srcTex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

    const fboTex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, fboTex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

    const fbo = gl.createFramebuffer();
    const uRestoreSource = gl.getUniformLocation(restoreProg, 'uSource');
    const uRestoreTexel = gl.getUniformLocation(restoreProg, 'uTexelSize');
    const uUpscaleRestored = gl.getUniformLocation(upscaleProg, 'uRestored');
    const uUpscaleTexSize = gl.getUniformLocation(upscaleProg, 'uTexSize');
    const uUpscaleTexel = gl.getUniformLocation(upscaleProg, 'uTexelSize');

    let fboW = 0;
    let fboH = 0;
    let disposed = false;
    let rafId = 0;
    let rvfcId = 0;

    const renderFrame = () => {
      if (disposed || !vid || vid.readyState < 2 || !vid.videoWidth || !vid.videoHeight) return;

      const vw = vid.videoWidth;
      const vh = vid.videoHeight;
      if (vw !== fboW || vh !== fboH) {
        fboW = vw;
        fboH = vh;
        gl.bindTexture(gl.TEXTURE_2D, fboTex);
        if (hasFloatFbo) {
          gl.texImage2D(
            gl.TEXTURE_2D,
            0,
            gl.RGBA16F,
            fboW,
            fboH,
            0,
            gl.RGBA,
            gl.HALF_FLOAT,
            null,
          );
        } else {
          gl.texImage2D(
            gl.TEXTURE_2D,
            0,
            gl.RGBA8,
            fboW,
            fboH,
            0,
            gl.RGBA,
            gl.UNSIGNED_BYTE,
            null,
          );
        }
        gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
        gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, fboTex, 0);
      }

      const dpr = Math.min(window.devicePixelRatio || 1, 2.5);
      const cw = Math.max(1, Math.min(3840, Math.round(canvas.clientWidth * dpr)));
      const ch = Math.max(1, Math.min(2160, Math.round(canvas.clientHeight * dpr)));
      if (canvas.width !== cw || canvas.height !== ch) {
        canvas.width = cw;
        canvas.height = ch;
      }

      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, srcTex);
      try {
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, vid);
      } catch {
        return;
      }

      // Pass 1: YCbCr Luma-Guided Deblock & Chroma Denoise + 2-Pass Compass Deband into RGBA16F FBO
      gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
      gl.viewport(0, 0, fboW, fboH);
      gl.useProgram(restoreProg);
      gl.uniform1i(uRestoreSource, 0);
      gl.uniform2f(uRestoreTexel, 1 / fboW, 1 / fboH);
      gl.drawArrays(gl.TRIANGLES, 0, 3);

      const videoAspect = vw / vh;
      const canvasAspect = cw / ch;
      let drawW = cw;
      let drawH = ch;
      let drawX = 0;
      let drawY = 0;
      if (canvasAspect > videoAspect) {
        drawW = Math.round(ch * videoAspect);
        drawX = Math.round((cw - drawW) * 0.5);
      } else {
        drawH = Math.round(cw / videoAspect);
        drawY = Math.round((ch - drawH) * 0.5);
      }

      // Pass 2: 5-Tap Catmull-Rom Bicubic + Anti-Ringing + Luma RCAS + Dark-Aware Dither
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.viewport(0, 0, cw, ch);
      gl.clearColor(0, 0, 0, 1);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.viewport(drawX, drawY, drawW, drawH);
      gl.useProgram(upscaleProg);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, fboTex);
      gl.uniform1i(uUpscaleRestored, 0);
      gl.uniform2f(uUpscaleTexSize, fboW, fboH);
      gl.uniform2f(uUpscaleTexel, 1 / fboW, 1 / fboH);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    };

    const scheduleNext = () => {
      if (disposed) return;
      if (typeof vid.requestVideoFrameCallback === 'function') {
        rvfcId = vid.requestVideoFrameCallback(() => {
          renderFrame();
          scheduleNext();
        });
      } else {
        rafId = requestAnimationFrame(() => {
          renderFrame();
          scheduleNext();
        });
      }
    };

    const onEvent = () => renderFrame();
    vid.addEventListener('loadeddata', onEvent);
    vid.addEventListener('seeked', onEvent);
    vid.addEventListener('play', onEvent);
    window.addEventListener('resize', onEvent);

    renderFrame();
    scheduleNext();

    return () => {
      disposed = true;
      if (rafId) cancelAnimationFrame(rafId);
      if (rvfcId && typeof vid.cancelVideoFrameCallback === 'function') {
        vid.cancelVideoFrameCallback(rvfcId);
      }
      vid.removeEventListener('loadeddata', onEvent);
      vid.removeEventListener('seeked', onEvent);
      vid.removeEventListener('play', onEvent);
      window.removeEventListener('resize', onEvent);
      gl.deleteFramebuffer(fbo);
      gl.deleteTexture(srcTex);
      gl.deleteTexture(fboTex);
      gl.deleteVertexArray(vao);
      gl.deleteProgram(restoreProg);
      gl.deleteProgram(upscaleProg);
    };
  }, [video, enabled]);

  if (!enabled) return null;
  return <canvas ref={canvasRef} className="player-upscale-canvas" aria-hidden="true" />;
}
