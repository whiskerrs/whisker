// Surface inputs from a standard material's factors and maps.
fn standard_surface(input: SurfaceInput) -> Surface {
    let texel = textureSample(texture0, material_sampler, input.uv);
    let metal_rough = textureSample(texture1, material_sampler, input.uv);
    let mapped = textureSample(texture2, material_sampler, input.uv).xyz * 2.0 - 1.0;
    let glow = textureSample(texture3, material_sampler, input.uv).rgb;
    let occlusion = textureSample(texture4, material_sampler, input.uv).r;
    var s: Surface;
    s.base_color = draw.base_color * texel * input.color;
    s.emissive = draw.emissive.rgb * glow;
    s.metallic = draw.params.x * metal_rough.b;
    s.roughness = draw.params.y * metal_rough.g;
    s.normal = mapped;
    s.occlusion = occlusion;
    s.clearcoat = draw.layers.x;
    s.clearcoat_roughness = draw.layers.y;
    s.sheen_color = draw.sheen.rgb;
    s.sheen_roughness = draw.sheen.w;
    s.transmission = draw.layers.z;
    s.ior = draw.layers.w;
    s.thickness = draw.anisotropy.z;
    s.anisotropy = draw.anisotropy.x;
    s.anisotropy_direction = vec2(cos(draw.anisotropy.y), sin(draw.anisotropy.y));
    return s;
}
