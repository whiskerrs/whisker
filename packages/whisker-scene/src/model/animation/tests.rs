use super::*;

fn channel(interpolation: Interpolation, property: Property, values: Vec<f32>) -> Channel {
    Channel::new(0, property, interpolation, vec![0.0, 1.0], values)
}

#[test]
fn linear_and_step_sampling_hold_outside_the_keys() {
    let linear = channel(
        Interpolation::Linear,
        Property::Translation,
        vec![0.0, 0.0, 0.0, 2.0, 4.0, 6.0],
    );
    assert_eq!(linear.sample(0.25), [0.5, 1.0, 1.5]);
    assert_eq!(linear.sample(-1.0), [0.0, 0.0, 0.0]);
    assert_eq!(linear.sample(3.0), [2.0, 4.0, 6.0]);
    let step = channel(
        Interpolation::Step,
        Property::Translation,
        vec![0.0, 0.0, 0.0, 2.0, 4.0, 6.0],
    );
    assert_eq!(step.sample(0.99), [0.0, 0.0, 0.0]);
}

#[test]
fn rotations_slerp() {
    let quarter = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let mut values = Quat::IDENTITY.to_array().to_vec();
    values.extend(quarter.to_array());
    let sampled = channel(Interpolation::Linear, Property::Rotation, values).sample(0.5);
    let q = Quat::from_slice(&sampled);
    assert!(
        q.abs_diff_eq(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4), 1e-5),
        "{q}"
    );
}

#[test]
fn cubic_splines_pass_through_their_keys_and_follow_tangents() {
    // Per key: in-tangent, value, out-tangent.
    let values = vec![0.0, 0.0, 1.0, 0.0, 1.0, 0.0];
    let cubic = channel(Interpolation::CubicSpline, Property::Weights, values);
    assert_eq!(cubic.sample(0.0), [0.0]);
    assert_eq!(cubic.sample(1.0), [1.0]);
    // h00·0 + h10·1·1 + h01·1 + h11·0 at s = 0.5.
    assert!((cubic.sample(0.5)[0] - 0.625).abs() < 1e-6);
}

#[test]
fn clip_duration_is_the_last_key() {
    let clip = Clip::new(
        "Walk".into(),
        vec![Channel::new(
            0,
            Property::Scale,
            Interpolation::Linear,
            vec![0.0, 2.5],
            vec![1.0; 6],
        )],
    );
    assert_eq!(clip.duration, 2.5);
}
