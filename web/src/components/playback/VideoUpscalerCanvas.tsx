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

// Pass 1 (RGBA16F):
// 1. RemoveGrain(mode=2) 2nd-extrema impulse clamp (strips 1-2px film grain & mosquito speckles)
// 2. Weber relative contrast (ΔY / Y) gate: smooths grain/blocks on skin & walls (<7.2% Weber)
//    while preserving & restoring hair strands, eyes, eyelashes, eyebrows, lips, and fabric (>=8% Weber)
// 3. Despiked 13-tap Adaptive-Sharpen on Weber-gated structural features
const RESTORE_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 fragColor;
uniform sampler2D uSource;
uniform vec2 uTexelSize;

float luma(vec3 c) {
  return dot(c, vec3(0.2126, 0.7152, 0.0722));
}

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
  vec3 center = rgbToYCbCr(texture(uSource, vUv).rgb);
  float cY = center.x;

  vec3 n  = rgbToYCbCr(texture(uSource, vUv + vec2( 0.0, -1.0) * uTexelSize).rgb);
  vec3 s  = rgbToYCbCr(texture(uSource, vUv + vec2( 0.0,  1.0) * uTexelSize).rgb);
  vec3 e  = rgbToYCbCr(texture(uSource, vUv + vec2( 1.0,  0.0) * uTexelSize).rgb);
  vec3 w  = rgbToYCbCr(texture(uSource, vUv + vec2(-1.0,  0.0) * uTexelSize).rgb);
  vec3 nw = rgbToYCbCr(texture(uSource, vUv + vec2(-1.0, -1.0) * uTexelSize).rgb);
  vec3 ne = rgbToYCbCr(texture(uSource, vUv + vec2( 1.0, -1.0) * uTexelSize).rgb);
  vec3 sw = rgbToYCbCr(texture(uSource, vUv + vec2(-1.0,  1.0) * uTexelSize).rgb);
  vec3 se = rgbToYCbCr(texture(uSource, vUv + vec2( 1.0,  1.0) * uTexelSize).rgb);

  float nnY = luma(texture(uSource, vUv + vec2( 0.0, -2.0) * uTexelSize).rgb);
  float ssY = luma(texture(uSource, vUv + vec2( 0.0,  2.0) * uTexelSize).rgb);
  float eeY = luma(texture(uSource, vUv + vec2( 2.0,  0.0) * uTexelSize).rgb);
  float wwY = luma(texture(uSource, vUv + vec2(-2.0,  0.0) * uTexelSize).rgb);

  // 1. RemoveGrain(mode=2): 1st and 2nd extrema of the 8 neighbors
  float vals[8] = float[8](n.x, s.x, e.x, w.x, nw.x, ne.x, sw.x, se.x);
  float min1 = 1.0, min2 = 1.0;
  float max1 = 0.0, max2 = 0.0;
  for (int i = 0; i < 8; i++) {
    float v = vals[i];
    if (v < min1) { min2 = min1; min1 = v; }
    else if (v < min2) { min2 = v; }
    if (v > max1) { max2 = max1; max1 = v; }
    else if (v > max2) { max2 = v; }
  }

  float despikeY = clamp(cY, min2, max2);
  float blur3x3 = (2.0 * (n.x + s.x + e.x + w.x) + (nw.x + ne.x + sw.x + se.x) + 4.0 * despikeY) / 16.0;

  // 2. Despiked local range & Weber contrast (ΔY / Y)
  float cleanRangeY = max2 - min2;
  float weber = cleanRangeY / max(blur3x3, 0.028);

  // Smooth skin & walls have weber < 0.072 -> featureGate = 0 (denoised, zero sharpen)
  // Hair waves, eyes, eyelashes, eyebrows, lips, fabric weave have weber >= 0.080 -> sharpened
  float featureGate = smoothstep(0.072, 0.140, weber) * smoothstep(0.010, 0.026, blur3x3);

  // 3. KrigBilateral Chroma denoising + low-Weber surface smoothing (cleans skin & walls)
  float darkFactor = smoothstep(0.30, 0.02, blur3x3);
  float sigmaY = mix(0.016, 0.028, darkFactor);
  float sigmaC = mix(0.042, 0.068, darkFactor);
  float inv2SigmaY2 = 1.0 / (2.0 * sigmaY * sigmaY);
  float inv2SigmaC2 = 1.0 / (2.0 * sigmaC * sigmaC);

  float sumY = despikeY;
  float wSumY = 1.0;
  vec2 sumC = center.yz;
  float wSumC = 1.0;
  vec3 ring[8] = vec3[8](n, s, e, w, nw, ne, sw, se);
  for (int i = 0; i < 8; i++) {
    float dy = ring[i].x - despikeY;
    vec2 dc = ring[i].yz - center.yz;
    float wy = exp(-(dy * dy) * inv2SigmaY2);
    float wc = exp(-(dy * dy) * inv2SigmaY2 * 0.6 - dot(dc, dc) * inv2SigmaC2);
    sumY += ring[i].x * wy;
    wSumY += wy;
    sumC += ring[i].yz * wc;
    wSumC += wc;
  }

  float baseLuma = mix(sumY / wSumY, despikeY, featureGate);
  vec2 cleanC = sumC / wSumC;

  // 4. Compass debanding on flat walls & dark gradients (where featureGate == 0)
  float flatMask = (1.0 - featureGate) * (1.0 - smoothstep(0.008, 0.020, cleanRangeY));
  if (flatMask > 0.01) {
    vec2 pixelCoord = vUv / uTexelSize;
    vec2 rnd = hash22(pixelCoord + vec2(19.37, 47.13));
    float radius = 3.0 + 10.0 * rnd.x;
    float angle = rnd.y * 6.2831853;
    vec2 d = vec2(cos(angle), sin(angle)) * radius * uTexelSize;
    vec2 dOrtho = vec2(-d.y, d.x);

    vec3 p1 = rgbToYCbCr(texture(uSource, clamp(vUv + d,      vec2(0.0), vec2(1.0))).rgb);
    vec3 p2 = rgbToYCbCr(texture(uSource, clamp(vUv - d,      vec2(0.0), vec2(1.0))).rgb);
    vec3 p3 = rgbToYCbCr(texture(uSource, clamp(vUv + dOrtho, vec2(0.0), vec2(1.0))).rgb);
    vec3 p4 = rgbToYCbCr(texture(uSource, clamp(vUv - dOrtho, vec2(0.0), vec2(1.0))).rgb);

    vec3 avg = 0.25 * (p1 + p2 + p3 + p4);
    float maxDiffY = max(max(abs(p1.x - avg.x), abs(p2.x - avg.x)), max(abs(p3.x - avg.x), abs(p4.x - avg.x)));
    float diffY = abs(baseLuma - avg.x);
    float thresh = mix(0.014, 0.025, darkFactor);
    float bandFactor = (1.0 - smoothstep(thresh * 0.5, thresh, diffY)) *
                       (1.0 - smoothstep(thresh * 0.8, thresh * 1.6, maxDiffY)) *
                       flatMask;
    baseLuma = mix(baseLuma, avg.x, bandFactor * 0.82);
    cleanC = mix(cleanC, avg.yz, bandFactor * 0.85);
  }

  // 5. Despiked 13-tap Adaptive-Sharpen on Weber-gated structural features
  float outerAvg = 0.25 * (nnY + ssY + eeY + wwY);
  float laplacian = (despikeY - blur3x3) + 0.22 * (despikeY - clamp(outerAvg, min1, max1));
  float curveHeight = 0.95;
  float sharpenVal = (curveHeight / (curveHeight * 0.60 * cleanRangeY + 0.52)) * featureGate;
  float over = 0.008 * featureGate;
  float restoredY = clamp(baseLuma + laplacian * sharpenVal, min1 - over, max1 + over);

  fragColor = vec4(yCbCrToRgb(vec3(restoredY, cleanC)), 1.0);
}
`;

// Pass 2 (4K Output):
// 1. 5-tap Catmull-Rom Bicubic reconstruction
// 2. Weber-gated sub-pixel half-texel FidelityFX RCAS (-0.21 peak) for crisp 4K detail without skin grain
// 3. Dark-floor perceptual dithering
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

  vec3 bicubic = sampleCatmullRom5(uv);

  // Sample at sub-pixel 4K step (0.5 source texel) for true high-DPI edge definition
  vec2 subStep = uTexelSize * 0.5;
  vec3 n  = texture(uRestored, uv + vec2( 0.0, -1.0) * subStep).rgb;
  vec3 s  = texture(uRestored, uv + vec2( 0.0,  1.0) * subStep).rgb;
  vec3 e  = texture(uRestored, uv + vec2( 1.0,  0.0) * subStep).rgb;
  vec3 w  = texture(uRestored, uv + vec2(-1.0,  0.0) * subStep).rgb;
  vec3 nw = texture(uRestored, uv + vec2(-1.0, -1.0) * subStep).rgb;
  vec3 ne = texture(uRestored, uv + vec2( 1.0, -1.0) * subStep).rgb;
  vec3 sw = texture(uRestored, uv + vec2(-1.0,  1.0) * subStep).rgb;
  vec3 se = texture(uRestored, uv + vec2( 1.0,  1.0) * subStep).rgb;

  vec3 minBox = min(bicubic, min(min(min(n, s), min(e, w)), min(min(nw, ne), min(sw, se))));
  vec3 maxBox = max(bicubic, max(max(max(n, s), max(e, w)), max(max(nw, ne), max(sw, se))));

  vec3 base = clamp(bicubic, minBox, maxBox);

  float bY = luma(base);
  float nY = luma(n);
  float sY = luma(s);
  float eY = luma(e);
  float wY = luma(w);

  float minY = min(bY, min(min(nY, sY), min(eY, wY)));
  float maxY = max(bY, max(max(nY, sY), max(eY, wY)));
  float contrast = maxY - minY;
  float weber = contrast / max(bY, 0.028);

  // Smooth skin & walls (weber < 0.062) stay 100% untouched;
  // Hair strands, eyes, eyebrows, lips, fingers, and fabric weave get crisp 4K RCAS (-0.21)
  float detailMask = smoothstep(0.062, 0.130, weber) * smoothstep(0.010, 0.028, bY);
  float amp = clamp(min(minY, 1.0 - maxY) / max(maxY, 0.0001), 0.0, 1.0);
  amp = sqrt(amp) * detailMask;

  float wght = -amp * 0.21;
  float rcasY = ((nY + sY + eY + wY) * wght + bY) / (1.0 + 4.0 * wght);
  float over = 0.006 * detailMask;
  float sharpY = clamp(rcasY, minY - over, maxY + over);
  vec3 sharpened = clamp(base + (sharpY - bY), minBox - over, maxBox + over);

  float ditherMask = smoothstep(0.004, 0.025, sharpY) * (1.0 - detailMask);
  float triNoise = ign(gl_FragCoord.xy) - ign(gl_FragCoord.xy + vec2(17.0, 31.0));
  float dither = triNoise * (0.65 / 255.0) * ditherMask;

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

      const dpr = Math.max(window.devicePixelRatio || 1, 2.0);
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

      // Pass 1: Micro-detail Adaptive-Sharpen + Chroma Denoise + Flat-Shadow Deband
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

      // Pass 2: 4K Catmull-Rom + Sub-Pixel RCAS + Dark-Floor Dither
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
