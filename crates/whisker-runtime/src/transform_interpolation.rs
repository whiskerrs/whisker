//! CSS transform-list interpolation owned by the Rust runtime.
//!
//! Matching transform functions interpolate component-wise. At the first
//! mismatch (and for explicit matrices), the remaining suffix is resolved to
//! a 4x4 matrix, decomposed, blended, and recomposed as required by CSS
//! Transforms. The decomposition follows the Chromium/Lynx algorithm.

use whisker_engine::whisker_style::{
    ComputedLengthPercentage, ComputedTransformFunction, ComputedTransformStyle, StyleNumber,
};

#[derive(Clone, Copy, Debug)]
struct DecomposedTransform {
    translate: [f32; 3],
    scale: [f32; 3],
    skew: [f32; 3],
    perspective: [f32; 4],
    quaternion: [f32; 4],
}

impl Default for DecomposedTransform {
    fn default() -> Self {
        Self {
            translate: [0.0; 3],
            scale: [1.0; 3],
            skew: [0.0; 3],
            perspective: [0.0, 0.0, 0.0, 1.0],
            quaternion: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

pub(crate) fn interpolate_transform_style(
    from: &ComputedTransformStyle,
    to: &ComputedTransformStyle,
    progress: f32,
    reference_width: f32,
    reference_height: f32,
) -> Option<ComputedTransformStyle> {
    if !reference_width.is_finite()
        || !reference_height.is_finite()
        || reference_width < 0.0
        || reference_height < 0.0
    {
        return None;
    }

    let from_count = from.functions.len();
    let to_count = to.functions.len();
    let operation_count = from_count.max(to_count);
    let mut matching_prefix = 0;
    while matching_prefix < from_count.min(to_count)
        && same_direct_kind(
            &from.functions[matching_prefix],
            &to.functions[matching_prefix],
        )
    {
        matching_prefix += 1;
    }
    if matching_prefix == from_count.min(to_count) {
        while matching_prefix < operation_count {
            let unmatched = from
                .functions
                .get(matching_prefix)
                .or_else(|| to.functions.get(matching_prefix))
                .expect("index is bounded by the longest transform list");
            if matches!(unmatched, ComputedTransformFunction::Matrix(_)) {
                break;
            }
            matching_prefix += 1;
        }
    }

    let mut functions = Vec::with_capacity(matching_prefix.saturating_add(1));
    for index in 0..matching_prefix {
        let function = match (from.functions.get(index), to.functions.get(index)) {
            (Some(from), Some(to)) => interpolate_transform_function(from, to, progress)?,
            (Some(from), None) => {
                interpolate_transform_function(from, &identity_transform_function(from)?, progress)?
            }
            (None, Some(to)) => {
                interpolate_transform_function(&identity_transform_function(to)?, to, progress)?
            }
            (None, None) => unreachable!("index is bounded by the longest transform list"),
        };
        functions.push(function);
    }

    if matching_prefix < operation_count {
        let from_matrix = suffix_matrix(
            &from.functions[matching_prefix..],
            reference_width,
            reference_height,
        )?;
        let to_matrix = suffix_matrix(
            &to.functions[matching_prefix..],
            reference_width,
            reference_height,
        )?;
        let from_decomposed = decompose(from_matrix)?;
        let to_decomposed = decompose(to_matrix)?;
        functions.push(ComputedTransformFunction::Matrix(
            compose(blend(from_decomposed, to_decomposed, progress)).map(StyleNumber::new),
        ));
    }

    let mut current = to.clone();
    current.functions = functions;
    Some(current)
}

pub(crate) fn interpolate_transform_function(
    from: &ComputedTransformFunction,
    to: &ComputedTransformFunction,
    progress: f32,
) -> Option<ComputedTransformFunction> {
    let number = |from: StyleNumber, to: StyleNumber| {
        StyleNumber::new(from.get() + (to.get() - from.get()) * progress)
    };
    let length = |from: ComputedLengthPercentage, to: ComputedLengthPercentage| {
        ComputedLengthPercentage::new(
            from.length() + (to.length() - from.length()) * progress,
            from.fraction() + (to.fraction() - from.fraction()) * progress,
        )
    };
    match (from, to) {
        (
            ComputedTransformFunction::Translate {
                x: from_x,
                y: from_y,
                z: from_z,
            },
            ComputedTransformFunction::Translate {
                x: to_x,
                y: to_y,
                z: to_z,
            },
        ) => Some(ComputedTransformFunction::Translate {
            x: length(*from_x, *to_x),
            y: length(*from_y, *to_y),
            z: number(*from_z, *to_z),
        }),
        (ComputedTransformFunction::RotateX(from), ComputedTransformFunction::RotateX(to)) => {
            Some(ComputedTransformFunction::RotateX(number(*from, *to)))
        }
        (ComputedTransformFunction::RotateY(from), ComputedTransformFunction::RotateY(to)) => {
            Some(ComputedTransformFunction::RotateY(number(*from, *to)))
        }
        (ComputedTransformFunction::RotateZ(from), ComputedTransformFunction::RotateZ(to)) => {
            Some(ComputedTransformFunction::RotateZ(number(*from, *to)))
        }
        (
            ComputedTransformFunction::Scale {
                x: from_x,
                y: from_y,
                z: from_z,
            },
            ComputedTransformFunction::Scale {
                x: to_x,
                y: to_y,
                z: to_z,
            },
        ) => Some(ComputedTransformFunction::Scale {
            x: number(*from_x, *to_x),
            y: number(*from_y, *to_y),
            z: number(*from_z, *to_z),
        }),
        (
            ComputedTransformFunction::Skew {
                x_degrees: from_x,
                y_degrees: from_y,
            },
            ComputedTransformFunction::Skew {
                x_degrees: to_x,
                y_degrees: to_y,
            },
        ) => Some(ComputedTransformFunction::Skew {
            x_degrees: number(*from_x, *to_x),
            y_degrees: number(*from_y, *to_y),
        }),
        _ => None,
    }
}

pub(crate) fn identity_transform_function(
    function: &ComputedTransformFunction,
) -> Option<ComputedTransformFunction> {
    let zero = StyleNumber::new(0.0);
    let one = StyleNumber::new(1.0);
    match function {
        ComputedTransformFunction::Translate { .. } => Some(ComputedTransformFunction::Translate {
            x: ComputedLengthPercentage::ZERO,
            y: ComputedLengthPercentage::ZERO,
            z: zero,
        }),
        ComputedTransformFunction::RotateX(_) => Some(ComputedTransformFunction::RotateX(zero)),
        ComputedTransformFunction::RotateY(_) => Some(ComputedTransformFunction::RotateY(zero)),
        ComputedTransformFunction::RotateZ(_) => Some(ComputedTransformFunction::RotateZ(zero)),
        ComputedTransformFunction::Scale { .. } => Some(ComputedTransformFunction::Scale {
            x: one,
            y: one,
            z: one,
        }),
        ComputedTransformFunction::Skew { .. } => Some(ComputedTransformFunction::Skew {
            x_degrees: zero,
            y_degrees: zero,
        }),
        ComputedTransformFunction::Matrix(_) => None,
    }
}

fn same_direct_kind(from: &ComputedTransformFunction, to: &ComputedTransformFunction) -> bool {
    matches!(
        (from, to),
        (
            ComputedTransformFunction::Translate { .. },
            ComputedTransformFunction::Translate { .. }
        ) | (
            ComputedTransformFunction::RotateX(_),
            ComputedTransformFunction::RotateX(_)
        ) | (
            ComputedTransformFunction::RotateY(_),
            ComputedTransformFunction::RotateY(_)
        ) | (
            ComputedTransformFunction::RotateZ(_),
            ComputedTransformFunction::RotateZ(_)
        ) | (
            ComputedTransformFunction::Scale { .. },
            ComputedTransformFunction::Scale { .. }
        ) | (
            ComputedTransformFunction::Skew { .. },
            ComputedTransformFunction::Skew { .. }
        )
    )
}

fn suffix_matrix(
    functions: &[ComputedTransformFunction],
    reference_width: f32,
    reference_height: f32,
) -> Option<[f32; 16]> {
    let mut matrix = identity();
    for function in functions {
        matrix = multiply(
            matrix,
            function_matrix(function, reference_width, reference_height)?,
        );
    }
    Some(matrix)
}

fn function_matrix(
    function: &ComputedTransformFunction,
    reference_width: f32,
    reference_height: f32,
) -> Option<[f32; 16]> {
    let matrix = match function {
        ComputedTransformFunction::Translate { x, y, z } => translation(
            x.length() + x.fraction() * reference_width,
            y.length() + y.fraction() * reference_height,
            z.get(),
        ),
        ComputedTransformFunction::RotateX(degrees) => {
            let (sin, cos) = degrees.get().to_radians().sin_cos();
            [
                1.0, 0.0, 0.0, 0.0, 0.0, cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]
        }
        ComputedTransformFunction::RotateY(degrees) => {
            let (sin, cos) = degrees.get().to_radians().sin_cos();
            [
                cos, 0.0, -sin, 0.0, 0.0, 1.0, 0.0, 0.0, sin, 0.0, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]
        }
        ComputedTransformFunction::RotateZ(degrees) => rotation_z(degrees.get()),
        ComputedTransformFunction::Scale { x, y, z } => scale(x.get(), y.get(), z.get()),
        ComputedTransformFunction::Skew {
            x_degrees,
            y_degrees,
        } => {
            let mut matrix = identity();
            matrix[4] = x_degrees.get().to_radians().tan();
            matrix[1] = y_degrees.get().to_radians().tan();
            matrix
        }
        ComputedTransformFunction::Matrix(values) => values.map(StyleNumber::get),
    };
    matrix
        .iter()
        .all(|value| value.is_finite())
        .then_some(matrix)
}

fn decompose(matrix: [f32; 16]) -> Option<DecomposedTransform> {
    if let Some(decomposed) = decompose_2d(matrix) {
        return Some(decomposed);
    }

    let mut matrix = matrix;
    let normalization = matrix[15];
    if !normalization.is_finite() || normalization.abs() <= f32::EPSILON {
        return None;
    }
    for value in &mut matrix {
        *value /= normalization;
    }

    let mut perspective_matrix = matrix;
    perspective_matrix[3] = 0.0;
    perspective_matrix[7] = 0.0;
    perspective_matrix[11] = 0.0;
    perspective_matrix[15] = 1.0;
    let inverse_perspective = inverse(perspective_matrix)?;

    let mut output = DecomposedTransform::default();
    if matrix[3] != 0.0 || matrix[7] != 0.0 || matrix[11] != 0.0 {
        let right = [matrix[3], matrix[7], matrix[11], matrix[15]];
        for row in 0..4 {
            output.perspective[row] = (0..4)
                .map(|column| at(&inverse_perspective, column, row) * right[column])
                .sum();
        }
    }

    output.translate = [matrix[12], matrix[13], matrix[14]];
    let mut columns = [
        [matrix[0], matrix[1], matrix[2]],
        [matrix[4], matrix[5], matrix[6]],
        [matrix[8], matrix[9], matrix[10]],
    ];

    output.scale[0] = length3(columns[0]);
    if output.scale[0].abs() <= f32::EPSILON {
        return None;
    }
    scale_vec(&mut columns[0], 1.0 / output.scale[0]);

    output.skew[0] = dot3(columns[0], columns[1]);
    columns[1] = combine(columns[1], columns[0], 1.0, -output.skew[0]);
    output.scale[1] = length3(columns[1]);
    if output.scale[1].abs() <= f32::EPSILON {
        return None;
    }
    scale_vec(&mut columns[1], 1.0 / output.scale[1]);
    output.skew[0] /= output.scale[1];

    output.skew[1] = dot3(columns[0], columns[2]);
    columns[2] = combine(columns[2], columns[0], 1.0, -output.skew[1]);
    output.skew[2] = dot3(columns[1], columns[2]);
    columns[2] = combine(columns[2], columns[1], 1.0, -output.skew[2]);
    output.scale[2] = length3(columns[2]);
    if output.scale[2].abs() <= f32::EPSILON {
        return None;
    }
    scale_vec(&mut columns[2], 1.0 / output.scale[2]);
    output.skew[1] /= output.scale[2];
    output.skew[2] /= output.scale[2];

    if dot3(columns[0], cross3(columns[1], columns[2])) < 0.0 {
        for (scale, column) in output.scale.iter_mut().zip(&mut columns) {
            *scale *= -1.0;
            scale_vec(column, -1.0);
        }
    }

    let q_xx = columns[0][0];
    let q_xy = columns[1][0];
    let q_xz = columns[2][0];
    let q_yx = columns[0][1];
    let q_yy = columns[1][1];
    let q_yz = columns[2][1];
    let q_zx = columns[0][2];
    let q_zy = columns[1][2];
    let q_zz = columns[2][2];
    let trace = q_xx + q_yy + q_zz;
    output.quaternion = if trace > 0.0 {
        let r = (1.0 + trace).sqrt();
        let s = 0.5 / r;
        [
            (q_zy - q_yz) * s,
            (q_xz - q_zx) * s,
            (q_yx - q_xy) * s,
            0.5 * r,
        ]
    } else if q_xx > q_yy && q_xx > q_zz {
        let r = (1.0 + q_xx - q_yy - q_zz).sqrt();
        let s = 0.5 / r;
        [
            0.5 * r,
            (q_xy + q_yx) * s,
            (q_xz + q_zx) * s,
            (q_zy - q_yz) * s,
        ]
    } else if q_yy > q_zz {
        let r = (1.0 - q_xx + q_yy - q_zz).sqrt();
        let s = 0.5 / r;
        [
            (q_xy + q_yx) * s,
            0.5 * r,
            (q_yz + q_zy) * s,
            (q_xz - q_zx) * s,
        ]
    } else {
        let r = (1.0 - q_xx - q_yy + q_zz).sqrt();
        let s = 0.5 / r;
        [
            (q_xz + q_zx) * s,
            (q_yz + q_zy) * s,
            0.5 * r,
            (q_yx - q_xy) * s,
        ]
    };
    output
        .translate
        .iter()
        .chain(output.scale.iter())
        .chain(output.skew.iter())
        .chain(output.perspective.iter())
        .chain(output.quaternion.iter())
        .all(|value| value.is_finite())
        .then_some(output)
}

fn decompose_2d(matrix: [f32; 16]) -> Option<DecomposedTransform> {
    let is_2d = matrix[2] == 0.0
        && matrix[6] == 0.0
        && matrix[8] == 0.0
        && matrix[9] == 0.0
        && matrix[10] == 1.0
        && matrix[11] == 0.0
        && matrix[14] == 0.0
        && matrix[3] == 0.0
        && matrix[7] == 0.0
        && matrix[15] == 1.0;
    if !is_2d {
        return None;
    }
    let (mut m11, mut m12, mut m21, mut m22) = (matrix[0], matrix[1], matrix[4], matrix[5]);
    let determinant = m11 * m22 - m12 * m21;
    if determinant.abs() <= f32::EPSILON {
        return None;
    }
    let mut output = DecomposedTransform {
        translate: [matrix[12], matrix[13], 0.0],
        ..DecomposedTransform::default()
    };
    if determinant < 0.0 {
        if m11 < m22 {
            output.scale[0] = -1.0;
        } else {
            output.scale[1] = -1.0;
        }
    }
    output.scale[0] *= (m11 * m11 + m12 * m12).sqrt();
    if output.scale[0].abs() <= f32::EPSILON {
        return None;
    }
    m11 /= output.scale[0];
    m12 /= output.scale[0];
    let scaled_shear = m11 * m21 + m12 * m22;
    m21 -= m11 * scaled_shear;
    m22 -= m12 * scaled_shear;
    output.scale[1] *= (m21 * m21 + m22 * m22).sqrt();
    if output.scale[1].abs() <= f32::EPSILON {
        return None;
    }
    output.skew[0] = scaled_shear / output.scale[1];
    let angle = m12.atan2(m11) * 0.5;
    output.quaternion = [0.0, 0.0, angle.sin(), angle.cos()];
    Some(output)
}

fn blend(from: DecomposedTransform, to: DecomposedTransform, progress: f32) -> DecomposedTransform {
    let blend_array = |from: &[f32], to: &[f32], output: &mut [f32]| {
        for ((output, from), to) in output.iter_mut().zip(from).zip(to) {
            *output = *from + (*to - *from) * progress;
        }
    };
    let mut output = DecomposedTransform::default();
    blend_array(&from.translate, &to.translate, &mut output.translate);
    blend_array(&from.scale, &to.scale, &mut output.scale);
    blend_array(&from.skew, &to.skew, &mut output.skew);
    blend_array(&from.perspective, &to.perspective, &mut output.perspective);
    output.quaternion = slerp(from.quaternion, to.quaternion, progress);
    output
}

fn compose(value: DecomposedTransform) -> [f32; 16] {
    let mut matrix = identity();
    matrix[3] = value.perspective[0];
    matrix[7] = value.perspective[1];
    matrix[11] = value.perspective[2];
    matrix[15] = value.perspective[3];
    matrix = multiply(
        matrix,
        translation(value.translate[0], value.translate[1], value.translate[2]),
    );
    matrix = multiply(matrix, quaternion_matrix(value.quaternion));
    if value.skew[2] != 0.0 {
        let mut skew = identity();
        skew[9] = value.skew[2];
        matrix = multiply(matrix, skew);
    }
    if value.skew[1] != 0.0 {
        let mut skew = identity();
        skew[8] = value.skew[1];
        matrix = multiply(matrix, skew);
    }
    if value.skew[0] != 0.0 {
        let mut skew = identity();
        skew[4] = value.skew[0];
        matrix = multiply(matrix, skew);
    }
    multiply(
        matrix,
        scale(value.scale[0], value.scale[1], value.scale[2]),
    )
}

fn slerp(mut from: [f32; 4], to: [f32; 4], progress: f32) -> [f32; 4] {
    let mut cosine = dot4(from, to);
    if cosine < 0.0 {
        for value in &mut from {
            *value = -*value;
        }
        cosine = -cosine;
    }
    cosine = cosine.min(1.0);
    let sine = (1.0 - cosine * cosine).max(0.0).sqrt();
    if sine < 1.0e-5 {
        return from;
    }
    let angle = cosine.acos();
    let from_scale = ((1.0 - progress) * angle).sin() / sine;
    let to_scale = (progress * angle).sin() / sine;
    std::array::from_fn(|index| from[index] * from_scale + to[index] * to_scale)
}

fn quaternion_matrix([x, y, z, w]: [f32; 4]) -> [f32; 16] {
    [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        0.0,
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        0.0,
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

fn inverse(matrix: [f32; 16]) -> Option<[f32; 16]> {
    let mut augmented = [[0.0_f64; 8]; 4];
    for (row, values) in augmented.iter_mut().enumerate() {
        for (column, value) in values[..4].iter_mut().enumerate() {
            *value = f64::from(at(&matrix, row, column));
        }
        values[4 + row] = 1.0;
    }
    for column in 0..4 {
        let pivot = (column..4).max_by(|left, right| {
            augmented[*left][column]
                .abs()
                .total_cmp(&augmented[*right][column].abs())
        })?;
        if augmented[pivot][column].abs() < 1.0e-8 {
            return None;
        }
        augmented.swap(column, pivot);
        let divisor = augmented[column][column];
        for value in &mut augmented[column] {
            *value /= divisor;
        }
        let pivot_row = augmented[column];
        for (row, values) in augmented.iter_mut().enumerate() {
            if row == column {
                continue;
            }
            let factor = values[column];
            for (value, pivot_value) in values.iter_mut().zip(pivot_row) {
                *value -= factor * pivot_value;
            }
        }
    }
    let mut output = [0.0; 16];
    for row in 0..4 {
        for column in 0..4 {
            output[column * 4 + row] = augmented[row][4 + column] as f32;
        }
    }
    output
        .iter()
        .all(|value| value.is_finite())
        .then_some(output)
}

fn at(matrix: &[f32; 16], row: usize, column: usize) -> f32 {
    matrix[column * 4 + row]
}

fn identity() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn translation(x: f32, y: f32, z: f32) -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0,
    ]
}

fn scale(x: f32, y: f32, z: f32) -> [f32; 16] {
    [
        x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn rotation_z(degrees: f32) -> [f32; 16] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    [
        cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn multiply(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    let mut output = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            output[column * 4 + row] = (0..4)
                .map(|index| left[index * 4 + row] * right[column * 4 + index])
                .sum();
        }
    }
    output
}

fn length3(value: [f32; 3]) -> f32 {
    dot3(value, value).sqrt()
}

fn dot3(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn dot4(left: [f32; 4], right: [f32; 4]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2] + left[3] * right[3]
}

fn cross3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn combine(left: [f32; 3], right: [f32; 3], left_scale: f32, right_scale: f32) -> [f32; 3] {
    std::array::from_fn(|index| left[index] * left_scale + right[index] * right_scale)
}

fn scale_vec(value: &mut [f32; 3], scale: f32) {
    for component in value {
        *component *= scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(functions: Vec<ComputedTransformFunction>) -> ComputedTransformStyle {
        ComputedTransformStyle {
            functions,
            ..ComputedTransformStyle::default()
        }
    }

    #[test]
    fn mismatched_2d_lists_decompose_and_blend() {
        let from = style(vec![ComputedTransformFunction::RotateZ(StyleNumber::new(
            0.0,
        ))]);
        let to = style(vec![ComputedTransformFunction::Translate {
            x: ComputedLengthPercentage::new(100.0, 0.0),
            y: ComputedLengthPercentage::ZERO,
            z: StyleNumber::new(0.0),
        }]);
        let current = interpolate_transform_style(&from, &to, 0.5, 200.0, 100.0).unwrap();
        let ComputedTransformFunction::Matrix(matrix) = &current.functions[0] else {
            panic!("mismatched suffix must become a matrix");
        };
        assert!((matrix[12].get() - 50.0).abs() < 0.0001);
        assert!((matrix[0].get() - 1.0).abs() < 0.0001);
        assert!((matrix[5].get() - 1.0).abs() < 0.0001);
    }

    #[test]
    fn matrix_pairs_use_decomposition_instead_of_element_wise_blending() {
        let from = style(vec![ComputedTransformFunction::Matrix(
            identity().map(StyleNumber::new),
        )]);
        let to = style(vec![ComputedTransformFunction::Matrix(
            multiply(translation(100.0, 40.0, 0.0), rotation_z(90.0)).map(StyleNumber::new),
        )]);
        let current = interpolate_transform_style(&from, &to, 0.5, 100.0, 100.0).unwrap();
        let ComputedTransformFunction::Matrix(matrix) = &current.functions[0] else {
            panic!("matrix interpolation must remain a matrix");
        };
        let root_half = std::f32::consts::FRAC_1_SQRT_2;
        assert!((matrix[0].get() - root_half).abs() < 0.0001);
        assert!((matrix[1].get() - root_half).abs() < 0.0001);
        assert!((matrix[12].get() - 50.0).abs() < 0.0001);
        assert!((matrix[13].get() - 20.0).abs() < 0.0001);
    }

    #[test]
    fn singular_matrix_refuses_decomposition() {
        let from = style(vec![ComputedTransformFunction::Matrix(
            identity().map(StyleNumber::new),
        )]);
        let to = style(vec![ComputedTransformFunction::Matrix(
            [0.0; 16].map(StyleNumber::new),
        )]);
        assert!(interpolate_transform_style(&from, &to, 0.5, 100.0, 100.0).is_none());
    }
}
