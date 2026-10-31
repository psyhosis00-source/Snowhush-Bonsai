cbuffer Uniforms : register(b0) {
    float2 viewSize;
    float scale;
    float time;
    float columnWidth;
    int columnCount;
    float pileTop;
    int occluderCount;
    float2 origin;
    float pileBottom;
    int heightOffset;
    float4 light;
    float4 shadowTint;
    float4 occluders[16];
};

Buffer<float> heights : register(t0);

struct SpriteIn {
    float2 position : POSITION;
    float2 velocity : VELOCITY;
    float size : SIZE;
    float alpha : ALPHA;
    float soft : SOFT;
    float tintAmount : TINTAMOUNT;
    float3 tintColor : TINTCOLOR;
    float lump : LUMP;
    uint vid : SV_VertexID;
};

struct SpriteOut {
    float4 position : SV_Position;
    float2 coord : TEXCOORD0;
    float alpha : ALPHA;
    float soft : SOFT;
    float4 tint : TINT;
    float2 streak : STREAK;
    float radius : RADIUS;
    float lump : LUMP;
    float spin : SPIN;
};

SpriteOut spriteVertex(SpriteIn s) {
    float2 corner = float2(float(s.vid & 1), float(s.vid >> 1));
    SpriteOut o;
    if (s.lump <= -100.0) {
        // Flat pixel-art square (trees, needles, text): size is the side in DIPs, tint the colour.
        float extent = max(s.size * scale, 1.0);
        float2 center = s.position / viewSize * 2.0 - 1.0;
        o.position = float4(center + (corner - 0.5) * extent * 2.0 / (viewSize * scale), 0.0, 1.0);
        o.coord = float2(corner.x, 1.0 - corner.y);
        o.alpha = s.alpha;
        o.soft = 0.0;
        o.tint = float4(s.tintColor, 1.0);
        o.streak = float2(0.0, 0.0);
        o.radius = 0.5;
        o.lump = s.lump;
        o.spin = 0.0;
        return o;
    }
    if (s.lump < 0.0) {
        // Rain streak: thin velocity-aligned quad whose length scales with speed and tilts with wind.
        float speed = length(s.velocity);
        float2 dir = speed > 0.001 ? s.velocity / speed : float2(0.0, -1.0);
        float2 perp = float2(-dir.y, dir.x);
        float exposure = -s.lump * 0.034;
        float lenDip = clamp(speed * exposure, s.size * 2.0, 68.0);
        // Keep distant rain hair-thin while allowing foreground drops to read as liquid.
        float halfWidthDip = max(s.size * 0.92, 0.85 / max(scale, 0.001));
        float across = (corner.x * 2.0 - 1.0) * halfWidthDip;
        float along = lerp(-lenDip, halfWidthDip, corner.y);
        float2 worldPos = s.position + perp * across + dir * along;
        o.position = float4(worldPos / viewSize * 2.0 - 1.0, 0.0, 1.0);
        o.coord = float2(corner.x * 2.0 - 1.0, corner.y);
        o.streak = float2(0.0, 0.0);
        o.radius = 0.5;
        o.alpha = s.alpha * saturate(s.size * scale * 0.85);
        o.soft = s.soft;
        o.tint = float4(s.tintColor, s.tintAmount);
        o.lump = s.lump;
        o.spin = 0.0;
        return o;
    }

    float pixels = s.size * scale;
    // About a 1/55 s exposure, capped so even the fastest grain stays a short smear and never reads as rain.
    float2 travel = s.velocity * 0.018;
    float smear = min(length(travel), s.size * (s.lump > 0.0 ? 1.2 : 2.2));
    float2 streak = smear > 0.25 ? normalize(travel) * smear * scale : float2(0.0, 0.0);
    // A lump's ragged edge reaches past its nominal size.
    float reach = s.lump > 0.0 ? 1.3 : 1.0;
    float extent = max(pixels * reach + length(streak), 2.0);
    float2 center = s.position / viewSize * 2.0 - 1.0;
    o.position = float4(center + (corner - 0.5) * extent * 2.0 / (viewSize * scale), 0.0, 1.0);
    o.coord = float2(corner.x, 1.0 - corner.y);
    o.streak = streak / extent;
    o.radius = pixels / extent * 0.5;
    // A streak spreads the same flake over more pixels, so each is fainter.
    o.alpha = s.alpha * min(pixels / 2.0, 1.0) * pixels / max(pixels + length(streak) * 0.6, 0.001);
    o.soft = s.soft;
    o.tint = float4(s.tintColor, s.tintAmount);
    o.lump = s.lump;
    o.spin = s.lump > 0.0 ? time * (s.lump - 0.5) * 9.0 + s.lump * 40.0 : 0.0;
    return o;
}

float4 spriteFragment(SpriteOut i) : SV_Target {
    if (i.lump <= -100.0) {
        return float4(i.tint.rgb * light.rgb * i.alpha, i.alpha);
    }
    if (i.lump < 0.0) {
        float crossDist = abs(i.coord.x);
        float u = i.coord.y;
        // A rain streak is not a rectangle: it tapers at the tail, swells just
        // behind the head, then rounds off into a tiny, bright water bead.
        float tailTaper = smoothstep(0.0, 0.30, u);
        float headTaper = 1.0 - 0.34 * smoothstep(0.78, 1.0, u);
        float width = max(0.16, tailTaper * headTaper);
        float profile = crossDist / width;
        float edge = 1.0 - smoothstep(lerp(0.28, 0.12, i.soft), 1.0, profile);
        float tailFade = pow(saturate(u / 0.88), 1.18);
        float headRound = 1.0 - smoothstep(0.91, 1.0, u);
        float headGlow = exp(-pow((u - 0.91) * 10.0, 2.0)) * exp(-crossDist * 5.0);
        float a = edge * tailFade * headRound * i.alpha;
        if (a < 0.002) discard;
        float3 body = lerp(float3(0.62, 0.73, 0.87), i.tint.rgb, i.tint.a);
        float core = (1.0 - smoothstep(0.0, 0.58, crossDist)) * tailFade;
        float3 color = lerp(body, float3(0.96, 0.985, 1.0), core * 0.72);
        color += float3(0.30, 0.39, 0.52) * headGlow * 0.16;
        color *= light.rgb;
        a = saturate(a + headGlow * i.alpha * 0.12);
        return float4(color * a, a);
    }

    // Quad coordinates run down the screen; velocities run up.
    float2 p = float2(i.coord.x - 0.5, 0.5 - i.coord.y);
    float2 half_ = i.streak * 0.5;
    float span = dot(half_, half_);
    float t = span > 0.0 ? clamp(dot(p, half_) / span, -1.0, 1.0) : 0.0;
    float2 d = p - half_ * t;
    float r = length(d) / max(i.radius, 0.001);
    if (i.soft < 0.0) {
        // Four hard-edged pixel silhouettes. The packed flag also carries a per-flake
        // starting angle and angular velocity, so flakes gently tumble as they fall.
        uint packed = (uint)round(frac(-i.soft - 1.0) * 128.0);
        int variant = (int)(packed & 3);
        float startAngle = ((packed >> 2) & 7) * 0.78539816339;
        float spinRate = ((packed >> 5) & 3) == 0 ? -0.72 :
                         ((packed >> 5) & 3) == 1 ? -0.38 :
                         ((packed >> 5) & 3) == 2 ? 0.46 : 0.82;
        float angle = startAngle + time * spinRate;
        float sn = sin(angle);
        float cs = cos(angle);
        float2 pixelCoord = i.coord - 0.5;
        pixelCoord = float2(pixelCoord.x * cs - pixelCoord.y * sn,
                            pixelCoord.x * sn + pixelCoord.y * cs) + 0.5;
        int2 cell = int2(floor(pixelCoord * 7.0));
        int x = cell.x - 3;
        int y = cell.y - 3;
        int ax = abs(x);
        int ay = abs(y);
        bool inside = false;
        if (variant == 0) {
            inside = (x == 0 && y == 0);                         // single pixel
        } else if (variant == 1) {
            inside = (ax <= 1 && ay <= 1);                       // 2x2-ish pixel cluster
        } else if (variant == 2) {
            inside = (x == 0 && ay <= 2) || (y == 0 && ax <= 2); // small plus
        } else {
            inside = (x == 0 && ay <= 2) || (y == 0 && ax <= 2)
                || (ax == 1 && ay == 1);                         // plus with diagonal pixels
        }
        if (!inside) discard;
        float pixelAlpha = i.alpha * min(i.radius * 2.0, 1.0);
        float3 pixelColor = float3(0.97, 0.985, 1.0) * light.rgb;
        return float4(pixelColor * pixelAlpha, pixelAlpha);
    }
    float shading = 0.0;
    if (i.lump > 0.0) {
        float angle = atan2(d.y, d.x) + i.spin;
        r /= 1.0 + 0.14 * sin(3.0 * angle) + 0.08 * sin(5.0 * angle + i.lump * 21.0) + 0.05 * sin(2.0 * angle + i.lump * 9.0);
        shading = saturate(dot(d / max(i.radius, 0.001), float2(0.45, -0.6))) * 0.4;
    }
    // Brighter at the head, where the flake is now, than at the tail, where it was.
    float head = span > 0.0 ? 0.15 + 0.85 * pow(t * 0.5 + 0.5, 1.5) : 1.0;
    float a = (1.0 - smoothstep(lerp(0.6, 0.0, i.soft), 1.0, r)) * i.alpha * head;
    if (a < 0.002) discard;
    float3 core = float3(0.965, 0.975, 0.995);
    float3 rim = float3(0.62, 0.67, 0.77);
    float3 color = lerp(core, rim, saturate(smoothstep(0.35, 1.0, r) * 0.65 + shading));
    color = lerp(color, i.tint.rgb, i.tint.a) * light.rgb;
    return float4(color * a, a);
}

struct PileOut {
    float4 position : SV_Position;
    float2 local : TEXCOORD0;
};

PileOut pileVertex(uint vid : SV_VertexID) {
    float2 corner = float2(float(vid & 1), float(vid >> 1));
    float2 p = float2(corner.x * float(columnCount) * columnWidth, lerp(pileBottom, pileTop, corner.y));
    PileOut o;
    o.position = float4((p + origin) / viewSize * 2.0 - 1.0, 0.0, 1.0);
    o.local = p;
    return o;
}

float heightAt(int start, float x) {
    float f = x / columnWidth - 0.5;
    float base = floor(f);
    int i = clamp(int(base), 0, columnCount - 1);
    int j = min(i + 1, columnCount - 1);
    return lerp(heights[start + i], heights[start + j], saturate(f - base));
}

float hash(float2 p) {
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
}

float4 pileFragment(PileOut i) : SV_Target {
    float2 world = i.local + origin;
    [loop]
    for (int k = 0; k < occluderCount; k++) {
        float4 o = occluders[k];
        if (world.x >= o.x && world.x < o.z && world.y >= o.y && world.y < o.w) discard;
    }

    float x = i.local.x;
    float level = heightAt(heightOffset, x);
    float bed = heightAt(heightOffset + columnCount, x);
    float depth = level - i.local.y;
    float coverage = saturate(depth * scale + 0.5)
        * saturate((i.local.y - bed) * scale + 0.5)
        * saturate((level - bed) / 0.75);
    if (coverage <= 0.0) discard;

    float slope = (heightAt(heightOffset, x + 2.0) - heightAt(heightOffset, x - 2.0)) * 0.25;
    float lit = dot(normalize(float2(-slope, 1.0)), normalize(float2(-0.5, 1.0)));

    float3 shadow = lerp(float3(0.72, 0.78, 0.88), shadowTint.rgb, shadowTint.a);
    float3 surface = lerp(float3(0.925, 0.94, 0.965), min(shadowTint.rgb * 1.18, float3(1.0, 1.0, 1.0)), shadowTint.a * 0.2);
    float shade = (1.0 - exp(-depth / 30.0)) * 0.32 + (1.0 - lit) * 0.6 * exp(-depth / 5.0);
    float3 color = lerp(surface, shadow, saturate(shade));
    color = lerp(color, float3(0.975, 0.982, 0.995), exp(-depth * 0.9) * 0.6);
    color += (hash(floor(i.position.xy)) - 0.5) * 0.02;

    float seed = hash(floor(i.position.xy * 0.5) + 31.7);
    if (seed > 0.992) {
        float twinkle = pow(saturate(sin(time * (0.6 + seed * 1.8) + seed * 90.0)), 28.0);
        color = lerp(color, float3(1.0, 1.0, 1.0), twinkle * exp(-depth / 7.0));
    }

    color *= light.rgb;
    return float4(color * coverage, coverage);
}
