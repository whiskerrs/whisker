// Tone curves and encodings shared by the scene and post-processing shaders.

// Stephen Hill's fit of the ACES reference rendering and output transforms.
fn aces(color: vec3<f32>) -> vec3<f32> {
    let input = mat3x3(
        vec3(0.59719, 0.07600, 0.02840),
        vec3(0.35458, 0.90834, 0.13383),
        vec3(0.04823, 0.01566, 0.83777),
    );
    let output = mat3x3(
        vec3(1.60475, -0.10208, -0.00327),
        vec3(-0.53108, 1.10813, -0.07276),
        vec3(-0.07367, -0.00605, 1.07602),
    );
    let v = input * color;
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return output * (a / b);
}

// Benjamin Wrensch's polynomial fit of Blender's AgX base contrast.
fn agx(color: vec3<f32>) -> vec3<f32> {
    let inset = mat3x3(
        vec3(0.842479062253094, 0.0423282422610123, 0.0423756549057051),
        vec3(0.0784335999999992, 0.878468636469772, 0.0784336),
        vec3(0.0792237451477643, 0.0791661274605434, 0.879142973793104),
    );
    let outset = mat3x3(
        vec3(1.19687900512017, -0.0528968517574562, -0.0529716355144438),
        vec3(-0.0980208811401368, 1.15190312990417, -0.0980434501171241),
        vec3(-0.0990297440797205, -0.0989611768448433, 1.15107367264116),
    );
    let min_ev = -12.47393;
    let max_ev = 4.026069;
    var v = inset * color;
    v = clamp(log2(max(v, vec3(1e-10))), vec3(min_ev), vec3(max_ev));
    v = (v - min_ev) / (max_ev - min_ev);
    let v2 = v * v;
    let v4 = v2 * v2;
    v = 15.5 * v4 * v2 - 40.14 * v4 * v + 31.96 * v4 - 6.868 * v2 * v + 0.4298 * v2 + 0.1191 * v
        - 0.00232;
    v = outset * v;
    // The curve's output is display-encoded; decoding it lets every mode share the sRGB step.
    return pow(max(v, vec3(0.0)), vec3(2.2));
}

fn encode_srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3(0.0031308));
}

fn decode_srgb(encoded: vec3<f32>) -> vec3<f32> {
    let low = encoded / 12.92;
    let high = pow((encoded + 0.055) / 1.055, vec3(2.4));
    return select(high, low, encoded <= vec3(0.04045));
}

// Exposes `color` and maps it with the curve `mode`: 0 none, 1 Reinhard, 2 ACES, 3 AgX.
fn tone_curve(color: vec3<f32>, exposure: f32, mode: i32) -> vec3<f32> {
    let exposed = color * exposure;
    if mode == 1 {
        return exposed / (1.0 + exposed);
    } else if mode == 2 {
        return aces(exposed);
    } else if mode == 3 {
        return agx(exposed);
    }
    return exposed;
}
