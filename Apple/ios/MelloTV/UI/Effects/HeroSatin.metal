#include <metal_stdlib>
#include <SwiftUI/SwiftUI_Metal.h>
using namespace metal;

static float satinHash(float2 p) {
    p = fract(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

static float satinNoise(float2 p) {
    float2 cell = floor(p);
    float2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(satinHash(cell), satinHash(cell + float2(1, 0)), f.x),
               mix(satinHash(cell + float2(0, 1)), satinHash(cell + float2(1, 1)), f.x), f.y);
}

static float satinField(float2 p) {
    float field = 0.0;
    float strength = 0.55;
    for (int octave = 0; octave < 4; ++octave) {
        field += satinNoise(p) * strength;
        p = float2(p.x * 1.63 - p.y * 1.12, p.x * 1.12 + p.y * 1.63) + 7.31;
        strength *= 0.48;
    }
    return field;
}

// An abstract mineral-satin surface: irregular depth without repeating ridges.
[[ stitchable ]] half4 satinSurface(float2 position, half4 source, float2 size, float2 tilt, float dark, float strength) {
    float2 uv = position / max(size, float2(1));
    float2 movement = clamp(tilt * 1.1, float2(-0.36), float2(0.36));
    float2 p = position / 170.0;
    float2 warp = float2(satinNoise(p * 0.7 + 4.3), satinNoise(p * 0.7 + 19.1));
    p += warp * 1.3;
    float field = satinField(p);
    float dx = satinField(p + float2(0.035, 0)) - field;
    float dy = satinField(p + float2(0, 0.035)) - field;
    float3 normal = normalize(float3(-dx * 14.0, -dy * 14.0, 1.0));
    float3 light = normalize(float3(-0.45 + movement.x * 2, -0.35 + movement.y * 2, 1));
    float diffuse = dot(normal, light);
    float sheen = pow(max(0.0, dot(reflect(-light, normal), float3(0, 0, 1))), 12.0);
    float2 d = uv - (float2(0.34, 0.28) + movement);
    float atmosphere = exp(-(d.x * d.x * 3 + d.y * d.y * 3.6));
    float pigment = smoothstep(0.20, 0.80, field);
    float3 cool = mix(float3(0.22, 0.49, 0.82), float3(0.10, 0.19, 0.27), dark);
    float3 base = float3(source.rgb);
    float3 result = mix(base, cool, (0.18 + pigment * 0.45) * mix(0.38, 0.52, dark));
    float grain = (satinHash(floor(position * 2)) - 0.5) * mix(0.025, 0.034, dark);
    result += (field - 0.5) * mix(0.075, 0.095, dark) + grain;
    result += (diffuse - 0.82) * mix(0.09, 0.14, dark);
    // Blue reflections retain contrast against the light canvas; dark mode keeps its pale sheen.
    float3 blueReflection = mix(result, float3(0.06, 0.38, 0.78), sheen * atmosphere * 0.20);
    float3 paleReflection = result + sheen * atmosphere * 0.080 * float3(0.80, 0.90, 1);
    result = mix(blueReflection, paleReflection, dark);
    return half4(half3(clamp(mix(base, result, strength), 0.0, 1.0)), source.a);
}

// Keep the artwork readable in the center, then diffuse it into the detail surface.
[[ stitchable ]] half4 detailArtworkBlur(float2 position, SwiftUI::Layer layer, float2 size) {
    float y = position.y / max(size.y, 1.0);
    float radius = 3.0 * (1.0 - smoothstep(0.0, 0.22, y))
                 + 26.0 * smoothstep(0.52, 0.95, y);
    half4 result = half4(0);
    float total = 0;
    for (int row = -2; row <= 2; ++row) {
        for (int col = -2; col <= 2; ++col) {
            float2 offset = float2(col, row);
            float weight = exp(-dot(offset, offset) * 0.5);
            float2 point = clamp(position + offset * radius * 0.5, float2(0.5), max(size - 0.5, float2(0.5)));
            result += layer.sample(point) * weight;
            total += weight;
        }
    }
    return result / total;
}
